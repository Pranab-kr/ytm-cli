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

/// Sidebar order, and therefore what the number keys 1-6 select. The sidebar
/// widget renders from `SOURCES`, which must stay in this order — there is a
/// test pinning the two together.
pub const PANE_ORDER: [Pane; 6] = [
    Pane::Playlists,
    Pane::Songs,
    Pane::Albums,
    Pane::Artists,
    Pane::Search,
    Pane::Queue,
];

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
    /// Which playlist to add the pending tracks to (FR-C4).
    ///
    /// `choices` is carried in the modal rather than read from `playlists` at
    /// draw time so the row the user picks cannot change under them if a
    /// refresh lands mid-decision.
    PickPlaylist {
        targets: Vec<VideoId>,
        choices: Vec<(PlaylistId, String)>,
        selected: usize,
    },
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
    /// Byte offset of the caret within `search_query`. Bytes, not chars, so it
    /// can index the string directly — always kept on a char boundary.
    pub search_cursor: usize,
    pub search_results: Vec<Track>,

    /// Multi-select for bulk add/remove (FR-C4).
    pub marked: HashSet<VideoId>,

    /// Anchor row of an active range selection, or `None` outside visual mode.
    pub visual_anchor: Option<usize>,
    /// Marks that existed when visual mode began, so recomputing the range
    /// cannot discard rows the user had already marked by hand.
    pub marks_before_visual: HashSet<VideoId>,

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

            // Art is owned by the loop's `ArtCache`, not by state — protocol
            // objects are not comparable, cloneable, or meaningful to a test
            // backend. State ignores them so `apply` stays a pure reducer.
            AppEvent::ArtLoaded { .. } | AppEvent::ArtFailed { .. } => {}

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
                (
                    Modal::PickPlaylist {
                        selected, choices, ..
                    },
                    InputAction::Down,
                ) => *selected = (*selected + 1).min(choices.len().saturating_sub(1)),
                (Modal::PickPlaylist { selected, .. }, InputAction::Up) => {
                    *selected = selected.saturating_sub(1)
                }
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
                    let at = self.clamped_cursor();
                    self.search_query.insert(at, c);
                    self.search_cursor = at + c.len_utf8();
                    self.selected = 0;
                    return;
                }
                InputAction::Backspace => {
                    // Remove the char before the caret, by boundary rather than
                    // by byte: slicing mid-codepoint panics.
                    let at = self.clamped_cursor();
                    if let Some((i, _)) = self.search_query[..at].char_indices().next_back() {
                        self.search_query.remove(i);
                        self.search_cursor = i;
                    }
                    self.selected = 0;
                    return;
                }
                // Ctrl+W. Shell-style, so it eats a trailing space with the word.
                InputAction::DeleteWordBack => {
                    let at = self.clamped_cursor();
                    let from = crate::util::text::prev_word_boundary(&self.search_query, at);
                    self.search_query.replace_range(from..at, "");
                    self.search_cursor = from;
                    self.selected = 0;
                    return;
                }
                InputAction::WordLeft => {
                    self.search_cursor = crate::util::text::prev_word_boundary(
                        &self.search_query,
                        self.clamped_cursor(),
                    );
                    return;
                }
                InputAction::WordRight => {
                    self.search_cursor = crate::util::text::next_word_boundary(
                        &self.search_query,
                        self.clamped_cursor(),
                    );
                    return;
                }
                InputAction::CharLeft => {
                    let at = self.clamped_cursor();
                    self.search_cursor = self.search_query[..at]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    return;
                }
                InputAction::CharRight => {
                    let at = self.clamped_cursor();
                    self.search_cursor = self.search_query[at..]
                        .chars()
                        .next()
                        .map(|c| at + c.len_utf8())
                        .unwrap_or(at);
                    return;
                }
                InputAction::LineStart => {
                    self.search_cursor = 0;
                    return;
                }
                InputAction::LineEnd => {
                    self.search_cursor = self.search_query.len();
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
            // Every row movement redraws an active range, so the four are
            // grouped rather than each remembering to call the refresh.
            InputAction::Down | InputAction::Up | InputAction::Home | InputAction::End => {
                match a {
                    InputAction::Down => self.select_next(),
                    InputAction::Up => self.select_prev(),
                    InputAction::Home => self.selected = 0,
                    _ => self.selected = self.list_len().saturating_sub(1),
                }
                self.refresh_visual_marks();
            }
            InputAction::ToggleVisual => self.toggle_visual(),
            InputAction::OpenHelp => self.modal = Some(Modal::Help),
            InputAction::OpenSearch => {
                self.set_pane(Pane::Search);
                self.focus = Focus::SearchInput;
                // Reopening lands the caret after whatever query is still there.
                self.search_cursor = self.search_query.len();
            }
            InputAction::OpenQueue => self.set_pane(Pane::Queue),
            // Esc means "undo this selection" while a range is being made.
            // Guarded so it only claims the key during visual mode; outside it,
            // Esc keeps whatever meaning it had.
            InputAction::Cancel if self.visual_anchor.is_some() => self.cancel_visual(),
            // `h` is "go up a level" first and "focus the sidebar" second, so
            // the pair reads like opening and closing a folder. Only the
            // playlist pane has a level to leave; everywhere else `h` keeps its
            // old meaning rather than swallowing the key.
            InputAction::Left => {
                if self.close_open_playlist() {
                    // Stay in the list: the user is navigating it, not leaving it.
                } else {
                    self.focus = Focus::Sidebar;
                }
            }
            InputAction::Right => self.focus = Focus::Main,
            InputAction::GoTo(n) => self.goto_source(n),
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

    /// The focus the *keymap* should resolve against, which is not always the
    /// focus the user is navigating with.
    ///
    /// `Focus::SearchInput` means "letters are literal, not commands". Two
    /// modals need that: a prompt, whose letters are the name being typed, and a
    /// confirm, whose `y`/`n` are answers. Resolving against `self.focus`
    /// instead left it as Sidebar/Main while the modal was open, so in a prompt
    /// `q` arrived as Quit, and in a confirm `y` matched nothing at all while
    /// `n` skipped the track behind the box.
    ///
    /// The picker keeps command focus on purpose: it is a list, so `j`/`k` and
    /// the arrows should move through it.
    pub fn input_focus(&self) -> Focus {
        match self.modal {
            Some(Modal::Prompt { .. }) | Some(Modal::Confirm { .. }) => Focus::SearchInput,
            _ => self.focus,
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
    /// Leave an open playlist and show the playlist list again. Returns false
    /// when there was no level to leave, so the caller can fall back.
    ///
    /// Clears `tracks` as well as the id: they are the open playlist's rows, and
    /// leaving them would make `list_len` and `selected_track` disagree with
    /// what is on screen.
    pub fn close_open_playlist(&mut self) -> bool {
        if self.pane != Pane::Playlists || self.open_playlist.is_none() {
            return false;
        }
        self.open_playlist = None;
        self.tracks.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.marked.clear();
        self.visual_anchor = None;
        self.marks_before_visual.clear();
        true
    }

    /// Jump straight to the nth source, counting from 1 in sidebar order.
    ///
    /// Out of range is ignored rather than clamped: clamping would make `9` mean
    /// "Queue", which is not what the user pressed.
    pub fn goto_source(&mut self, n: u8) {
        let Some(idx) = (n as usize).checked_sub(1) else {
            return;
        };
        let Some(pane) = PANE_ORDER.get(idx).copied() else {
            return;
        };
        self.sidebar_selected = idx;
        self.set_pane(pane);
        // Land in the list, not on the sidebar — the number key already said
        // which source, so stopping at the sidebar would need a second key.
        self.focus = if pane == Pane::Search {
            // Otherwise letters would scroll instead of typing a query.
            Focus::SearchInput
        } else {
            Focus::Main
        };
    }

    pub fn set_pane(&mut self, p: Pane) {
        self.pane = p;
        self.selected = 0;
        self.scroll_offset = 0;
        self.marked.clear();
        // The anchor indexes the pane being left. Carrying it over would mark
        // whichever rows happened to sit at those indices in the new pane.
        self.visual_anchor = None;
        self.marks_before_visual.clear();
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

    /// The caret, guaranteed in range and on a char boundary.
    ///
    /// The query can be replaced from outside the field (a cleared pane, a
    /// restored session), so a stored offset may be stale — and slicing a stale
    /// one panics rather than misbehaving.
    fn clamped_cursor(&self) -> usize {
        let mut at = self.search_cursor.min(self.search_query.len());
        while at > 0 && !self.search_query.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// Video ids of the rows the main pane is showing, in display order.
    ///
    /// Mirrors `selected_track`'s pane mapping so a range and a hand-made mark
    /// can never disagree about which list they are indexing.
    fn row_ids(&self) -> Vec<VideoId> {
        self.track_rows()
            .iter()
            .map(|t| t.video_id.clone())
            .collect()
    }

    /// The pane's rows when they are tracks, empty when they are not.
    ///
    /// One place so `selected_track`, a visual range, and the bulk-action
    /// targets can never disagree about which list is on screen.
    pub fn track_rows(&self) -> &[Track] {
        match self.pane {
            Pane::Search => &self.search_results,
            Pane::Queue => &self.queue,
            Pane::Songs => &self.tracks,
            Pane::Playlists if self.open_playlist.is_some() => &self.tracks,
            Pane::Playlists | Pane::Albums | Pane::Artists => &[],
        }
    }

    /// Start or end a range selection anchored at the current row (`V`).
    ///
    /// Ending it keeps the marks: the selection exists so `A` or `x` can act on
    /// it, so dropping them here would make the mode useless. `Esc` is the way
    /// out that undoes it.
    pub fn toggle_visual(&mut self) {
        if self.visual_anchor.is_some() {
            self.visual_anchor = None;
            self.marks_before_visual.clear();
            return;
        }
        // Nothing to anchor to on an empty list, and an anchor into a list with
        // no rows would mark by coincidence once one loaded.
        if self.selected_track().is_none() {
            return;
        }
        // Remembered so extending, shrinking, or cancelling the range can be
        // recomputed from scratch without discarding marks made by hand.
        self.marks_before_visual = self.marked.clone();
        self.visual_anchor = Some(self.selected);
        self.refresh_visual_marks();
    }

    /// Leave visual mode and put the marks back as they were when it started.
    pub fn cancel_visual(&mut self) {
        if self.visual_anchor.take().is_some() {
            self.marked = std::mem::take(&mut self.marks_before_visual);
        }
    }

    /// Redraw the range after the cursor moved.
    ///
    /// Recomputed from the anchor rather than accumulated, so walking back over
    /// rows unmarks them instead of leaving the overshoot behind.
    pub fn refresh_visual_marks(&mut self) {
        let Some(anchor) = self.visual_anchor else {
            return;
        };
        let rows = self.row_ids();
        if rows.is_empty() {
            return;
        }
        // Clamped: a refresh can land after the list shrank under the anchor.
        let last = rows.len() - 1;
        let a = anchor.min(last);
        let b = self.selected.min(last);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.marked = self.marks_before_visual.clone();
        self.marked.extend(rows[lo..=hi].iter().cloned());
    }

    /// True while a range selection is being made — the status line says so.
    pub fn in_visual_mode(&self) -> bool {
        self.visual_anchor.is_some()
    }

    /// Mark or unmark the selected track for a bulk action (FR-C4).
    pub fn toggle_mark(&mut self) {
        let Some(id) = self.selected_track().map(|t| t.video_id.clone()) else {
            return;
        };
        if !self.marked.remove(&id) {
            self.marked.insert(id);
        }
    }

    /// The track under the cursor, or `None` when this pane's rows are not
    /// tracks.
    ///
    /// Exhaustive on `Pane` rather than falling through to `tracks`: Albums,
    /// Artists, and the playlist list keep whatever `tracks` was last loaded
    /// with, so the old `_` arm reported a song that was not on screen. `Enter`
    /// then played it and `a` queued it, and `v` marked it invisibly.
    pub fn selected_track(&self) -> Option<&Track> {
        match self.pane {
            Pane::Search => self.search_results.get(self.selected),
            Pane::Queue => self.queue.get(self.selected),
            Pane::Songs => self.tracks.get(self.selected),
            // Only an open playlist shows tracks; the list of playlists does not.
            Pane::Playlists => self
                .open_playlist
                .is_some()
                .then(|| self.tracks.get(self.selected))
                .flatten(),
            Pane::Albums | Pane::Artists => None,
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
            // The field has a caret now, so a test that types must place it.
            search_cursor: 3,
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
            search_cursor: "日本".len(),
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
    fn a_prompt_modal_makes_the_keymap_treat_letters_as_text() {
        // The bug: `keymap.resolve` was given `state.focus`, which is still
        // Sidebar/Main while a modal is open, so `q` resolved to Quit and
        // Char(c) was never produced. Typing a playlist name was impossible.
        let s = AppState {
            modal: Some(Modal::Prompt {
                title: "Name".into(),
                value: String::new(),
                action: PromptAction::CreatePlaylist,
            }),
            ..Default::default()
        };
        assert_eq!(s.input_focus(), Focus::SearchInput);
    }

    #[test]
    fn a_confirm_modal_makes_y_and_n_literal_answers() {
        // With command focus `y` resolved to nothing and `n` to NextTrack, so a
        // delete could be neither accepted nor declined.
        let s = AppState {
            modal: Some(Modal::Confirm {
                text: "sure?".into(),
                action: ConfirmAction::DeletePlaylist("p1".into()),
            }),
            ..Default::default()
        };
        assert_eq!(s.input_focus(), Focus::SearchInput);
    }

    #[test]
    fn the_picker_keeps_command_focus_so_navigation_still_resolves() {
        let s = AppState {
            modal: Some(Modal::PickPlaylist {
                targets: vec![],
                choices: vec![],
                selected: 0,
            }),
            focus: Focus::Main,
            ..Default::default()
        };
        assert_eq!(s.input_focus(), Focus::Main);
    }

    #[test]
    fn without_a_modal_input_focus_is_just_the_focus() {
        let s = AppState {
            focus: Focus::Main,
            ..Default::default()
        };
        assert_eq!(s.input_focus(), Focus::Main);
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

    #[test]
    fn left_closes_an_open_playlist_like_going_up_a_folder() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![Playlist::stub("p1", "Focus")],
            open_playlist: Some("p1".into()),
            tracks: vec![Track::stub("v1", "A")],
            focus: Focus::Main,
            selected: 3,
            ..Default::default()
        };
        s.apply_input(InputAction::Left);
        assert!(s.open_playlist.is_none(), "the playlist should be closed");
        assert!(
            s.tracks.is_empty(),
            "its tracks are no longer what the pane shows"
        );
        assert_eq!(
            s.selected, 0,
            "selection returns to the top of the playlist list"
        );
        assert_eq!(s.focus, Focus::Main, "closing keeps focus on the list");
    }

    #[test]
    fn left_with_nothing_open_falls_back_to_focusing_the_sidebar() {
        let mut s = AppState {
            pane: Pane::Playlists,
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply_input(InputAction::Left);
        assert_eq!(s.focus, Focus::Sidebar);
    }

    #[test]
    fn left_in_a_pane_with_no_hierarchy_just_focuses_the_sidebar() {
        // Songs has nothing to go "back" to, so h must not eat the keypress.
        let mut s = AppState {
            pane: Pane::Songs,
            tracks: vec![Track::stub("v1", "A")],
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply_input(InputAction::Left);
        assert_eq!(s.focus, Focus::Sidebar);
        assert_eq!(s.tracks.len(), 1, "Songs tracks are not a playlist view");
    }

    #[test]
    fn right_from_the_sidebar_moves_into_the_list() {
        let mut s = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };
        s.apply_input(InputAction::Right);
        assert_eq!(s.focus, Focus::Main);
    }

    #[test]
    fn a_number_key_selects_its_source_and_switches_pane() {
        // 1-6 match the sidebar order top to bottom.
        let mut s = AppState::default();
        s.apply_input(InputAction::GoTo(3));
        assert_eq!(s.pane, Pane::Albums);
        assert_eq!(s.sidebar_selected, 2, "the sidebar highlight follows");
        assert_eq!(s.focus, Focus::Main, "a source jump lands in the list");
    }

    #[test]
    fn number_keys_cover_every_source_in_sidebar_order() {
        for (n, want) in [
            (1, Pane::Playlists),
            (2, Pane::Songs),
            (3, Pane::Albums),
            (4, Pane::Artists),
            (5, Pane::Search),
            (6, Pane::Queue),
        ] {
            let mut s = AppState::default();
            s.apply_input(InputAction::GoTo(n));
            assert_eq!(s.pane, want, "{n} should select {want:?}");
        }
    }

    #[test]
    fn an_out_of_range_number_is_ignored_rather_than_clamped() {
        // Clamping would make 9 mean "Queue", which the user did not ask for.
        let mut s = AppState {
            pane: Pane::Songs,
            ..Default::default()
        };
        s.apply_input(InputAction::GoTo(9));
        assert_eq!(s.pane, Pane::Songs);
        s.apply_input(InputAction::GoTo(0));
        assert_eq!(s.pane, Pane::Songs);
    }

    #[test]
    fn jumping_to_search_focuses_the_input_so_typing_works() {
        // Otherwise `5` lands you in a search pane where letters scroll.
        let mut s = AppState::default();
        s.apply_input(InputAction::GoTo(5));
        assert_eq!(s.pane, Pane::Search);
        assert_eq!(s.focus, Focus::SearchInput);
    }

    #[test]
    fn opening_a_playlist_then_going_back_leaves_the_list_navigable() {
        let mut s = AppState {
            pane: Pane::Playlists,
            playlists: vec![Playlist::stub("p1", "A"), Playlist::stub("p2", "B")],
            open_playlist: Some("p1".into()),
            tracks: vec![Track::stub("v1", "T")],
            ..Default::default()
        };
        s.apply_input(InputAction::Left);
        s.apply_input(InputAction::Down);
        assert_eq!(s.selected, 1, "list_len must count playlists again");
        assert!(s.selected_playlist().is_some());
    }

    /// A songs pane with `n` rows, focused on the list.
    fn songs(n: usize) -> AppState {
        AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: (0..n)
                .map(|i| Track::stub(&format!("v{i}"), &format!("Track {i}")))
                .collect(),
            ..Default::default()
        }
    }

    fn marked_ids(s: &AppState) -> Vec<String> {
        let mut v: Vec<String> = s.marked.iter().map(|i| i.0.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn visual_mode_marks_the_anchor_row_as_soon_as_it_opens() {
        // Otherwise `V` then `A` on a single row would add nothing.
        let mut s = songs(5);
        s.selected = 2;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        assert_eq!(s.visual_anchor, Some(2));
        assert_eq!(marked_ids(&s), vec!["v2"]);
    }

    #[test]
    fn moving_down_in_visual_mode_extends_the_range() {
        let mut s = songs(5);
        s.selected = 1;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(marked_ids(&s), vec!["v1", "v2", "v3"]);
    }

    #[test]
    fn moving_up_from_the_anchor_extends_backwards() {
        // The range is anchor..=cursor in either direction, like vim.
        let mut s = songs(5);
        s.selected = 3;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Up));
        s.apply(AppEvent::Input(InputAction::Up));
        assert_eq!(marked_ids(&s), vec!["v1", "v2", "v3"]);
    }

    #[test]
    fn shrinking_the_range_unmarks_the_rows_left_behind() {
        // Overshooting and coming back must not leave stale marks.
        let mut s = songs(6);
        s.selected = 0;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        for _ in 0..4 {
            s.apply(AppEvent::Input(InputAction::Down));
        }
        assert_eq!(marked_ids(&s).len(), 5);
        for _ in 0..3 {
            s.apply(AppEvent::Input(InputAction::Up));
        }
        assert_eq!(marked_ids(&s), vec!["v0", "v1"]);
    }

    #[test]
    fn crossing_back_over_the_anchor_flips_the_range() {
        let mut s = songs(6);
        s.selected = 3;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down)); // 3..=4
        s.apply(AppEvent::Input(InputAction::Up));
        s.apply(AppEvent::Input(InputAction::Up)); // 2..=3
        assert_eq!(marked_ids(&s), vec!["v2", "v3"]);
    }

    #[test]
    fn a_second_v_leaves_visual_mode_but_keeps_the_marks() {
        // The marks are the point of the selection — `A` comes next.
        let mut s = songs(5);
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        assert_eq!(s.visual_anchor, None, "visual mode must end");
        assert_eq!(marked_ids(&s), vec!["v0", "v1"], "marks survive");
    }

    #[test]
    fn escape_cancels_visual_mode_and_restores_the_previous_marks() {
        // Esc is "undo this selection", so a hand-marked row from before must
        // come back and the range's own rows must go.
        let mut s = songs(6);
        s.selected = 5;
        s.toggle_mark(); // hand-marked v5, before visual mode
        s.selected = 0;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(marked_ids(&s), vec!["v0", "v1", "v5"]);
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert_eq!(s.visual_anchor, None);
        assert_eq!(marked_ids(&s), vec!["v5"], "only the old mark remains");
    }

    #[test]
    fn a_range_selection_adds_to_marks_made_by_hand() {
        // FR-C4 multi-select: `v` on scattered rows then `V` over a run must
        // give the union, not one or the other.
        let mut s = songs(8);
        s.selected = 7;
        s.toggle_mark();
        s.selected = 1;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(marked_ids(&s), vec!["v1", "v2", "v7"]);
    }

    #[test]
    fn home_and_end_extend_the_range_too() {
        // Any movement while visual is on redraws the range, not just j/k.
        let mut s = songs(5);
        s.selected = 2;
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::End));
        assert_eq!(marked_ids(&s), vec!["v2", "v3", "v4"]);
        s.apply(AppEvent::Input(InputAction::Home));
        assert_eq!(marked_ids(&s), vec!["v0", "v1", "v2"]);
    }

    #[test]
    fn changing_pane_leaves_visual_mode() {
        // The anchor indexes the old list; keeping it would mark by coincidence.
        let mut s = songs(5);
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.set_pane(Pane::Albums);
        assert_eq!(s.visual_anchor, None);
        assert!(s.marked.is_empty());
    }

    #[test]
    fn leaving_an_open_playlist_leaves_visual_mode() {
        let mut s = songs(5);
        s.pane = Pane::Playlists;
        s.open_playlist = Some(PlaylistId::from("p1"));
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        assert!(s.close_open_playlist());
        assert_eq!(s.visual_anchor, None);
        assert!(s.marked.is_empty());
    }

    #[test]
    fn visual_mode_on_an_empty_list_does_nothing() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        assert_eq!(s.visual_anchor, None, "no row to anchor to");
        assert!(s.marked.is_empty());
    }

    #[test]
    fn visual_mode_works_in_the_queue_pane() {
        // The queue reads its rows from a different vec, so the range has to
        // follow `selected_track`, not `tracks`.
        let mut s = AppState {
            pane: Pane::Queue,
            focus: Focus::Main,
            queue: (0..4).map(|i| Track::stub(&format!("q{i}"), "T")).collect(),
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::Down));
        assert_eq!(marked_ids(&s), vec!["q0", "q1"]);
    }

    #[test]
    fn a_range_over_the_whole_list_marks_every_row() {
        let mut s = songs(4);
        s.apply(AppEvent::Input(InputAction::ToggleVisual));
        s.apply(AppEvent::Input(InputAction::End));
        assert_eq!(marked_ids(&s), vec!["v0", "v1", "v2", "v3"]);
    }

    #[test]
    fn marking_a_playlist_row_does_not_mark_a_stale_track() {
        let mut s = songs(3);
        // Visit the songs pane, then switch to the playlist list. `tracks` still
        // holds the song rows, but the pane now shows playlists.
        s.set_pane(Pane::Playlists);
        s.playlists = vec![Playlist::stub("p1", "Focus"), Playlist::stub("p2", "Chill")];
        assert!(s.open_playlist.is_none(), "showing the playlist list");
        s.toggle_mark();
        assert!(
            s.marked.is_empty(),
            "a playlist row is not a track; marked {:?}",
            marked_ids(&s)
        );
    }

    #[test]
    fn the_albums_pane_reports_no_selected_track() {
        // `selected_track` falls through to `tracks` for every pane it does not
        // name, so a pane showing albums reports a song that is not on screen.
        let mut s = songs(3);
        s.set_pane(Pane::Albums);
        s.albums = vec![Album {
            id: AlbumId::from("a1"),
            title: "Geogaddi".into(),
            artists: vec![],
            year: None,
            thumbnail_url: None,
        }];
        assert!(
            s.selected_track().is_none(),
            "an album row is not a track, got {:?}",
            s.selected_track().map(|t| t.title.clone())
        );
    }

    #[test]
    fn the_playlist_list_reports_no_selected_track() {
        let mut s = songs(3);
        s.set_pane(Pane::Playlists);
        s.playlists = vec![Playlist::stub("p1", "Focus")];
        assert!(
            s.selected_track().is_none(),
            "a playlist row is not a track"
        );
    }

    /// A search pane with the field focused and `q` already typed.
    fn searching(q: &str) -> AppState {
        AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: q.to_owned(),
            search_cursor: q.len(),
            ..Default::default()
        }
    }

    fn press(s: &mut AppState, a: InputAction) {
        s.apply(AppEvent::Input(a));
    }

    #[test]
    fn ctrl_w_deletes_the_word_before_the_cursor() {
        let mut s = searching("boards of canada");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "boards of ");
        assert_eq!(s.search_cursor, s.search_query.len());
    }

    #[test]
    fn ctrl_w_deletes_only_up_to_the_cursor() {
        // With the caret mid-line the tail must survive.
        let mut s = searching("boards of canada");
        s.search_cursor = "boards of".len();
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "boards  canada");
        assert_eq!(s.search_cursor, "boards ".len());
    }

    #[test]
    fn ctrl_w_on_an_empty_field_is_harmless() {
        let mut s = searching("");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "");
        assert_eq!(s.search_cursor, 0);
    }

    #[test]
    fn ctrl_w_repeated_clears_the_line_word_by_word() {
        let mut s = searching("one two three");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "one two ");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "one ");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "");
        press(&mut s, InputAction::DeleteWordBack);
        assert_eq!(s.search_query, "", "still nothing to delete");
    }

    #[test]
    fn ctrl_arrows_move_the_cursor_a_word_at_a_time() {
        let mut s = searching("boards of canada");
        press(&mut s, InputAction::WordLeft);
        assert_eq!(s.search_cursor, "boards of ".len());
        press(&mut s, InputAction::WordLeft);
        assert_eq!(s.search_cursor, "boards ".len());
        press(&mut s, InputAction::WordRight);
        assert_eq!(s.search_cursor, "boards of ".len());
    }

    #[test]
    fn word_motion_clamps_at_both_ends() {
        let mut s = searching("boards");
        s.search_cursor = 0;
        press(&mut s, InputAction::WordLeft);
        assert_eq!(s.search_cursor, 0);
        press(&mut s, InputAction::WordRight);
        press(&mut s, InputAction::WordRight);
        assert_eq!(s.search_cursor, "boards".len());
    }

    #[test]
    fn typing_inserts_at_the_cursor_not_the_end() {
        let mut s = searching("boards canada");
        s.search_cursor = "boards ".len();
        press(&mut s, InputAction::Char('o'));
        press(&mut s, InputAction::Char('f'));
        press(&mut s, InputAction::Char(' '));
        assert_eq!(s.search_query, "boards of canada");
        assert_eq!(s.search_cursor, "boards of ".len());
    }

    #[test]
    fn backspace_deletes_at_the_cursor_not_the_end() {
        let mut s = searching("boardss of");
        s.search_cursor = "boardss".len();
        press(&mut s, InputAction::Backspace);
        assert_eq!(s.search_query, "boards of");
        assert_eq!(s.search_cursor, "boards".len());
    }

    #[test]
    fn backspace_removes_a_whole_multibyte_character_at_the_cursor() {
        // Byte-slicing a codepoint would panic and take the terminal with it.
        let mut s = searching("日本語");
        press(&mut s, InputAction::Backspace);
        assert_eq!(s.search_query, "日本");
        assert!(s.search_query.is_char_boundary(s.search_cursor));
    }

    #[test]
    fn left_and_right_move_by_character_inside_the_field() {
        let mut s = searching("abc");
        press(&mut s, InputAction::CharLeft);
        assert_eq!(s.search_cursor, 2);
        press(&mut s, InputAction::CharRight);
        assert_eq!(s.search_cursor, 3);
    }

    #[test]
    fn character_motion_steps_over_a_whole_multibyte_character() {
        let mut s = searching("日本");
        press(&mut s, InputAction::CharLeft);
        assert!(s.search_query.is_char_boundary(s.search_cursor));
        assert_eq!(s.search_cursor, "日".len());
    }

    #[test]
    fn ctrl_a_and_ctrl_e_jump_to_the_ends_of_the_line() {
        let mut s = searching("boards of canada");
        press(&mut s, InputAction::LineStart);
        assert_eq!(s.search_cursor, 0);
        press(&mut s, InputAction::LineEnd);
        assert_eq!(s.search_cursor, s.search_query.len());
    }

    #[test]
    fn a_fresh_query_puts_the_cursor_at_the_end() {
        // Reopening the pane and typing must not insert at offset 0.
        let mut s = AppState::default();
        s.apply(AppEvent::Input(InputAction::OpenSearch));
        press(&mut s, InputAction::Char('b'));
        press(&mut s, InputAction::Char('o'));
        assert_eq!(s.search_query, "bo");
        assert_eq!(s.search_cursor, 2);
    }
}
