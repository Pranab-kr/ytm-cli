//! The event loop. Owns AppState; nothing here may block on I/O (NFR-2).

use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::Stdout;
use std::sync::Arc;
use tokio::sync::mpsc;
use ytm_core::MusicSource;
use ytm_player::player::{Player, PlayerCommand};
use ytm_tui::{
    app::{AppState, Pane, ToastKind},
    event::{AppEvent, InputAction},
    keymap::KeyMap,
    search_state::SearchDebounce,
    theme::Theme,
};

/// Seek step in seconds (FR-P4).
const SEEK_STEP: i64 = 5;
/// Volume step per key press.
const VOLUME_STEP: i64 = 5;

/// Work the loop should start in the background as a result of an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    LoadPlaylists,
    LoadSongs,
    LoadAlbums,
    LoadArtists,
    OpenPlaylist(ytm_core::PlaylistId),
    Search(String),
}

/// Translate one input action into player commands, state changes, and
/// background work. Pure with respect to I/O — that is what makes it testable.
pub fn dispatch_input(
    action: InputAction,
    state: &mut AppState,
    _source: &Arc<impl MusicSource + ?Sized>,
    player: &impl Player,
) -> Option<Task> {
    use InputAction as A;

    // A modal owns the keyboard, transport included; let the state handle it.
    if state.modal.is_some() {
        state.apply(AppEvent::Input(action));
        return None;
    }

    // Transport actions belong to the player; everything else to the state.
    match action {
        A::TogglePause => {
            send(player, PlayerCommand::TogglePause);
        }
        A::NextTrack => {
            send(player, PlayerCommand::Next);
        }
        A::PrevTrack => {
            send(player, PlayerCommand::Previous);
        }
        A::SeekForward => {
            send(player, PlayerCommand::SeekRelative(SEEK_STEP));
        }
        A::SeekBack => {
            send(player, PlayerCommand::SeekRelative(-SEEK_STEP));
        }
        A::VolumeUp | A::VolumeDown => {
            let delta = if action == A::VolumeUp {
                VOLUME_STEP
            } else {
                -VOLUME_STEP
            };
            // Set it locally too: the bar should move on the next frame rather
            // than waiting for the actor's VolumeChanged to come back.
            state.volume = ytm_player::player::clamp_volume(state.volume as i64 + delta);
            send(player, PlayerCommand::SetVolume(state.volume));
        }
        A::ToggleMute => {
            state.muted = !state.muted;
            send(player, PlayerCommand::ToggleMute);
        }
        A::ToggleShuffle => {
            state.shuffle = !state.shuffle;
            send(player, PlayerCommand::SetShuffle(state.shuffle));
        }
        A::CycleRepeat => {
            state.repeat = state.repeat.next();
            send(player, PlayerCommand::SetRepeat(state.repeat));
        }
        A::Confirm => {
            // In the playlist list, Enter opens; on a track, Enter plays.
            if let Some(p) = state.selected_playlist() {
                return start(state, Task::OpenPlaylist(p.id.clone()));
            }
            if let Some(t) = state.selected_track().cloned() {
                send(player, PlayerCommand::PlayNow(t));
            }
        }
        A::AddToQueue => {
            if let Some(t) = state.selected_track().cloned() {
                send(player, PlayerCommand::EnqueueBack(vec![t]));
            }
        }
        A::PlayNext => {
            if let Some(t) = state.selected_track().cloned() {
                send(player, PlayerCommand::EnqueueNext(vec![t]));
            }
        }
        // Queue edits. All three are queue-pane-only: `x` means
        // remove-from-playlist elsewhere (Task 32), and there is no server-side
        // track reordering to bind `J`/`K` to (out of scope).
        //
        // None of them touch `state.queue`. The actor owns queue truth and
        // answers with `QueueChanged`; a local edit would leave the view
        // showing an order the player disagrees with.
        A::RemoveFromPlaylist if state.pane == Pane::Queue => {
            if state.selected < state.queue.len() {
                send(player, PlayerCommand::RemoveFromQueue(state.selected));
            }
        }
        A::MoveEntryUp | A::MoveEntryDown if state.pane == Pane::Queue => {
            let from = state.selected;
            let to = if action == A::MoveEntryUp {
                from.checked_sub(1)
            } else {
                Some(from + 1).filter(|t| *t < state.queue.len())
            };
            // Out of range at either end: the actor would index past the queue.
            if let Some(to) = to {
                send(player, PlayerCommand::MoveInQueue { from, to });
                // Follow the entry rather than the row, or a held key would
                // walk the selection back over the track it just moved.
                state.selected = to;
            }
        }
        A::MoveEntryUp | A::MoveEntryDown => {}
        A::ClearQueue if state.pane == Pane::Queue => {
            send(player, PlayerCommand::ClearQueue);
            state.selected = 0;
        }
        A::ClearQueue => {}
        A::Refresh => {
            let task = match state.pane {
                Pane::Songs => Task::LoadSongs,
                Pane::Albums => Task::LoadAlbums,
                Pane::Artists => Task::LoadArtists,
                _ => Task::LoadPlaylists,
            };
            return start(state, task);
        }
        // Everything else is a state transition.
        other => state.apply(AppEvent::Input(other)),
    }
    None
}

/// Record the current query on every keystroke, restarting the debounce timer.
///
/// Reads the query from `AppState` rather than taking the character, so the
/// buffer stays owned by the reducer and this cannot disagree with it.
/// Keystrokes outside the search pane are ignored — nothing else edits the
/// query, and noting them would schedule a search the user never asked for.
pub fn note_search_input(d: &mut SearchDebounce, state: &AppState, now_ms: u64) {
    if state.pane == Pane::Search {
        d.note_input(&state.search_query, now_ms);
    }
}

/// Call on every tick. Returns a search to run once typing has settled.
pub fn search_tick(d: &mut SearchDebounce, state: &mut AppState, now_ms: u64) -> Option<Task> {
    let q = d.should_fire(now_ms)?;
    start(state, Task::Search(q))
}

/// A dropped command means the actor thread is gone. There is nothing useful to
/// do about it from here, so log it rather than unwrapping into a panic that
/// would take the terminal down with it.
fn send(player: &impl Player, cmd: PlayerCommand) {
    if let Err(e) = player.send(cmd) {
        tracing::error!(error = %e, "player command dropped");
    }
}

/// Run until the user quits. Four event sources, one owner of state.
///
/// Nothing in this function awaits network or audio work: every slow thing is
/// handed to `spawn_task` or the player actor and comes back as an `AppEvent`
/// (NFR-2). The `select!` arms are all cheap.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut state: AppState,
    source: Arc<dyn MusicSource>,
    player: impl Player,
    mut player_events: mpsc::UnboundedReceiver<ytm_player::player::PlayerEvent>,
    keymap: KeyMap,
    theme: Theme,
    tick_ms: u64,
    cookie_auth: bool,
) -> color_eyre::Result<()> {
    use crossterm::event::{Event as CtEvent, EventStream, KeyEventKind};
    use futures::StreamExt;

    // App-internal events (results of background work).
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let mut term_events = EventStream::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(tick_ms.max(1)));
    let started = std::time::Instant::now();
    let mut debounce = SearchDebounce::default();

    // Paint immediately, then load — NFR-1 depends on not awaiting first.
    terminal.draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap))?;
    state.loading = true;
    spawn_task(Task::LoadPlaylists, source.clone(), app_tx.clone());

    loop {
        tokio::select! {
            // Terminal input
            Some(Ok(ev)) = term_events.next() => {
                match ev {
                    CtEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        if let Some(a) = keymap.resolve(k, state.focus) {
                            if let Some(task) = dispatch_input(a, &mut state, &source, &player) {
                                spawn_task(task, source.clone(), app_tx.clone());
                            }
                            // Restart the debounce timer, so the search fires
                            // from the tick arm once typing stops.
                            state.elapsed_ms = started.elapsed().as_millis() as u64;
                            note_search_input(&mut debounce, &state, state.elapsed_ms);
                        }
                    }
                    CtEvent::Resize(_, _) => state.apply(AppEvent::Resize),
                    _ => {}
                }
            }

            // Player actor
            Some(pe) = player_events.recv() => state.apply(AppEvent::Player(pe)),

            // Background work results
            Some(ae) = app_rx.recv() => {
                if let Some(hint) = empty_library_hint(cookie_auth, &ae) {
                    state.push_toast(ToastKind::Info, &hint, state.elapsed_ms);
                }
                state.apply(ae);
            }

            // Render tick
            _ = ticker.tick() => {
                let now_ms = started.elapsed().as_millis() as u64;
                state.elapsed_ms = now_ms;
                state.apply(AppEvent::Tick);
                if let Some(task) = search_tick(&mut debounce, &mut state, now_ms) {
                    spawn_task(task, source.clone(), app_tx.clone());
                }
            }
        }

        terminal.draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap))?;

        if state.should_quit {
            send(&player, PlayerCommand::Shutdown);
            return Ok(());
        }
    }
}

/// Run one unit of work off-thread and post the result back.
fn spawn_task(task: Task, source: Arc<dyn MusicSource>, tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        // Logged both ways: nothing about a background fetch is visible on
        // screen, so the log is the only place to see one succeed or fail.
        tracing::debug!(?task, "background task started");
        let started = std::time::Instant::now();
        let ev = match task {
            Task::LoadPlaylists => match source.library_playlists().await {
                Ok(v) => AppEvent::PlaylistsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadSongs => match source.library_songs().await {
                Ok(v) => AppEvent::LibrarySongsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadAlbums => match source.library_albums().await {
                Ok(v) => AppEvent::AlbumsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::LoadArtists => match source.library_artists().await {
                Ok(v) => AppEvent::ArtistsLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::OpenPlaylist(id) => match source.playlist_tracks(id.clone()).await {
                Ok(tracks) => AppEvent::PlaylistTracksLoaded { id, tracks },
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::Search(q) => match source.search_songs(q.clone()).await {
                Ok(tracks) => AppEvent::SearchResults { query: q, tracks },
                Err(e) => AppEvent::Error(e.to_string()),
            },
        };
        match &ev {
            AppEvent::Error(m) => tracing::warn!(error = %m, "background task failed"),
            other => tracing::info!(
                event = event_name(other),
                rows = event_rows(other),
                ms = started.elapsed().as_millis() as u64,
                "background task finished"
            ),
        }
        let _ = tx.send(ev);
    });
}

/// Log-friendly names, so a load is legible in the log without a Debug dump of
/// every track.
fn event_name(ev: &AppEvent) -> &'static str {
    match ev {
        AppEvent::PlaylistsLoaded(_) => "playlists",
        AppEvent::LibrarySongsLoaded(_) => "songs",
        AppEvent::AlbumsLoaded(_) => "albums",
        AppEvent::ArtistsLoaded(_) => "artists",
        AppEvent::PlaylistTracksLoaded { .. } => "playlist_tracks",
        AppEvent::SearchResults { .. } => "search",
        _ => "other",
    }
}

fn event_rows(ev: &AppEvent) -> usize {
    match ev {
        AppEvent::PlaylistsLoaded(v) => v.len(),
        AppEvent::LibrarySongsLoaded(v) => v.len(),
        AppEvent::AlbumsLoaded(v) => v.len(),
        AppEvent::ArtistsLoaded(v) => v.len(),
        AppEvent::PlaylistTracksLoaded { tracks, .. } => tracks.len(),
        AppEvent::SearchResults { tracks, .. } => tracks.len(),
        _ => 0,
    }
}

/// A successful library call that returned zero rows, under cookie auth, is the
/// only signal an expired cookie gives us (FR-A6).
///
/// There is no auth error to catch: InnerTube answers an expired cookie with
/// HTTP 200 and a signed-out page, which `ytmapi-rs` parses faithfully into an
/// empty list. So "empty" and "expired" are indistinguishable from here, and the
/// honest thing is to name both possibilities rather than show a bare empty
/// list. Restricted to cookie auth because telling an OAuth user to re-export
/// cookies they do not have would be noise.
pub fn empty_library_hint(cookie_auth: bool, ev: &AppEvent) -> Option<String> {
    if !cookie_auth {
        return None;
    }
    let empty = match ev {
        AppEvent::PlaylistsLoaded(v) => v.is_empty(),
        AppEvent::LibrarySongsLoaded(v) => v.is_empty(),
        AppEvent::AlbumsLoaded(v) => v.is_empty(),
        AppEvent::ArtistsLoaded(v) => v.is_empty(),
        _ => false,
    };
    empty.then(|| {
        "library came back empty — if that is wrong, the cookie expired; re-export it".to_owned()
    })
}

/// Mark the spinner before handing work off, so FR-U4 holds for the whole
/// round trip rather than starting when the answer arrives.
fn start(state: &mut AppState, task: Task) -> Option<Task> {
    state.loading = true;
    Some(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ytm_core::mock::MockSource;
    // `Player` itself arrives via `use super::*`.
    use ytm_player::{mock::MockPlayer, player::PlayerCommand};
    use ytm_tui::{
        app::{AppState, Focus, Pane},
        event::{AppEvent, InputAction},
    };

    fn deps() -> (Arc<MockSource>, Arc<MockPlayer>) {
        let (p, _rx) = MockPlayer::new();
        (Arc::new(MockSource::new()), Arc::new(p))
    }

    #[test]
    fn toggle_pause_reaches_the_player_not_the_state() {
        let (src, player) = deps();
        let mut s = AppState::default();
        dispatch_input(InputAction::TogglePause, &mut s, &src, &*player);
        assert!(matches!(player.commands()[0], PlayerCommand::TogglePause));
    }

    #[test]
    fn volume_up_clamps_at_one_hundred() {
        let (src, player) = deps();
        let mut s = AppState {
            volume: 97,
            ..Default::default()
        };
        dispatch_input(InputAction::VolumeUp, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetVolume(v) => assert_eq!(v, 100),
            ref o => panic!("expected SetVolume, got {o:?}"),
        }
    }

    #[test]
    fn volume_down_clamps_at_zero() {
        let (src, player) = deps();
        let mut s = AppState {
            volume: 2,
            ..Default::default()
        };
        dispatch_input(InputAction::VolumeDown, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetVolume(v) => assert_eq!(v, 0),
            ref o => panic!("expected SetVolume, got {o:?}"),
        }
    }

    #[test]
    fn cycle_repeat_advances_the_mode() {
        let (src, player) = deps();
        let mut s = AppState::default();
        dispatch_input(InputAction::CycleRepeat, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::SetRepeat(m) => {
                assert_eq!(m, ytm_player::player::RepeatMode::One)
            }
            ref o => panic!("expected SetRepeat, got {o:?}"),
        }
    }

    #[test]
    fn confirm_on_a_selected_track_plays_it() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v7", "Song")],
            selected: 0,
            ..Default::default()
        };
        dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        match &player.commands()[0] {
            PlayerCommand::PlayNow(t) => assert_eq!(t.video_id.as_str(), "v7"),
            o => panic!("expected PlayNow, got {o:?}"),
        }
    }

    #[test]
    fn confirm_with_an_empty_list_sends_nothing() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            ..Default::default()
        };
        dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        assert!(
            player.commands().is_empty(),
            "must not play a track that does not exist"
        );
    }

    #[test]
    fn add_to_queue_enqueues_the_selected_track() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            ..Default::default()
        };
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        assert!(matches!(
            player.commands()[0],
            PlayerCommand::EnqueueBack(_)
        ));
    }

    #[test]
    fn an_empty_library_under_cookie_auth_hints_at_an_expired_cookie() {
        // An expired cookie is NOT an auth error: InnerTube answers HTTP 200
        // with a signed-out page, so the library parses as zero rows. Without
        // this hint the only symptom is an empty list. See PROGRESS.md.
        let ev = AppEvent::PlaylistsLoaded(vec![]);
        let hint = empty_library_hint(true, &ev).expect("cookie auth must hint");
        assert!(hint.contains("re-export"), "got {hint:?}");
    }

    #[test]
    fn a_populated_library_never_hints() {
        let ev = AppEvent::PlaylistsLoaded(vec![ytm_core::Playlist::stub("p1", "Focus")]);
        assert!(empty_library_hint(true, &ev).is_none());
    }

    #[test]
    fn oauth_auth_does_not_get_the_cookie_hint() {
        // A genuinely empty account under OAuth has a different diagnosis;
        // telling the user to re-export cookies they do not use is noise.
        let ev = AppEvent::PlaylistsLoaded(vec![]);
        assert!(empty_library_hint(false, &ev).is_none());
    }

    #[test]
    fn typing_a_query_fires_one_search_after_the_pause_not_per_keystroke() {
        // FR-S2: three keystrokes must cost one request, not three.
        let (src, player) = deps();
        let mut d = ytm_tui::search_state::SearchDebounce::new(300);
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            ..Default::default()
        };

        let mut fired = Vec::new();
        for (i, c) in "boa".chars().enumerate() {
            let now = 1000 + i as u64 * 50;
            dispatch_input(InputAction::Char(c), &mut s, &src, &*player);
            note_search_input(&mut d, &s, now);
            // A tick between keystrokes is too soon to fire.
            if let Some(t) = search_tick(&mut d, &mut s, now + 10) {
                fired.push(t);
            }
        }
        assert!(fired.is_empty(), "fired mid-typing: {fired:?}");

        let task = search_tick(&mut d, &mut s, 1500).expect("must fire after the pause");
        assert_eq!(task, Task::Search("boa".into()));
        assert!(s.loading, "the spinner must show while the search runs");
    }

    #[test]
    fn a_settled_query_does_not_fire_again_on_every_tick() {
        let (src, player) = deps();
        let mut d = ytm_tui::search_state::SearchDebounce::new(300);
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            ..Default::default()
        };
        dispatch_input(InputAction::Char('x'), &mut s, &src, &*player);
        note_search_input(&mut d, &s, 1000);
        assert!(search_tick(&mut d, &mut s, 1400).is_some());
        for now in [1500, 1600, 5000] {
            assert_eq!(search_tick(&mut d, &mut s, now), None, "re-fired at {now}");
        }
    }

    #[test]
    fn keystrokes_outside_the_search_pane_never_schedule_a_search() {
        let (src, player) = deps();
        let mut d = ytm_tui::search_state::SearchDebounce::new(300);
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            ..Default::default()
        };
        dispatch_input(InputAction::Down, &mut s, &src, &*player);
        note_search_input(&mut d, &s, 1000);
        assert_eq!(search_tick(&mut d, &mut s, 2000), None);
    }

    fn queue_of_three() -> AppState {
        AppState {
            pane: Pane::Queue,
            queue: vec![
                ytm_core::Track::stub("v1", "A"),
                ytm_core::Track::stub("v2", "B"),
                ytm_core::Track::stub("v3", "C"),
            ],
            focus: Focus::Main,
            selected: 1,
            ..Default::default()
        }
    }

    #[test]
    fn x_in_the_queue_removes_the_selected_entry() {
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::RemoveFromPlaylist, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::RemoveFromQueue(i) => assert_eq!(i, 1),
            ref o => panic!("expected RemoveFromQueue, got {o:?}"),
        }
    }

    #[test]
    fn removing_a_queue_entry_does_not_mutate_the_local_queue() {
        // The actor owns queue truth and answers with QueueChanged. Editing
        // `state.queue` here would show a row count the player disagrees with.
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::RemoveFromPlaylist, &mut s, &src, &*player);
        assert_eq!(s.queue.len(), 3, "the view must wait for QueueChanged");
    }

    #[test]
    fn x_outside_the_queue_does_not_touch_the_queue() {
        // In a playlist, `x` means remove-from-playlist (Task 32), not dequeue.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            ..Default::default()
        };
        dispatch_input(InputAction::RemoveFromPlaylist, &mut s, &src, &*player);
        assert!(player.commands().is_empty());
    }

    #[test]
    fn moving_an_entry_down_swaps_it_with_the_next_one() {
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::MoveEntryDown, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::MoveInQueue { from, to } => assert_eq!((from, to), (1, 2)),
            ref o => panic!("expected MoveInQueue, got {o:?}"),
        }
        assert_eq!(s.selected, 2, "the selection follows the entry it moved");
    }

    #[test]
    fn moving_an_entry_up_swaps_it_with_the_previous_one() {
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::MoveEntryUp, &mut s, &src, &*player);
        match player.commands()[0] {
            PlayerCommand::MoveInQueue { from, to } => assert_eq!((from, to), (1, 0)),
            ref o => panic!("expected MoveInQueue, got {o:?}"),
        }
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn an_entry_cannot_be_moved_off_either_end() {
        let (src, player) = deps();
        let mut top = AppState {
            selected: 0,
            ..queue_of_three()
        };
        dispatch_input(InputAction::MoveEntryUp, &mut top, &src, &*player);
        let mut bottom = AppState {
            selected: 2,
            ..queue_of_three()
        };
        dispatch_input(InputAction::MoveEntryDown, &mut bottom, &src, &*player);
        assert!(
            player.commands().is_empty(),
            "an out-of-range move would panic the actor"
        );
        assert_eq!(top.selected, 0);
        assert_eq!(bottom.selected, 2);
    }

    #[test]
    fn reordering_outside_the_queue_is_ignored() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![
                ytm_core::Track::stub("v1", "A"),
                ytm_core::Track::stub("v2", "B"),
            ],
            ..Default::default()
        };
        dispatch_input(InputAction::MoveEntryDown, &mut s, &src, &*player);
        assert!(
            player.commands().is_empty(),
            "there is no server-side track order to change (out of scope)"
        );
    }

    #[test]
    fn the_clear_binding_empties_the_queue() {
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::ClearQueue, &mut s, &src, &*player);
        assert!(matches!(player.commands()[0], PlayerCommand::ClearQueue));
    }

    #[test]
    fn clearing_resets_the_selection_so_it_cannot_dangle() {
        let (src, player) = deps();
        let mut s = queue_of_three();
        dispatch_input(InputAction::ClearQueue, &mut s, &src, &*player);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn navigation_actions_do_not_reach_the_player() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![
                ytm_core::Track::stub("a", "A"),
                ytm_core::Track::stub("b", "B"),
            ],
            ..Default::default()
        };
        dispatch_input(InputAction::Down, &mut s, &src, &*player);
        assert!(player.commands().is_empty());
        assert_eq!(s.selected, 1, "state handles navigation");
    }
}
