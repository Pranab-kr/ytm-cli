//! The single source of truth. Owned by the event loop — no locks, no sharing.

use crate::event::{AppEvent, InputAction};
use crate::mutation::{Mutation, MutationLog};
use std::collections::HashSet;
use ytm_core::*;
use ytm_player::player::{PlaybackState, PlayerEvent, RepeatMode};

pub const TOAST_TTL_MS: u64 = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Playlists,
    Songs,
    Albums,
    Artists,
    Search,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Sidebar,
    Main,
    SearchInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    /// Millis since app start, so expiry is testable without a clock.
    pub born_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeletePlaylist(PlaylistId),
    RemoveTracks {
        playlist: PlaylistId,
        entries: Vec<SetVideoId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Confirm {
        text: String,
        action: ConfirmAction,
    },
    Prompt {
        title: String,
        value: String,
        action: PromptAction,
    },
    Help,
    Login {
        user_code: String,
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAction {
    CreatePlaylist,
    RenamePlaylist(PlaylistId),
}

#[derive(Default)]
pub struct AppState {
    pub pane: Pane,
    pub focus: Focus,
    pub selected: usize,
    pub sidebar_selected: usize,
    pub scroll_offset: usize,

    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub open_playlist: Option<PlaylistId>,

    pub search_query: String,
    pub search_results: Vec<Track>,

    /// Multi-select for bulk add/remove (FR-C4).
    pub marked: HashSet<VideoId>,

    pub now_playing: Option<Track>,
    pub playback: PlaybackState,
    pub position: TrackDuration,
    pub duration: TrackDuration,
    pub volume: u8,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue: Vec<Track>,
    pub queue_current: Option<usize>,

    /// Edits applied locally that the server has not confirmed yet (FR-C6).
    pub pending: MutationLog,
    pub loading: bool,
    pub toasts: Vec<Toast>,
    pub modal: Option<Modal>,
    pub should_quit: bool,
    pub elapsed_ms: u64,
}

impl AppState {
    pub fn apply(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Input(a) => self.apply_input(a),
            AppEvent::Player(p) => self.apply_player(p),
            AppEvent::Tick => self.expire_toasts(self.elapsed_ms),
            AppEvent::Resize => {}

            AppEvent::PlaylistsLoaded(v) => {
                self.playlists = v;
                self.loading = false;
            }
            AppEvent::LibrarySongsLoaded(v) => {
                self.tracks = v;
                self.loading = false;
            }
            AppEvent::AlbumsLoaded(v) => {
                self.albums = v;
                self.loading = false;
            }
            AppEvent::ArtistsLoaded(v) => {
                self.artists = v;
                self.loading = false;
            }
            AppEvent::PlaylistTracksLoaded { id, tracks } => {
                self.open_playlist = Some(id);
                self.tracks = tracks;
                self.selected = 0;
                self.loading = false;
            }
            AppEvent::SearchResults { query, tracks } => {
                // Ignore results for a query the user has already moved past.
                if query == self.search_query {
                    self.search_results = tracks;
                    self.selected = 0;
                    self.loading = false;
                }
            }

            AppEvent::MutationOk {
                token,
                real_id,
                message,
            } => {
                self.commit(token, real_id);
                self.push_toast(ToastKind::Success, &message, self.elapsed_ms);
            }
            AppEvent::MutationFailed { token, message } => {
                self.rollback(token);
                self.push_toast(ToastKind::Error, &message, self.elapsed_ms);
            }

            AppEvent::Error(m) => self.push_toast(ToastKind::Error, &m, self.elapsed_ms),
            AppEvent::LoginNeeded { user_code, url } => {
                self.modal = Some(Modal::Login { user_code, url });
            }
            AppEvent::LoginComplete => {
                self.modal = None;
                self.push_toast(ToastKind::Success, "signed in", self.elapsed_ms);
            }
        }
    }

    fn apply_input(&mut self, a: InputAction) {
        // A modal owns the keyboard while it is open. Note `Quit` is absent:
        // inside a prompt the keymap resolves `q` to `Char('q')`, and a confirm
        // must be answered or cancelled rather than quit out of.
        if let Some(modal) = self.modal.as_mut() {
            match (modal, a) {
                (_, InputAction::Cancel) => self.modal = None,
                (Modal::Prompt { value, .. }, InputAction::Char(c)) => value.push(c),
                // pop() removes a whole char — byte slicing would panic on
                // multibyte input.
                (Modal::Prompt { value, .. }, InputAction::Backspace) => {
                    value.pop();
                }
                // Submission is the loop's job: it owns the API calls.
                _ => {}
            }
            return;
        }
        // While the search field has focus, printable keys are text. The keymap
        // already resolves them to `Char`/`Backspace` rather than commands, so
        // this arm only has to edit the buffer.
        if self.focus == Focus::SearchInput {
            match a {
                InputAction::Char(c) => {
                    self.search_query.push(c);
                    self.selected = 0;
                    return;
                }
                InputAction::Backspace => {
                    // By character, not byte: truncating mid-codepoint panics.
                    self.search_query.pop();
                    self.selected = 0;
                    return;
                }
                // Esc and Enter both leave the field for the results list.
                // Neither clears the query or the results — the user is going
                // to navigate what they just found.
                InputAction::Cancel | InputAction::Confirm => {
                    self.focus = Focus::Main;
                    return;
                }
                _ => {}
            }
        }
        match a {
            InputAction::Quit => self.should_quit = true,
            InputAction::Down => self.select_next(),
            InputAction::Up => self.select_prev(),
            InputAction::Home => self.selected = 0,
            InputAction::End => self.selected = self.list_len().saturating_sub(1),
            InputAction::OpenHelp => self.modal = Some(Modal::Help),
            InputAction::OpenSearch => {
                self.set_pane(Pane::Search);
                self.focus = Focus::SearchInput;
            }
            InputAction::OpenQueue => self.set_pane(Pane::Queue),
            InputAction::Left => self.focus = Focus::Sidebar,
            InputAction::Right => self.focus = Focus::Main,
            _ => {} // transport actions are handled by the loop, not here
        }
    }

    fn apply_player(&mut self, p: PlayerEvent) {
        match p {
            PlayerEvent::StateChanged(s) => self.playback = s,
            PlayerEvent::TrackChanged(t) => {
                self.now_playing = t;
                self.position = TrackDuration::default();
            }
            PlayerEvent::Progress { position, duration } => {
                self.position = position;
                self.duration = duration;
            }
            PlayerEvent::VolumeChanged(v) => self.volume = v,
            PlayerEvent::ShuffleChanged(s) => self.shuffle = s,
            PlayerEvent::RepeatChanged(r) => self.repeat = r,
            PlayerEvent::QueueChanged { tracks, current } => {
                self.queue = tracks;
                self.queue_current = current;
            }
            PlayerEvent::Error(m) => self.push_toast(ToastKind::Error, &m, self.elapsed_ms),
            PlayerEvent::TrackEnded(_) => {}
        }
    }

    /// Row count of whatever the main pane is showing.
    pub fn list_len(&self) -> usize {
        match self.pane {
            Pane::Playlists if self.open_playlist.is_some() => self.tracks.len(),
            Pane::Playlists => self.playlists.len(),
            Pane::Songs => self.tracks.len(),
            Pane::Albums => self.albums.len(),
            Pane::Artists => self.artists.len(),
            Pane::Search => self.search_results.len(),
            Pane::Queue => self.queue.len(),
        }
    }

    pub fn select_next(&mut self) {
        let n = self.list_len();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Always reset the selection — a stale index points at the wrong row.
    pub fn set_pane(&mut self, p: Pane) {
        self.pane = p;
        self.selected = 0;
        self.scroll_offset = 0;
        self.marked.clear();
    }

    pub fn push_toast(&mut self, kind: ToastKind, text: &str, now_ms: u64) {
        self.toasts.push(Toast {
            kind,
            text: text.to_owned(),
            born_ms: now_ms,
        });
    }

    pub fn expire_toasts(&mut self, now_ms: u64) {
        self.toasts
            .retain(|t| now_ms.saturating_sub(t.born_ms) < TOAST_TTL_MS);
    }

    /// Apply the edit to local state right now and return its token.
    ///
    /// The point of FR-C6 is that the list changes under the user's hands
    /// instead of after a round trip, so this never waits for the network.
    pub fn begin_mutation(&mut self, m: Mutation) -> u64 {
        let token = self.pending.next_token();
        match &m {
            Mutation::CreatePlaylist { temp } => self.playlists.push(temp.clone()),
            Mutation::RenamePlaylist { id, next, .. } => {
                if let Some(p) = self.playlists.iter_mut().find(|p| &p.id == id) {
                    p.title = next.clone();
                }
            }
            Mutation::DeletePlaylist { id, .. } => self.playlists.retain(|p| &p.id != id),
            // Nothing local to show: the tracks were added to a playlist that
            // is not necessarily the one on screen.
            Mutation::AddTracks { .. } => {}
            Mutation::RemoveTracks { removed, .. } => {
                let drop: Vec<_> = removed.iter().map(|(_, t)| t.video_id.clone()).collect();
                self.tracks.retain(|t| !drop.contains(&t.video_id));
            }
        }
        self.pending.insert(token, m);
        token
    }

    /// The server accepted it. For a create, swap the temp id for the real one.
    pub fn commit(&mut self, token: u64, real_id: Option<PlaylistId>) {
        if let Some(Mutation::CreatePlaylist { temp }) = self.pending.take(token)
            && let Some(real) = real_id
            && let Some(p) = self.playlists.iter_mut().find(|p| p.id == temp.id)
        {
            p.id = real;
        }
    }

    /// The server rejected it. Undo exactly this edit.
    ///
    /// Keyed by token, not "the last change": with two edits in flight the
    /// responses can arrive in either order, and reverting the newest would
    /// discard an edit that actually succeeded.
    pub fn rollback(&mut self, token: u64) {
        let Some(m) = self.pending.take(token) else {
            return;
        };
        match m {
            Mutation::CreatePlaylist { temp } => self.playlists.retain(|p| p.id != temp.id),
            Mutation::RenamePlaylist { id, previous, .. } => {
                if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
                    p.title = previous;
                }
            }
            Mutation::DeletePlaylist {
                index, snapshot, ..
            } => {
                let at = index.min(self.playlists.len());
                self.playlists.insert(at, snapshot);
            }
            Mutation::AddTracks { .. } => {}
            Mutation::RemoveTracks { removed, .. } => {
                // Ascending order so each insert lands at its original index.
                for (idx, track) in removed {
                    let at = idx.min(self.tracks.len());
                    self.tracks.insert(at, track);
                }
            }
        }
    }

    pub fn selected_track(&self) -> Option<&Track> {
        match self.pane {
            Pane::Search => self.search_results.get(self.selected),
            Pane::Queue => self.queue.get(self.selected),
            _ => self.tracks.get(self.selected),
        }
    }

    pub fn selected_playlist(&self) -> Option<&Playlist> {
        (self.pane == Pane::Playlists && self.open_playlist.is_none())
            .then(|| self.playlists.get(self.selected))
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::{Playlist, Track};

    #[test]
    fn starts_focused_on_the_sidebar_with_nothing_loaded() {
        let s = AppState::default();
        assert_eq!(s.focus, Focus::Sidebar);
        assert_eq!(s.pane, Pane::Playlists);
        assert!(s.playlists.is_empty());
        assert!(!s.should_quit);
    }

    #[test]
    fn playlists_loaded_replaces_the_list_and_clears_loading() {
        let mut s = AppState {
            loading: true,
            ..Default::default()
        };
        s.apply(AppEvent::PlaylistsLoaded(vec![Playlist::stub(
            "p1", "Focus",
        )]));
        assert_eq!(s.playlists.len(), 1);
        assert!(!s.loading, "spinner must stop when data lands (FR-U4)");
    }

    #[test]
    fn an_error_event_becomes_an_error_toast() {
        let mut s = AppState::default();
        s.apply(AppEvent::Error("rate limited".into()));
        assert_eq!(s.toasts.len(), 1);
        assert_eq!(s.toasts[0].kind, ToastKind::Error);
        assert_eq!(s.toasts[0].text, "rate limited");
    }

    #[test]
    fn toasts_expire_after_four_seconds() {
        let mut s = AppState::default();
        s.push_toast(ToastKind::Info, "hi", 1000);
        s.expire_toasts(1000 + TOAST_TTL_MS - 1);
        assert_eq!(s.toasts.len(), 1);
        s.expire_toasts(1000 + TOAST_TTL_MS + 1);
        assert!(s.toasts.is_empty(), "FR-U3: toasts auto-dismiss");
    }

    #[test]
    fn selection_moves_and_clamps_at_both_ends() {
        // Pane::Songs is the pane whose rows come from `tracks`; the default
        // Playlists pane counts `playlists`, so a tracks-only fixture there
        // would leave list_len() at 0 and nothing to move through.
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![Track::stub("a", "A"), Track::stub("b", "B")],
            focus: Focus::Main,
            selected: 0,
            ..Default::default()
        };
        s.select_next();
        assert_eq!(s.selected, 1);
        s.select_next();
        assert_eq!(s.selected, 1, "clamps at the end");
        s.select_prev();
        s.select_prev();
        assert_eq!(s.selected, 0, "clamps at the start");
    }

    #[test]
    fn switching_pane_resets_the_selection() {
        let mut s = AppState {
            tracks: vec![Track::stub("a", "A"), Track::stub("b", "B")],
            selected: 1,
            ..Default::default()
        };
        s.set_pane(Pane::Search);
        assert_eq!(s.selected, 0, "a stale index would point at the wrong row");
    }

    #[test]
    fn progress_updates_position_and_duration() {
        let mut s = AppState::default();
        s.apply(AppEvent::Player(PlayerEvent::Progress {
            position: TrackDuration::from_secs(30),
            duration: TrackDuration::from_secs(200),
        }));
        assert_eq!(s.position.as_secs(), 30);
        assert_eq!(s.duration.as_secs(), 200);
    }

    #[test]
    fn quit_action_sets_the_quit_flag() {
        let mut s = AppState::default();
        s.apply(AppEvent::Input(InputAction::Quit));
        assert!(s.should_quit);
    }

    #[test]
    fn typing_in_the_search_field_builds_the_query() {
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            ..Default::default()
        };
        for c in "boa".chars() {
            s.apply(AppEvent::Input(InputAction::Char(c)));
        }
        assert_eq!(s.search_query, "boa");
    }

    #[test]
    fn backspace_deletes_the_last_character_of_the_query() {
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "boa".into(),
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Backspace));
        assert_eq!(s.search_query, "bo");
    }

    #[test]
    fn backspace_on_an_empty_query_is_harmless() {
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Backspace));
        assert_eq!(s.search_query, "");
    }

    #[test]
    fn backspace_removes_a_whole_multibyte_character() {
        // Truncating by one *byte* would leave invalid UTF-8 and panic.
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "日本".into(),
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Backspace));
        assert_eq!(s.search_query, "日");
    }

    #[test]
    fn escape_leaves_the_search_field_without_clearing_the_results() {
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "boards".into(),
            search_results: vec![Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert_eq!(s.focus, Focus::Main, "Esc returns to the results list");
        assert_eq!(s.search_results.len(), 1, "results must survive");
    }

    #[test]
    fn typing_outside_the_search_field_does_not_edit_the_query() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Char('j')));
        assert_eq!(s.search_query, "");
    }

    #[test]
    fn a_modal_swallows_navigation_so_it_cannot_move_the_list_behind_it() {
        let mut s = AppState {
            tracks: vec![Track::stub("a", "A"), Track::stub("b", "B")],
            modal: Some(Modal::Confirm {
                text: "sure?".into(),
                action: ConfirmAction::DeletePlaylist("p1".into()),
            }),
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(s.selected, 0, "modal must capture input");
    }
}
