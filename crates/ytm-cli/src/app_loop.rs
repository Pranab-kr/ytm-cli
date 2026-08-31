//! The event loop. Owns AppState; nothing here may block on I/O (NFR-2).

use crate::mpris;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::Stdout;
use std::sync::Arc;
use tokio::sync::mpsc;
use ytm_core::MusicSource;
use ytm_player::player::{Player, PlayerCommand};
use ytm_tui::{
    app::{AppState, ConfirmAction, Modal, Pane, PromptAction, ToastKind},
    event::{AppEvent, InputAction},
    keymap::KeyMap,
    mutation::Mutation,
    search_state::SearchDebounce,
    theme::Theme,
};

/// What a config reload produced. Both are rebuilt together: a `[keys]` change
/// and a `[ui] theme` change land in the same file.
pub struct ReloadedConfig {
    pub keymap: KeyMap,
    pub theme: Theme,
    pub theme_name: String,
}

/// Suspend the TUI, open `$EDITOR` on the config, and reload on exit (`,`).
///
/// Returns `Ok(None)` when there is nothing to apply — no editor configured, or
/// the user quit without saving. A parse error is returned rather than applied,
/// so a typo leaves the running keymap and theme intact instead of resetting
/// them to defaults mid-session.
///
/// The alternate screen and raw mode have to be released around the child: an
/// editor drawing into our screen buffer inherits a terminal it cannot use, and
/// leaving raw mode on means it never sees a newline.
pub fn edit_config_in_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    path: &std::path::Path,
) -> color_eyre::Result<Option<ReloadedConfig>> {
    use crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };

    let Some(editor) = editor_command() else {
        return Err(color_eyre::eyre::eyre!(
            "set $EDITOR (or $VISUAL) to edit the config from here"
        ));
    };

    // Written on demand so the editor always opens on something real, and the
    // user gets the documented defaults rather than an empty buffer.
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, crate::config::EXAMPLE_TOML)?;
    }
    let before = std::fs::read_to_string(path).unwrap_or_default();

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;

    // Split so `EDITOR="code -w"` works, not just a bare binary name.
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(bin)
        .args(parts)
        .arg(path)
        .status();

    // Restore the TUI before reporting anything: an error surfaces as a toast,
    // which needs the alternate screen back.
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Hide
    )?;
    terminal.clear()?;

    let status = status?;
    if !status.success() {
        return Err(color_eyre::eyre::eyre!("{bin} exited with {status}"));
    }
    let after = std::fs::read_to_string(path)?;
    if after == before {
        return Ok(None);
    }
    reload_config(&after).map(Some)
}

/// Rebuild the keymap and theme from config text. Separate from the editor so
/// it is testable without spawning a process.
pub fn reload_config(text: &str) -> color_eyre::Result<ReloadedConfig> {
    let cfg = crate::config::Config::from_toml_str(text)?;
    let keys_toml = toml::to_string(&cfg.keys)?;
    let keymap = KeyMap::from_toml_str(&keys_toml)?;
    let (theme, theme_name) = crate::config::resolve_theme(&cfg)?;
    Ok(ReloadedConfig {
        keymap,
        theme,
        theme_name,
    })
}

/// `$VISUAL` first, then `$EDITOR` — the conventional order.
pub fn editor_command() -> Option<String> {
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(v) = std::env::var(key)
            && !v.trim().is_empty()
        {
            return Some(v);
        }
    }
    None
}

/// Seek step in seconds (FR-P4).
const SEEK_STEP: i64 = 5;
/// Volume step per key press.
const VOLUME_STEP: i64 = 5;

/// Work the loop should start in the background as a result of an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// The home feed's recommendation shelves (FR-B6).
    LoadHome,
    LoadPlaylists,
    LoadSongs,
    LoadAlbums,
    LoadArtists,
    OpenPlaylist(ytm_core::PlaylistId),
    /// An artist's top tracks (FR-B7). Carries the name so the heading can be
    /// set without looking it up — an artist opened from search is not in
    /// `artists`, so there would be nothing to look up.
    OpenArtist {
        id: ytm_core::ArtistId,
        name: String,
    },
    Search(String),
    /// A server-side edit, carrying the token of the optimistic change it
    /// settles. Same spawn path as a read so the loop keeps one.
    Mutate {
        token: u64,
        task: MutationTask,
    },
}

/// Background work that changes server state.
///
/// Separate from `Task` because these carry a mutation token: the response has
/// to name the optimistic edit it settles, or a late failure would revert
/// whichever edit happened to be newest (FR-C6).
#[derive(Debug, Clone, PartialEq, Eq)]
// Delete is wired in Task 31, AddTracks/RemoveTracks in Tasks 31-32. Declared
// now because `run_mutation` handles all five, and a partial enum would mean
// touching its match again for every task.
#[allow(dead_code)]
pub enum MutationTask {
    Create {
        title: String,
        description: Option<String>,
        privacy: ytm_core::Privacy,
    },
    Rename {
        id: ytm_core::PlaylistId,
        title: String,
    },
    Delete {
        id: ytm_core::PlaylistId,
    },
    AddTracks {
        id: ytm_core::PlaylistId,
        videos: Vec<ytm_core::VideoId>,
    },
    RemoveTracks {
        id: ytm_core::PlaylistId,
        entries: Vec<ytm_core::SetVideoId>,
    },
}

/// Perform one mutation and report the outcome, tagged with its token.
pub async fn run_mutation(
    token: u64,
    task: MutationTask,
    source: Arc<dyn MusicSource>,
) -> AppEvent {
    let (result, ok_msg): (
        Result<Option<ytm_core::PlaylistId>, ytm_core::SourceError>,
        &str,
    ) = match task {
        MutationTask::Create {
            title,
            description,
            privacy,
        } => (
            source
                .create_playlist(title, description, privacy)
                .await
                .map(Some),
            "playlist created",
        ),
        MutationTask::Rename { id, title } => (
            source
                .edit_playlist(id, Some(title), None, None)
                .await
                .map(|_| None),
            "playlist renamed",
        ),
        MutationTask::Delete { id } => (
            source.delete_playlist(id).await.map(|_| None),
            "playlist deleted",
        ),
        MutationTask::AddTracks { id, videos } => (
            source.add_tracks(id, videos).await.map(|_| None),
            "added to playlist",
        ),
        MutationTask::RemoveTracks { id, entries } => (
            source.remove_tracks(id, entries).await.map(|_| None),
            "removed from playlist",
        ),
    };

    match result {
        Ok(real_id) => AppEvent::MutationOk {
            token,
            real_id,
            message: ok_msg.to_owned(),
        },
        Err(e) => AppEvent::MutationFailed {
            token,
            message: e.to_string(),
        },
    }
}

/// A temp id for an optimistic row, replaced by the server's on commit. Prefixed
/// so a leaked one is obvious in a log rather than looking like a real id.
fn temp_playlist_id(token: u64) -> ytm_core::PlaylistId {
    ytm_core::PlaylistId::from(format!("ytm-cli-temp-{token}").as_str())
}

/// Apply an open prompt and return the edit plus the API call it needs.
///
/// Validation happens here rather than in the modal: an empty name is refused
/// before any optimistic row appears, so there is nothing to roll back.
pub fn submit_prompt(state: &mut AppState) -> Option<(u64, MutationTask)> {
    let Some(Modal::Prompt { value, action, .. }) = state.modal.clone() else {
        return None;
    };
    let title = value.trim().to_owned();
    if title.is_empty() {
        state.push_toast(ToastKind::Error, "a name is required", state.elapsed_ms);
        return None;
    }
    state.modal = None;

    match action {
        PromptAction::CreatePlaylist => {
            // The id is a placeholder until MutationOk brings the real one.
            // Peek rather than take, so the id names the token that settles it.
            let temp = ytm_core::Playlist {
                title: title.clone(),
                ..ytm_core::Playlist::stub(
                    temp_playlist_id(state.pending.peek_token()).as_str(),
                    &title,
                )
            };
            let token = state.begin_mutation(Mutation::CreatePlaylist { temp });
            Some((
                token,
                MutationTask::Create {
                    title,
                    description: None,
                    privacy: ytm_core::Privacy::Private,
                },
            ))
        }
        PromptAction::RenamePlaylist(id) => {
            let previous = state.playlists.iter().find(|p| p.id == id)?.title.clone();
            let token = state.begin_mutation(Mutation::RenamePlaylist {
                id: id.clone(),
                previous,
                next: title.clone(),
            });
            Some((token, MutationTask::Rename { id, title }))
        }
    }
}

/// Open the create prompt. Always allowed — it depends on no selection.
pub fn open_create_prompt(state: &mut AppState) {
    state.modal = Some(Modal::Prompt {
        title: "New playlist name".to_owned(),
        value: String::new(),
        action: PromptAction::CreatePlaylist,
    });
}

/// Open the rename prompt for the selected playlist, pre-filled with its title.
///
/// Refuses a system playlist before any API call: FR-C2 does not apply to them,
/// and YouTube would reject the edit anyway — better to say so immediately than
/// to show an optimistic rename that snaps back a second later.
pub fn open_rename_prompt(state: &mut AppState) -> Option<ytm_core::PlaylistId> {
    let p = state.selected_playlist()?;
    if p.is_system {
        let msg = format!("\"{}\" cannot be renamed", p.title);
        state.push_toast(ToastKind::Error, &msg, state.elapsed_ms);
        return None;
    }
    let (id, title) = (p.id.clone(), p.title.clone());
    state.modal = Some(Modal::Prompt {
        title: "Rename playlist".to_owned(),
        value: title,
        action: PromptAction::RenamePlaylist(id.clone()),
    });
    Some(id)
}

/// Ask before deleting (FR-C3). Refuses system playlists outright.
pub fn open_delete_confirm(state: &mut AppState) {
    let Some(p) = state.selected_playlist() else {
        return;
    };
    if p.is_system {
        // No confirmation for an action that cannot succeed: asking "are you
        // sure?" about something impossible only wastes a keystroke.
        let msg = format!(
            "\"{}\" is managed by YouTube Music and cannot be deleted",
            p.title
        );
        state.push_toast(ToastKind::Error, &msg, state.elapsed_ms);
        return;
    }
    state.modal = Some(Modal::Confirm {
        text: format!("Delete \"{}\"? This cannot be undone.", p.title),
        action: ConfirmAction::DeletePlaylist(p.id),
    });
}

/// The user pressed `y`. Apply optimistically and hand back the work to do.
pub fn confirm_action(state: &mut AppState) -> Option<(u64, MutationTask)> {
    let Some(Modal::Confirm { action, .. }) = state.modal.take() else {
        return None;
    };
    match action {
        ConfirmAction::DeletePlaylist(id) => {
            let index = state.playlists.iter().position(|p| p.id == id)?;
            let snapshot = state.playlists[index].clone();
            let token = state.begin_mutation(Mutation::DeletePlaylist {
                id: id.clone(),
                index,
                snapshot,
            });
            Some((token, MutationTask::Delete { id }))
        }
        ConfirmAction::RemoveTracks { playlist, entries } => {
            // Indices come from the list as it stands, so rollback can put each
            // track back where it was.
            let removed: Vec<(usize, ytm_core::Track)> = state
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    t.set_video_id
                        .as_ref()
                        .is_some_and(|sv| entries.contains(sv))
                })
                .map(|(i, t)| (i, t.clone()))
                .collect();
            let token = state.begin_mutation(Mutation::RemoveTracks {
                playlist: playlist.clone(),
                removed,
            });
            Some((
                token,
                MutationTask::RemoveTracks {
                    id: playlist,
                    entries,
                },
            ))
        }
    }
}

/// Marked tracks if any, otherwise the selected one (FR-C4).
///
/// Ordered by the rows on screen, not by `marked`'s iteration order: it is a
/// `HashSet`, so returning it directly sent a marked run to the API scrambled.
pub fn targets_for_add(state: &AppState) -> Vec<ytm_core::VideoId> {
    if !state.marked.is_empty() {
        return state
            .track_rows()
            .iter()
            .map(|t| t.video_id.clone())
            .filter(|id| state.marked.contains(id))
            .collect();
    }
    state
        .selected_track()
        .map(|t| vec![t.video_id.clone()])
        .unwrap_or_default()
}

/// Full tracks for a queue action: the marked rows, else the selected one.
///
/// Returns tracks rather than ids because the player queues `Track`s, and
/// re-looking them up by id would be a second source of truth.
fn queue_targets(state: &AppState) -> Vec<ytm_core::Track> {
    if !state.marked.is_empty() {
        return state
            .track_rows()
            .iter()
            .filter(|t| state.marked.contains(&t.video_id))
            .cloned()
            .collect();
    }
    state.selected_track().into_iter().collect()
}

/// What the toast says. Names a single track; counts a selection, because
/// naming only the first of twelve reads as a bug.
fn enqueue_message(tracks: &[ytm_core::Track], next: bool) -> String {
    let where_to = if next {
        "playing next"
    } else {
        "added to queue"
    };
    match tracks {
        [one] => format!("{}: {}", where_to, one.title),
        many => format!("{} {} tracks", where_to, many.len()),
    }
}

/// Open the target-playlist picker for the marked or selected tracks.
///
/// Only editable playlists are offered: YouTube rejects an add to a system
/// playlist, so listing them would be offering an action that cannot work.
pub fn open_add_to_playlist(state: &mut AppState) {
    let targets = targets_for_add(state);
    if targets.is_empty() {
        state.push_toast(ToastKind::Error, "nothing selected", state.elapsed_ms);
        return;
    }
    let choices: Vec<(ytm_core::PlaylistId, String)> = state
        .playlists
        .iter()
        .filter(|p| !p.is_system)
        .map(|p| (p.id.clone(), p.title.clone()))
        .collect();
    if choices.is_empty() {
        state.push_toast(
            ToastKind::Error,
            "no editable playlist to add to — create one with N",
            state.elapsed_ms,
        );
        return;
    }
    state.modal = Some(Modal::PickPlaylist {
        targets,
        choices,
        selected: 0,
    });
}

/// The user picked a playlist. Hand back the add to run.
///
/// There is no optimistic row to show: the tracks go into a playlist that is
/// not necessarily the one on screen, so `Mutation::AddTracks` records the edit
/// for the toast and nothing else changes locally.
pub fn submit_pick(state: &mut AppState) -> Option<(u64, MutationTask)> {
    let Some(Modal::PickPlaylist {
        targets,
        choices,
        selected,
    }) = state.modal.take()
    else {
        return None;
    };
    let (id, _) = choices.get(selected)?.clone();
    let token = state.begin_mutation(Mutation::AddTracks {
        playlist: id.clone(),
        count: targets.len(),
    });
    // The marks were the input to this action; leaving them set would make the
    // next `A` silently repeat it.
    state.marked.clear();
    Some((
        token,
        MutationTask::AddTracks {
            id,
            videos: targets,
        },
    ))
}

/// Confirm before removing (FR-C5). Refuses when the entries are unidentifiable.
///
/// Playlist reads do carry `set_video_id` now — `playlist_raw` extracts it from
/// the wire JSON, which `ytmapi-rs` 0.3.3 drops (PROGRESS.md open question 1,
/// resolved via option A). The refusal is the fallback for when extraction finds
/// nothing: better to say so than to send a request that cannot work.
pub fn open_remove_confirm(state: &mut AppState) {
    let Some(playlist) = state.open_playlist.clone() else {
        state.push_toast(ToastKind::Error, "open a playlist first", state.elapsed_ms);
        return;
    };

    let targets = targets_for_add(state);
    let entries: Vec<ytm_core::SetVideoId> = state
        .tracks
        .iter()
        .filter(|t| targets.contains(&t.video_id))
        .filter_map(|t| t.set_video_id.clone())
        .collect();

    if entries.is_empty() {
        state.push_toast(
            ToastKind::Error,
            "these tracks cannot be removed — try refreshing the playlist",
            state.elapsed_ms,
        );
        return;
    }

    state.modal = Some(Modal::Confirm {
        text: format!("Remove {} track(s) from this playlist?", entries.len()),
        action: ConfirmAction::RemoveTracks { playlist, entries },
    });
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

    // A modal owns the keyboard, transport included. Enter on a prompt is the
    // one action the reducer cannot finish, because submitting means an API
    // call — everything else is a state transition.
    if state.modal.is_some() {
        if action == A::Confirm && matches!(state.modal, Some(Modal::Prompt { .. })) {
            let (token, task) = submit_prompt(state)?;
            return Some(Task::Mutate { token, task });
        }
        if action == A::Confirm && matches!(state.modal, Some(Modal::PickPlaylist { .. })) {
            let (token, task) = submit_pick(state)?;
            return Some(Task::Mutate { token, task });
        }
        if matches!(state.modal, Some(Modal::Confirm { .. })) {
            match action {
                // `y`/`n` are not keymap bindings: they mean nothing outside a
                // confirm, and binding them globally would shadow real keys.
                A::Char('y') | A::Char('Y') | A::Confirm => {
                    let (token, task) = confirm_action(state)?;
                    return Some(Task::Mutate { token, task });
                }
                A::Char('n') | A::Char('N') => {
                    state.modal = None;
                    return None;
                }
                _ => {}
            }
        }
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
            // An artist row opens their top tracks (FR-B7). Before this the
            // Artists pane was a dead end: names on screen, and Enter did
            // nothing at all.
            if let Some(a) = state.selected_artist() {
                return start(
                    state,
                    Task::OpenArtist {
                        id: a.id.clone(),
                        name: a.name.clone(),
                    },
                );
            }
            // A home card does whatever its kind implies: a track plays, and a
            // playlist or artist opens. One carousel holds all of them, so the
            // row decides, not the pane.
            if state.pane == Pane::Home
                && let Some(item) = state.selected_home_item()
            {
                match item.target.clone() {
                    ytm_core::HomeTarget::Playlist(id) => {
                        return start(state, Task::OpenPlaylist(id));
                    }
                    ytm_core::HomeTarget::Artist(id) => {
                        let name = item.title.clone();
                        return start(state, Task::OpenArtist { id, name });
                    }
                    // An album has no pane of its own (out of scope), so it is
                    // left alone rather than opening something unrelated.
                    ytm_core::HomeTarget::Album(_) => return None,
                    ytm_core::HomeTarget::Track(_) => {}
                }
            }
            if let Some(t) = state.selected_track() {
                send(player, PlayerCommand::PlayNow(t));
            }
        }
        // Both honour a marked selection, so `V` over a run then `a` queues the
        // whole range rather than only the row under the cursor.
        A::AddToQueue | A::PlayNext => {
            let tracks = queue_targets(state);
            if tracks.is_empty() {
                return None;
            }
            let msg = enqueue_message(&tracks, action == A::PlayNext);
            let cmd = if action == A::PlayNext {
                PlayerCommand::EnqueueNext(tracks)
            } else {
                PlayerCommand::EnqueueBack(tracks)
            };
            send(player, cmd);
            // FR-U3: without this, `a` outside the queue pane gives no sign it
            // worked — the queue is not on screen to show the new row.
            state.push_toast(ToastKind::Success, &msg, state.elapsed_ms);
            state.marked.clear();
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
        A::AddToPlaylist => open_add_to_playlist(state),
        A::RemoveFromPlaylist => open_remove_confirm(state),
        A::ToggleMark => state.toggle_mark(),
        A::CreatePlaylist => open_create_prompt(state),
        A::DeletePlaylist => open_delete_confirm(state),
        A::RenamePlaylist => {
            open_rename_prompt(state);
        }
        A::Refresh => {
            return start(state, pane_task(state.pane)?);
        }
        // The forward half of the h/l pair: `l` descends into the selected
        // playlist, `h` comes back out. Opening needs a fetch, so it cannot
        // live in the reducer with the rest of Left/Right.
        //
        // Only descends from the playlist list. On a track pane there is
        // nothing below, and playing here would make a navigation key start
        // audio — Enter is the key that plays.
        A::Right => {
            if let Some(p) = state.selected_playlist() {
                return start(state, Task::OpenPlaylist(p.id.clone()));
            }
            state.apply(AppEvent::Input(A::Right));
        }
        // A number key switches pane, so the new pane needs its rows.
        A::GoTo(n) => {
            state.apply(AppEvent::Input(A::GoTo(n)));
            if let Some(task) = pane_task(state.pane) {
                return start(state, task);
            }
        }
        // Everything else is a state transition.
        other => state.apply(AppEvent::Input(other)),
    }
    None
}

/// The fetch a pane needs to fill itself, or `None` when it has nothing to load.
///
/// Queue is local state owned by the actor, and Search waits for a query — a
/// fetch for either would be a request the user never made.
fn pane_task(pane: Pane) -> Option<Task> {
    Some(match pane {
        Pane::Home => Task::LoadHome,
        Pane::Playlists => Task::LoadPlaylists,
        Pane::Songs => Task::LoadSongs,
        Pane::Albums => Task::LoadAlbums,
        Pane::Artists => Task::LoadArtists,
        Pane::Search | Pane::Queue => return None,
    })
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
    mut keymap: KeyMap,
    mut theme: Theme,
    tick_ms: u64,
    cookie_auth: bool,
    cache: Option<ytm_core::cache::Cache>,
    mut art: ytm_tui::widgets::art::ArtCache,
    mut media: Option<souvlaki::MediaControls>,
    // `config_path` is where `,` opens an editor and what a reload re-reads;
    // `theme_name` tracks where `t` is in the preset cycle.
    mut media_keys: mpsc::UnboundedReceiver<PlayerCommand>,
    config_path: std::path::PathBuf,
    mut theme_name: String,
) -> color_eyre::Result<()> {
    use crossterm::event::{Event as CtEvent, EventStream, KeyEventKind, MouseEventKind};
    use futures::StreamExt;

    // App-internal events (results of background work).
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let mut term_events = EventStream::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(tick_ms.max(1)));
    let started = std::time::Instant::now();
    let mut debounce = SearchDebounce::default();
    // `zz` spans two key presses, so the prefix has to live across iterations.
    let mut pending = ytm_tui::keymap::Pending::default();

    // `main` has already drawn the cached frame — building the source needs a
    // network round trip, and NFR-1 will not survive doing that first. Redrawing
    // here is cheap and keeps `run` correct when called with a cold cache.
    terminal.draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap, &mut art))?;
    state.loading = true;
    spawn_task(Task::LoadPlaylists, source.clone(), app_tx.clone());

    loop {
        tokio::select! {
            // Terminal input
            Some(Ok(ev)) = term_events.next() => {
                match ev {
                    // The wheel scrolls the focused list (FR-U8). Clicks are not
                    // bound on purpose — capturing them would cost the user the
                    // terminal's own text selection.
                    CtEvent::Mouse(m) => {
                        let scroll = match m.kind {
                            MouseEventKind::ScrollDown => Some(InputAction::ScrollDown),
                            MouseEventKind::ScrollUp => Some(InputAction::ScrollUp),
                            _ => None,
                        };
                        if let Some(a) = scroll {
                            state.apply(AppEvent::Input(a));
                        }
                    }
                    CtEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        // `input_focus`, not `focus`: an open prompt is a text field, so
                        // letters must resolve to Char(c) rather than commands.
                        let (action, next) =
                            keymap.resolve_chord(k, state.input_focus(), pending);
                        pending = next;
                        if let Some(a) = action {
                            // Both of these own resources `dispatch_input` cannot
                            // reach: the live theme, and the terminal itself.
                            match a {
                                InputAction::CycleTheme if state.modal.is_none() => {
                                    theme_name = ytm_tui::theme::Theme::next_preset(&theme_name).to_owned();
                                    theme = ytm_tui::theme::Theme::preset(&theme_name)
                                        .unwrap_or_default();
                                    state.push_toast(
                                        ToastKind::Info,
                                        &format!("theme: {theme_name}"),
                                        state.elapsed_ms,
                                    );
                                }
                                InputAction::EditConfig if state.modal.is_none() => {
                                    match edit_config_in_editor(terminal, &config_path) {
                                        Ok(Some(reloaded)) => {
                                            keymap = reloaded.keymap;
                                            theme = reloaded.theme;
                                            theme_name = reloaded.theme_name;
                                            state.push_toast(
                                                ToastKind::Success,
                                                "config reloaded",
                                                state.elapsed_ms,
                                            );
                                        }
                                        // Unchanged file, or no editor to open.
                                        Ok(None) => {}
                                        Err(e) => state.push_toast(
                                            ToastKind::Error,
                                            &format!("config not reloaded: {e}"),
                                            state.elapsed_ms,
                                        ),
                                    }
                                    // The editor painted over the frame.
                                    terminal.clear()?;
                                }
                                _ => {
                                    if let Some(task) = dispatch_input(a, &mut state, &source, &player) {
                                        spawn_task(task, source.clone(), app_tx.clone());
                                    }
                                }
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
            Some(pe) = player_events.recv() => {
                // Metadata on a track change, status on a state change. Not on
                // every Progress event: that is 4Hz of D-Bus traffic for a
                // position the desktop widget interpolates itself.
                let notify = matches!(
                    pe,
                    ytm_player::player::PlayerEvent::TrackChanged(_)
                        | ytm_player::player::PlayerEvent::StateChanged(_)
                );
                state.apply(AppEvent::Player(pe));
                if notify && let Some(c) = media.as_mut() {
                    mpris::update(c, &state);
                }
            }

            // OS media keys. The handler runs on souvlaki's thread and can only
            // send, so the command is forwarded from here where the player lives.
            Some(cmd) = media_keys.recv() => {
                tracing::debug!(?cmd, "media key");
                send(&player, cmd);
            }

            // Background work results
            Some(ae) = app_rx.recv() => {
                if let Some(hint) = empty_library_hint(cookie_auth, &ae) {
                    state.push_toast(ToastKind::Info, &hint, state.elapsed_ms);
                }
                if let Some(c) = cache.as_ref() {
                    cache_write_through(c, &ae);
                }
                // Art lives in the cache, not in state: protocol objects are
                // not comparable or cloneable, so a pure reducer cannot hold them.
                if let AppEvent::ArtFailed { url } = &ae {
                    art.mark_failed(url);
                }
                if let AppEvent::ArtLoaded { url, image } = ae {
                    art.insert(&url, *image);
                    continue;
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
                if art.is_enabled()
                    && let Some(url) = art_tick(&mut art, &state)
                {
                    spawn_art_fetch(url, app_tx.clone());
                }
            }
        }

        // Set before drawing: the reducer needs the row count for paging and
        // `zz`, and only the frame knows it.
        state.viewport_rows = ytm_tui::render::list_rows_for(
            terminal.size()?.into(),
            state.pane,
            !state.search_query.is_empty() || state.focus == ytm_tui::app::Focus::SearchInput,
        );
        terminal.draw(|f| ytm_tui::render::render(f, &state, &theme, &keymap, &mut art))?;

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
            Task::LoadHome => match source.home_shelves().await {
                Ok(v) => AppEvent::HomeLoaded(v),
                Err(e) => AppEvent::Error(e.to_string()),
            },
            Task::OpenArtist { id, name } => match source.artist_tracks(id.clone()).await {
                Ok(tracks) => AppEvent::ArtistTracksLoaded { id, name, tracks },
                Err(e) => AppEvent::Error(e.to_string()),
            },
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
            // Failures come back as MutationFailed, not Error: the token has to
            // survive so `rollback` reverts the right edit.
            Task::Mutate { token, task } => run_mutation(token, task, source.clone()).await,
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

/// Fill state from the cache before the first frame (NFR-1).
///
/// A cache read is a local SQLite query, so it is cheap enough to do before the
/// draw; the network refresh follows and overwrites. Failures are logged and
/// ignored — the cache is disposable, and a bad one must never block startup.
pub fn preload_from_cache(cache: &ytm_core::cache::Cache, state: &mut AppState) {
    match cache.load_playlists() {
        Ok(v) if !v.is_empty() => state.playlists = v,
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "could not read cached playlists"),
    }
    match cache.load_library_songs() {
        Ok(v) if !v.is_empty() => state.tracks = v,
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "could not read cached songs"),
    }
}

/// Persist a fresh library response so the next cold start has content.
///
/// Two deliberate exclusions:
/// - **An empty library list is not written.** Under cookie auth an expired
///   cookie answers HTTP 200 with zero rows, and writing that through would turn
///   a one-off auth lapse into a wiped cache — the next launch would come up
///   blank. An account that really is empty just keeps a stale cache, which is
///   the cheaper mistake.
/// - **Search results are not cached.** They belong to a query, not to the
///   library, and the schema has no place to key them.
///
/// Albums and artists have no tables (the schema caches playlists and tracks),
/// so they pass through untouched.
fn cache_write_through(cache: &ytm_core::cache::Cache, ev: &AppEvent) {
    let result = match ev {
        AppEvent::PlaylistsLoaded(v) if !v.is_empty() => cache.save_playlists(v),
        AppEvent::LibrarySongsLoaded(v) if !v.is_empty() => cache.save_library_songs(v),
        // A playlist genuinely can be empty, and its rows are keyed by id, so
        // there is no wipe-the-library risk in writing that through.
        AppEvent::PlaylistTracksLoaded { id, tracks } => cache.save_playlist_tracks(id, tracks),
        _ => return,
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, event = event_name(ev), "could not write to the cache");
    }
}

/// The URL to fetch art for, if any, exactly once per URL.
///
/// Called from the tick arm rather than on `TrackChanged`, so a track whose art
/// failed to decode, or that started playing before the picker finished probing,
/// still gets one attempt. `should_fetch` is what makes "every tick" cheap.
fn art_tick(art: &mut ytm_tui::widgets::art::ArtCache, state: &AppState) -> Option<String> {
    let url = state.now_playing.as_ref()?.thumbnail_url.as_deref()?;
    art.should_fetch(url).then(|| url.to_owned())
}

/// Fetch and decode one thumbnail off the UI thread (NFR-2).
///
/// Decoding is CPU work, so it goes to `spawn_blocking` rather than holding a
/// runtime worker. Every failure path posts `ArtFailed`, which is what stops the
/// URL being retried on every tick; a silent drop would retry forever.
fn spawn_art_fetch(url: String, tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let ev = match fetch_art(&url).await {
            Ok(image) => AppEvent::ArtLoaded {
                url: url.clone(),
                image: Box::new(image),
            },
            Err(e) => {
                // Debug level, not warn: a missing thumbnail is normal and
                // FR-U5 says the UI is complete without art.
                tracing::debug!(url = %url, error = %e, "album art unavailable");
                AppEvent::ArtFailed { url: url.clone() }
            }
        };
        let _ = tx.send(ev);
    });
}

async fn fetch_art(
    url: &str,
) -> Result<image::DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
    let bytes = reqwest::get(url).await?.error_for_status()?.bytes().await?;
    // Decode is CPU-bound; keep it off the async workers.
    let image = tokio::task::spawn_blocking(move || {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()?
            .decode()
    })
    .await??;
    Ok(image)
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

    #[tokio::test]
    async fn creating_a_playlist_calls_the_source_and_commits() {
        let src = Arc::new(MockSource::new());
        let mut s = AppState {
            modal: Some(ytm_tui::app::Modal::Prompt {
                title: "Name".into(),
                value: "Road Trip".into(),
                action: ytm_tui::app::PromptAction::CreatePlaylist,
            }),
            ..Default::default()
        };

        let (token, task) = submit_prompt(&mut s).expect("a prompt submission yields a mutation");
        assert_eq!(
            s.playlists.len(),
            1,
            "FR-C6: optimistic row appears at once"
        );
        assert!(s.modal.is_none(), "the modal closes on submit");

        let ev = run_mutation(token, task, src.clone()).await;
        assert!(
            src.calls().iter().any(|c| c.starts_with("create_playlist")),
            "got {:?}",
            src.calls()
        );
        match ev {
            AppEvent::MutationOk {
                token: t, real_id, ..
            } => {
                assert_eq!(t, token);
                assert!(real_id.is_some(), "commit needs the real id to swap in");
            }
            other => panic!("expected MutationOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failed_create_yields_mutation_failed_with_the_same_token() {
        let src = Arc::new(MockSource::new());
        src.fail_next(ytm_core::SourceError::RateLimited);
        let ev = run_mutation(
            7,
            MutationTask::Create {
                title: "X".into(),
                description: None,
                privacy: ytm_core::Privacy::Private,
            },
            src,
        )
        .await;
        match ev {
            AppEvent::MutationFailed { token, message } => {
                assert_eq!(token, 7);
                assert!(message.contains("too many requests"), "got: {message}");
            }
            other => panic!("expected MutationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn renaming_sends_the_new_title() {
        let src =
            Arc::new(MockSource::new().with_playlists(vec![ytm_core::Playlist::stub("p1", "Old")]));
        let _ = run_mutation(
            1,
            MutationTask::Rename {
                id: "p1".into(),
                title: "New".into(),
            },
            src.clone(),
        )
        .await;
        assert!(src.calls().iter().any(|c| c.starts_with("edit_playlist")));
        assert_eq!(src.library_playlists().await.unwrap()[0].title, "New");
    }

    #[test]
    fn renaming_a_system_playlist_is_refused_before_any_api_call() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist {
                is_system: true,
                ..ytm_core::Playlist::stub("LM", "Your Likes")
            }],
            selected: 0,
            ..Default::default()
        };
        assert!(open_rename_prompt(&mut s).is_none(), "must refuse");
        assert_eq!(s.toasts.len(), 1, "and say why");
        assert!(
            s.modal.is_none(),
            "no prompt for a playlist that cannot change"
        );
    }

    #[test]
    fn renaming_an_editable_playlist_prefills_its_current_title() {
        // An empty field would make rename feel like create.
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            selected: 0,
            ..Default::default()
        };
        assert!(open_rename_prompt(&mut s).is_some());
        match &s.modal {
            Some(ytm_tui::app::Modal::Prompt { value, .. }) => assert_eq!(value, "Focus"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn submitting_an_empty_name_is_refused() {
        let mut s = AppState {
            modal: Some(ytm_tui::app::Modal::Prompt {
                title: "Name".into(),
                value: "   ".into(),
                action: ytm_tui::app::PromptAction::CreatePlaylist,
            }),
            ..Default::default()
        };
        assert!(submit_prompt(&mut s).is_none());
        assert!(
            s.playlists.is_empty(),
            "no optimistic row for an invalid name"
        );
        assert_eq!(s.toasts.len(), 1, "and say why");
    }

    #[test]
    fn a_rejected_rename_puts_the_old_title_back() {
        // The whole point of the mutation log: the row reverts, not the list.
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            selected: 0,
            modal: Some(ytm_tui::app::Modal::Prompt {
                title: "Rename".into(),
                value: "Deep Focus".into(),
                action: ytm_tui::app::PromptAction::RenamePlaylist("p1".into()),
            }),
            ..Default::default()
        };
        let (token, _) = submit_prompt(&mut s).expect("rename must start");
        assert_eq!(s.playlists[0].title, "Deep Focus");
        s.apply(AppEvent::MutationFailed {
            token,
            message: "rejected".into(),
        });
        assert_eq!(s.playlists[0].title, "Focus");
    }

    #[test]
    fn n_opens_a_create_prompt_and_r_a_rename_prompt() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            selected: 0,
            ..Default::default()
        };
        dispatch_input(InputAction::CreatePlaylist, &mut s, &src, &*player);
        assert!(matches!(s.modal, Some(ytm_tui::app::Modal::Prompt { .. })));
        s.modal = None;
        dispatch_input(InputAction::RenamePlaylist, &mut s, &src, &*player);
        assert!(matches!(s.modal, Some(ytm_tui::app::Modal::Prompt { .. })));
        assert!(player.commands().is_empty(), "neither touches the player");
    }

    #[test]
    fn pressing_delete_opens_a_confirmation_rather_than_deleting() {
        // FR-C3: destructive actions are never one keystroke.
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            selected: 0,
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        assert!(matches!(s.modal, Some(Modal::Confirm { .. })));
        assert_eq!(s.playlists.len(), 1, "nothing is removed until confirmed");
    }

    #[test]
    fn the_confirmation_names_the_playlist() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        match &s.modal {
            Some(Modal::Confirm { text, .. }) => {
                assert!(text.contains("Focus"), "got: {text}")
            }
            other => panic!("expected a confirm, got {other:?}"),
        }
    }

    #[test]
    fn confirming_removes_the_row_and_returns_the_task() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        let (_token, task) = confirm_action(&mut s).expect("confirm yields work");
        assert!(matches!(task, MutationTask::Delete { .. }));
        assert!(s.playlists.is_empty(), "optimistic removal");
        assert!(s.modal.is_none());
    }

    #[test]
    fn a_failed_delete_puts_the_playlist_back() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![
                ytm_core::Playlist::stub("p1", "A"),
                ytm_core::Playlist::stub("p2", "B"),
            ],
            selected: 1,
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        let (token, _) = confirm_action(&mut s).unwrap();
        assert_eq!(s.playlists.len(), 1);
        s.rollback(token);
        assert_eq!(s.playlists.len(), 2);
        assert_eq!(s.playlists[1].title, "B", "restored at its original index");
    }

    #[test]
    fn a_system_playlist_cannot_be_deleted() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist {
                is_system: true,
                ..ytm_core::Playlist::stub("LM", "Your Likes")
            }],
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        assert!(
            s.modal.is_none(),
            "no confirmation for an impossible action"
        );
        assert_eq!(s.toasts.len(), 1, "explain why instead");
    }

    #[test]
    fn declining_the_confirmation_changes_nothing() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        open_delete_confirm(&mut s);
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert!(s.modal.is_none());
        assert_eq!(s.playlists.len(), 1);
        assert!(s.pending.is_empty());
    }

    #[test]
    fn y_confirms_a_delete_and_n_declines_it() {
        // Nothing else proves a user can actually answer the box: `y` and `n`
        // are not in the keymap, so the loop has to resolve them.
        let (src, player) = deps();
        let mut yes = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        open_delete_confirm(&mut yes);
        let task = dispatch_input(InputAction::Char('y'), &mut yes, &src, &*player);
        assert!(
            matches!(
                task,
                Some(Task::Mutate {
                    task: MutationTask::Delete { .. },
                    ..
                })
            ),
            "got {task:?}"
        );
        assert!(yes.playlists.is_empty());

        let mut no = AppState {
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        open_delete_confirm(&mut no);
        assert!(dispatch_input(InputAction::Char('n'), &mut no, &src, &*player).is_none());
        assert!(no.modal.is_none(), "n closes the box");
        assert_eq!(no.playlists.len(), 1, "and deletes nothing");
    }

    #[test]
    fn d_on_a_playlist_opens_the_delete_confirmation() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        dispatch_input(InputAction::DeletePlaylist, &mut s, &src, &*player);
        assert!(matches!(s.modal, Some(Modal::Confirm { .. })));
    }

    fn playlist_track(v: &str, sv: &str, title: &str) -> ytm_core::Track {
        ytm_core::Track {
            set_video_id: Some(ytm_core::SetVideoId::from(sv)),
            ..ytm_core::Track::stub(v, title)
        }
    }

    fn open_playlist_with(tracks: Vec<ytm_core::Track>) -> AppState {
        AppState {
            pane: Pane::Playlists,
            open_playlist: Some("p1".into()),
            tracks,
            ..Default::default()
        }
    }

    #[test]
    fn with_no_marks_the_target_is_the_selected_track() {
        let s = AppState {
            pane: Pane::Songs,
            tracks: vec![
                ytm_core::Track::stub("v1", "A"),
                ytm_core::Track::stub("v2", "B"),
            ],
            selected: 1,
            ..Default::default()
        };
        assert_eq!(targets_for_add(&s), vec![ytm_core::VideoId::from("v2")]);
    }

    #[test]
    fn marked_tracks_take_precedence_over_the_selection() {
        // FR-C4: multi-select.
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![
                ytm_core::Track::stub("v1", "A"),
                ytm_core::Track::stub("v2", "B"),
                ytm_core::Track::stub("v3", "C"),
            ],
            selected: 0,
            ..Default::default()
        };
        s.marked.insert(ytm_core::VideoId::from("v2"));
        s.marked.insert(ytm_core::VideoId::from("v3"));
        let mut got = targets_for_add(&s);
        got.sort();
        assert_eq!(
            got,
            vec![ytm_core::VideoId::from("v2"), ytm_core::VideoId::from("v3")]
        );
    }

    #[test]
    fn an_empty_list_yields_no_targets() {
        let s = AppState::default();
        assert!(targets_for_add(&s).is_empty());
    }

    #[test]
    fn toggle_mark_adds_then_removes() {
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            ..Default::default()
        };
        s.toggle_mark();
        assert!(s.marked.contains(&ytm_core::VideoId::from("v1")));
        s.toggle_mark();
        assert!(s.marked.is_empty());
    }

    #[test]
    fn removing_requires_an_open_playlist() {
        let mut s = AppState {
            pane: Pane::Songs, // library songs, not a playlist
            tracks: vec![playlist_track("v1", "sv1", "A")],
            ..Default::default()
        };
        open_remove_confirm(&mut s);
        assert!(s.modal.is_none(), "there is no playlist to remove from");
        assert_eq!(s.toasts.len(), 1);
    }

    #[test]
    fn removing_a_track_without_a_set_video_id_is_refused() {
        // Without SetVideoId the API cannot identify the entry.
        let mut s = open_playlist_with(vec![ytm_core::Track::stub("v1", "A")]);
        open_remove_confirm(&mut s);
        assert!(s.modal.is_none());
        assert_eq!(s.toasts.len(), 1, "explain rather than fail silently");
    }

    #[test]
    fn removing_opens_a_confirmation_naming_the_count() {
        let mut s = open_playlist_with(vec![
            playlist_track("v1", "sv1", "A"),
            playlist_track("v2", "sv2", "B"),
        ]);
        s.marked.insert(ytm_core::VideoId::from("v1"));
        s.marked.insert(ytm_core::VideoId::from("v2"));
        open_remove_confirm(&mut s);
        match &s.modal {
            Some(Modal::Confirm { text, .. }) => {
                assert!(text.contains('2'), "got: {text}")
            }
            other => panic!("expected a confirm, got {other:?}"),
        }
    }

    #[test]
    fn confirming_a_removal_drops_the_rows_and_can_be_undone() {
        let mut s = open_playlist_with(vec![
            playlist_track("v1", "sv1", "A"),
            playlist_track("v2", "sv2", "B"),
            playlist_track("v3", "sv3", "C"),
        ]);
        s.selected = 1;
        open_remove_confirm(&mut s);
        let (token, task) = confirm_action(&mut s).expect("confirm yields work");
        assert!(matches!(task, MutationTask::RemoveTracks { .. }));
        assert_eq!(s.tracks.len(), 2);
        s.rollback(token);
        let titles: Vec<_> = s.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["A", "B", "C"], "restored in order");
    }

    #[test]
    fn adding_with_no_editable_playlist_says_so_instead_of_opening_an_empty_picker() {
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            playlists: vec![ytm_core::Playlist {
                is_system: true,
                ..ytm_core::Playlist::stub("LM", "Your Likes")
            }],
            ..Default::default()
        };
        open_add_to_playlist(&mut s);
        assert!(s.modal.is_none());
        assert_eq!(s.toasts.len(), 1);
    }

    #[test]
    fn the_picker_lists_only_editable_playlists() {
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            playlists: vec![
                ytm_core::Playlist {
                    is_system: true,
                    ..ytm_core::Playlist::stub("LM", "Your Likes")
                },
                ytm_core::Playlist::stub("p1", "Focus"),
            ],
            ..Default::default()
        };
        open_add_to_playlist(&mut s);
        match &s.modal {
            Some(Modal::PickPlaylist {
                choices, targets, ..
            }) => {
                assert_eq!(choices.len(), 1, "a system playlist cannot be added to");
                assert_eq!(choices[0].1, "Focus");
                assert_eq!(targets.len(), 1);
            }
            other => panic!("expected a picker, got {other:?}"),
        }
    }

    #[test]
    fn picking_a_playlist_spawns_the_add_and_clears_the_marks() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![
                ytm_core::Track::stub("v1", "A"),
                ytm_core::Track::stub("v2", "B"),
            ],
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        s.marked.insert(ytm_core::VideoId::from("v1"));
        s.marked.insert(ytm_core::VideoId::from("v2"));
        open_add_to_playlist(&mut s);
        let task = dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        match task {
            Some(Task::Mutate {
                task: MutationTask::AddTracks { id, videos },
                ..
            }) => {
                assert_eq!(id, ytm_core::PlaylistId::from("p1"));
                assert_eq!(videos.len(), 2);
            }
            other => panic!("expected an AddTracks mutation, got {other:?}"),
        }
        assert!(s.modal.is_none());
        assert!(s.marked.is_empty(), "marks are consumed by the action");
    }

    #[test]
    fn the_picker_moves_through_its_choices() {
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "A")],
            playlists: vec![
                ytm_core::Playlist::stub("p1", "One"),
                ytm_core::Playlist::stub("p2", "Two"),
            ],
            ..Default::default()
        };
        open_add_to_playlist(&mut s);
        s.apply(AppEvent::Input(InputAction::Down));
        match &s.modal {
            Some(Modal::PickPlaylist { selected, .. }) => assert_eq!(*selected, 1),
            other => panic!("expected a picker, got {other:?}"),
        }
        // And it must not run off the end.
        s.apply(AppEvent::Input(InputAction::Down));
        match &s.modal {
            Some(Modal::PickPlaylist { selected, .. }) => assert_eq!(*selected, 1),
            other => panic!("expected a picker, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn adding_tracks_calls_the_source_with_every_video_id() {
        let src = Arc::new(
            MockSource::new().with_playlists(vec![ytm_core::Playlist::stub("p1", "Target")]),
        );
        let _ = run_mutation(
            1,
            MutationTask::AddTracks {
                id: "p1".into(),
                videos: vec![ytm_core::VideoId::from("v1"), ytm_core::VideoId::from("v2")],
            },
            src.clone(),
        )
        .await;
        assert!(
            src.calls().iter().any(|c| c == "add_tracks(p1,2)"),
            "got {:?}",
            src.calls()
        );
    }

    #[tokio::test]
    async fn removing_tracks_sends_every_set_video_id() {
        let src = Arc::new(MockSource::new());
        let _ = run_mutation(
            1,
            MutationTask::RemoveTracks {
                id: "p1".into(),
                entries: vec![
                    ytm_core::SetVideoId::from("sv1"),
                    ytm_core::SetVideoId::from("sv2"),
                ],
            },
            src.clone(),
        )
        .await;
        assert!(
            src.calls().iter().any(|c| c == "remove_tracks(p1,2)"),
            "got {:?}",
            src.calls()
        );
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

    #[test]
    fn a_warm_cache_fills_state_before_any_network_call() {
        // NFR-1: the first frame must have content without awaiting the network.
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        cache
            .save_playlists(&[ytm_core::Playlist::stub("p1", "Focus")])
            .unwrap();
        cache
            .save_library_songs(&[ytm_core::Track::stub("v1", "Song")])
            .unwrap();
        let mut s = AppState::default();
        preload_from_cache(&cache, &mut s);
        assert_eq!(s.playlists.len(), 1);
        assert_eq!(s.playlists[0].title, "Focus");
        assert_eq!(s.tracks.len(), 1);
    }

    #[test]
    fn a_cold_cache_leaves_state_untouched_and_does_not_fail() {
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        let mut s = AppState::default();
        preload_from_cache(&cache, &mut s);
        assert!(s.playlists.is_empty());
        assert!(s.tracks.is_empty());
    }

    #[test]
    fn a_playlists_event_is_written_through_to_the_cache() {
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        cache_write_through(
            &cache,
            &AppEvent::PlaylistsLoaded(vec![ytm_core::Playlist::stub("p1", "Focus")]),
        );
        assert_eq!(cache.load_playlists().unwrap()[0].title, "Focus");
    }

    #[test]
    fn playlist_tracks_are_written_through_under_their_playlist_id() {
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        let id = ytm_core::PlaylistId::from("p1");
        cache_write_through(
            &cache,
            &AppEvent::PlaylistTracksLoaded {
                id: id.clone(),
                tracks: vec![ytm_core::Track::stub("v1", "First")],
            },
        );
        assert_eq!(cache.load_playlist_tracks(&id).unwrap()[0].title, "First");
        assert!(
            cache.load_library_songs().unwrap().is_empty(),
            "playlist rows must not leak into the library songs"
        );
    }

    #[test]
    fn an_empty_library_response_does_not_wipe_a_good_cache() {
        // An expired cookie answers HTTP 200 with zero rows. Writing that
        // through would turn a one-off auth lapse into a lost cache, so the
        // next cold start would have nothing to show.
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        cache
            .save_playlists(&[ytm_core::Playlist::stub("p1", "Focus")])
            .unwrap();
        cache_write_through(&cache, &AppEvent::PlaylistsLoaded(vec![]));
        assert_eq!(cache.load_playlists().unwrap().len(), 1);
    }

    #[test]
    fn search_results_are_not_cached() {
        // Search is not library state; caching it would show stale matches for
        // a query the user has not typed yet.
        let cache = ytm_core::cache::Cache::open_in_memory().unwrap();
        cache_write_through(
            &cache,
            &AppEvent::SearchResults {
                query: "q".into(),
                tracks: vec![ytm_core::Track::stub("v1", "Hit")],
            },
        );
        assert!(cache.load_library_songs().unwrap().is_empty());
    }

    #[test]
    fn art_is_fetched_once_for_the_playing_track() {
        let mut art = ytm_tui::widgets::art::ArtCache::disabled();
        let s = AppState {
            now_playing: Some(ytm_core::Track {
                thumbnail_url: Some("https://example.com/a.jpg".into()),
                ..ytm_core::Track::stub("v1", "T")
            }),
            ..Default::default()
        };
        assert_eq!(
            art_tick(&mut art, &s).as_deref(),
            Some("https://example.com/a.jpg")
        );
        assert!(
            art_tick(&mut art, &s).is_none(),
            "a second tick must not refetch the same URL"
        );
    }

    #[test]
    fn no_art_is_fetched_when_nothing_is_playing() {
        let mut art = ytm_tui::widgets::art::ArtCache::disabled();
        assert!(art_tick(&mut art, &AppState::default()).is_none());
    }

    #[test]
    fn a_track_without_a_thumbnail_fetches_nothing() {
        let mut art = ytm_tui::widgets::art::ArtCache::disabled();
        let s = AppState {
            now_playing: Some(ytm_core::Track::stub("v1", "T")),
            ..Default::default()
        };
        assert!(art_tick(&mut art, &s).is_none());
    }

    #[test]
    fn a_failed_art_url_is_not_refetched() {
        let mut art = ytm_tui::widgets::art::ArtCache::disabled();
        let s = AppState {
            now_playing: Some(ytm_core::Track {
                thumbnail_url: Some("https://example.com/dead.jpg".into()),
                ..ytm_core::Track::stub("v1", "T")
            }),
            ..Default::default()
        };
        assert!(art_tick(&mut art, &s).is_some());
        art.mark_failed("https://example.com/dead.jpg");
        assert!(
            art_tick(&mut art, &s).is_none(),
            "a dead thumbnail must not be retried every tick"
        );
    }

    #[test]
    fn right_on_a_playlist_opens_it_like_enter() {
        // The forward half of the h/l pair: `l` descends into a playlist, `h`
        // comes back out. Opening is a network task, so it lives here rather
        // than in the reducer.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            focus: Focus::Main,
            ..Default::default()
        };
        let task = dispatch_input(InputAction::Right, &mut s, &src, &*player);
        assert_eq!(task, Some(Task::OpenPlaylist("p1".into())));
    }

    #[test]
    fn right_inside_an_open_playlist_does_not_reopen_it() {
        // Nothing to descend into, so `l` must not fire a redundant fetch.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            open_playlist: Some("p1".into()),
            tracks: vec![ytm_core::Track::stub("v1", "T")],
            focus: Focus::Main,
            ..Default::default()
        };
        assert!(dispatch_input(InputAction::Right, &mut s, &src, &*player).is_none());
    }

    #[test]
    fn right_on_a_track_pane_does_not_play_anything() {
        // `l` is navigation, not Enter. Playing on a focus change would be a
        // nasty surprise.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "T")],
            focus: Focus::Main,
            ..Default::default()
        };
        let task = dispatch_input(InputAction::Right, &mut s, &src, &*player);
        assert!(task.is_none());
        assert!(
            player.commands().is_empty(),
            "nothing should reach the player"
        );
    }

    #[test]
    fn a_number_key_switching_to_a_pane_loads_it() {
        // Pressing 4 for Albums must fetch albums, or the pane sits empty.
        // Home is source 1, so the sources after it all shifted by one.
        let (src, player) = deps();
        let mut s = AppState::default();
        let task = dispatch_input(InputAction::GoTo(4), &mut s, &src, &*player);
        assert_eq!(task, Some(Task::LoadAlbums));
        assert_eq!(s.pane, Pane::Albums);
    }

    #[test]
    fn a_number_key_for_the_queue_needs_no_fetch() {
        // The queue is local state owned by the actor; there is nothing to load.
        let (src, player) = deps();
        let mut s = AppState::default();
        assert!(dispatch_input(InputAction::GoTo(7), &mut s, &src, &*player).is_none());
        assert_eq!(s.pane, Pane::Queue);
    }

    #[test]
    fn marked_targets_come_out_in_row_order() {
        // `marked` is a HashSet, so iterating it scrambles the order. The user
        // selected a run of rows and expects them added in that order.
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: (0..12)
                .map(|i| ytm_core::Track::stub(&format!("v{i:02}"), "T"))
                .collect(),
            ..Default::default()
        };
        for t in s.tracks.clone() {
            s.marked.insert(t.video_id.clone());
        }
        let got: Vec<String> = targets_for_add(&s).iter().map(|v| v.0.clone()).collect();
        let want: Vec<String> = s.tracks.iter().map(|t| t.video_id.0.clone()).collect();
        assert_eq!(got, want, "targets must follow the on-screen order");
    }

    #[test]
    fn a_visual_range_reaches_add_to_playlist_through_the_keymap() {
        // End to end for `V`: the keymap must produce the action, the range must
        // mark, and the picker must carry every row. Feeding actions in directly
        // would prove the reducer works while nothing could reach it.
        let (src, player) = deps();
        let km = ytm_tui::keymap::KeyMap::default();
        let mut s = AppState {
            pane: Pane::Songs,
            focus: ytm_tui::app::Focus::Main,
            tracks: (0..5)
                .map(|i| ytm_core::Track::stub(&format!("v{i}"), "T"))
                .collect(),
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        let press = |s: &mut AppState, c: char| {
            use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
            let a = km
                .resolve(
                    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                    s.input_focus(),
                )
                .unwrap_or_else(|| panic!("{c:?} is unbound"));
            dispatch_input(a, s, &src, &*player)
        };
        press(&mut s, 'V');
        press(&mut s, 'j');
        press(&mut s, 'j');
        assert_eq!(s.marked.len(), 3, "V then j j must mark three rows");
        press(&mut s, 'A');
        match dispatch_input(InputAction::Confirm, &mut s, &src, &*player) {
            Some(Task::Mutate {
                task: MutationTask::AddTracks { videos, .. },
                ..
            }) => {
                let ids: Vec<String> = videos.iter().map(|v| v.0.clone()).collect();
                assert_eq!(ids, vec!["v0", "v1", "v2"], "in order, all three");
            }
            other => panic!("expected AddTracks, got {other:?}"),
        }
    }

    #[test]
    fn hand_marks_and_a_visual_range_add_up_through_the_keymap() {
        // The owner's question: does marking rows by hand still work alongside
        // the new range selection? The union must reach the API.
        let (src, player) = deps();
        let km = ytm_tui::keymap::KeyMap::default();
        let mut s = AppState {
            pane: Pane::Songs,
            focus: ytm_tui::app::Focus::Main,
            tracks: (0..6)
                .map(|i| ytm_core::Track::stub(&format!("v{i}"), "T"))
                .collect(),
            playlists: vec![ytm_core::Playlist::stub("p1", "Focus")],
            ..Default::default()
        };
        let press = |s: &mut AppState, c: char| {
            use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
            let a = km
                .resolve(
                    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                    s.input_focus(),
                )
                .unwrap();
            dispatch_input(a, s, &src, &*player)
        };
        press(&mut s, 'v'); // hand-mark v0
        press(&mut s, 'j');
        press(&mut s, 'j');
        press(&mut s, 'V'); // range from v2
        press(&mut s, 'j'); // ..v3
        press(&mut s, 'A');
        match dispatch_input(InputAction::Confirm, &mut s, &src, &*player) {
            Some(Task::Mutate {
                task: MutationTask::AddTracks { videos, .. },
                ..
            }) => {
                let ids: Vec<String> = videos.iter().map(|v| v.0.clone()).collect();
                assert_eq!(ids, vec!["v0", "v2", "v3"]);
            }
            other => panic!("expected AddTracks, got {other:?}"),
        }
    }

    #[test]
    fn a_visual_range_removes_every_selected_track_from_a_playlist() {
        // The other bulk action `V` feeds: the confirm must name all of them.
        let mut s = open_playlist_with(vec![
            playlist_track("v1", "sv1", "A"),
            playlist_track("v2", "sv2", "B"),
            playlist_track("v3", "sv3", "C"),
            playlist_track("v4", "sv4", "D"),
        ]);
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        s.apply(AppEvent::Input(InputAction::Down));
        open_remove_confirm(&mut s);
        match &s.modal {
            Some(Modal::Confirm {
                action: ConfirmAction::RemoveTracks { entries, .. },
                ..
            }) => assert_eq!(entries.len(), 3, "all three rows of the range"),
            other => panic!("expected a remove confirm, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_an_album_row_does_not_play_an_invisible_song() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            focus: ytm_tui::app::Focus::Main,
            tracks: vec![ytm_core::Track::stub("v0", "Left over from Songs")],
            ..Default::default()
        };
        // 4 = Albums (Home is source 1). The songs stay in `tracks`; the pane
        // now shows albums.
        s.apply(AppEvent::Input(InputAction::GoTo(4)));
        assert_eq!(s.pane, Pane::Albums);
        dispatch_input(InputAction::Confirm, &mut s, &src, &*player);
        let log = player.commands();
        assert!(
            log.is_empty(),
            "Enter on an album row started audio: {log:?}"
        );
    }

    #[test]
    fn add_to_queue_on_an_album_row_queues_nothing() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            focus: ytm_tui::app::Focus::Main,
            tracks: vec![ytm_core::Track::stub("v0", "Left over from Songs")],
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::GoTo(4)));
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        let log = player.commands();
        assert!(log.is_empty(), "`a` on an album row queued a song: {log:?}");
    }

    #[test]
    fn adding_to_the_queue_confirms_with_a_toast() {
        // FR-U3: `a` is otherwise silent unless the queue pane happens to be
        // open, so the user cannot tell it worked.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        assert_eq!(s.toasts.len(), 1, "adding must be acknowledged");
        assert_eq!(s.toasts[0].kind, ToastKind::Success);
        assert!(
            s.toasts[0].text.contains("Roygbiv"),
            "name what was added, got: {}",
            s.toasts[0].text
        );
    }

    #[test]
    fn play_next_confirms_with_its_own_wording() {
        // `e` and `a` do different things, so one shared message would mislead.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        dispatch_input(InputAction::PlayNext, &mut s, &src, &*player);
        assert_eq!(s.toasts.len(), 1);
        assert!(
            s.toasts[0].text.to_lowercase().contains("next"),
            "got: {}",
            s.toasts[0].text
        );
    }

    #[test]
    fn adding_a_whole_marked_selection_reports_the_count() {
        // A range of 12 tracks naming only the first would read as a bug.
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: (0..3)
                .map(|i| ytm_core::Track::stub(&format!("v{i}"), "T"))
                .collect(),
            ..Default::default()
        };
        for t in s.tracks.clone() {
            s.marked.insert(t.video_id.clone());
        }
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        assert!(s.toasts[0].text.contains('3'), "got: {}", s.toasts[0].text);
        match player.commands().first() {
            Some(PlayerCommand::EnqueueBack(ts)) => {
                assert_eq!(ts.len(), 3, "all marked tracks must be queued")
            }
            other => panic!("expected EnqueueBack, got {other:?}"),
        }
    }

    #[test]
    fn adding_with_an_empty_list_says_nothing_rather_than_lying() {
        let (src, player) = deps();
        let mut s = AppState {
            pane: Pane::Songs,
            ..Default::default()
        };
        dispatch_input(InputAction::AddToQueue, &mut s, &src, &*player);
        assert!(s.toasts.is_empty(), "nothing was added, so say nothing");
        assert!(player.commands().is_empty());
    }

    #[test]
    fn the_bundled_example_config_parses() {
        // `,` writes this file when the user has no config yet, so an invalid
        // example would hand them a config that refuses to load.
        let c = crate::config::Config::from_toml_str(crate::config::EXAMPLE_TOML)
            .expect("config.example.toml must parse");
        // Every value in it is commented out or a real default, so it must be
        // indistinguishable from no config at all.
        let d = crate::config::Config::default();
        assert_eq!(c.playback.volume, d.playback.volume);
        assert_eq!(c.behaviour.seek_step_secs, d.behaviour.seek_step_secs);
        assert_eq!(c.behaviour.volume_step, d.behaviour.volume_step);
        assert_eq!(c.ui.tick_ms, d.ui.tick_ms);
        assert_eq!(c.ui.theme, d.ui.theme);
    }

    #[test]
    fn every_commented_keybinding_in_the_example_names_a_real_action() {
        // A typo'd action name in the example is silently ignored by the keymap,
        // so the user would rebind a key and see nothing happen.
        let mut checked = 0;
        for line in crate::config::EXAMPLE_TOML.lines() {
            let l = line.trim().trim_start_matches('#').trim();
            let Some((name, _)) = l.split_once(" = ") else {
                continue;
            };
            // Only the [keys] block uses quoted single-char values.
            if !l.contains('"') || name.contains('.') {
                continue;
            }
            if KeyMap::action_names().contains(&name) {
                checked += 1;
            }
        }
        assert!(
            checked >= 25,
            "expected the example to document the bindings, matched {checked}"
        );
    }

    #[test]
    fn reloading_applies_a_rebound_key_and_a_new_theme() {
        let r = reload_config(
            r#"
            [ui]
            theme = "gruvbox"
            [keys]
            toggle_visual = "z"
            "#,
        )
        .expect("valid config must reload");
        assert_eq!(r.theme_name, "gruvbox");
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert_eq!(
            r.keymap.resolve(
                KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
                ytm_tui::app::Focus::Main
            ),
            Some(InputAction::ToggleVisual)
        );
    }

    #[test]
    fn reloading_a_broken_config_is_an_error_rather_than_a_reset() {
        // The running keymap and theme must survive a typo: resetting to
        // defaults mid-session would be worse than refusing.
        assert!(reload_config("[ui]\ntheme = \"no-such-theme\"").is_err());
        assert!(reload_config("this is not toml").is_err());
        assert!(reload_config("[behaviour]\nvolume_step = 0").is_err());
    }
}
