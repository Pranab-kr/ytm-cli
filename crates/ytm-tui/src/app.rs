//! The single source of truth. Owned by the event loop — no locks, no sharing.

use crate::event::{AppEvent, InputAction};
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

            AppEvent::MutationOk { message, .. } => {
                self.push_toast(ToastKind::Success, &message, self.elapsed_ms);
            }
            AppEvent::MutationFailed { message, .. } => {
                // Rollback itself is wired in Task 28.
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
        // A modal owns the keyboard while it is open.
        if self.modal.is_some() {
            match a {
                InputAction::Cancel => self.modal = None,
                InputAction::Quit => self.should_quit = true,
                _ => {}
            }
            return;
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
