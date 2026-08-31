//! The single source of truth. Owned by the event loop — no locks, no sharing.

use crate::event::{AppEvent, InputAction};
use crate::mutation::{Mutation, MutationLog};
use std::collections::HashSet;

/// Rows one wheel notch moves. Three is the common terminal default.
const WHEEL_ROWS: usize = 3;
use ytm_core::*;
use ytm_player::player::{PlaybackState, PlayerEvent, RepeatMode};

pub const TOAST_TTL_MS: u64 = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    /// YouTube Music's recommendation shelves (FR-B6). The landing pane: a
    /// library of nine liked songs is a poor thing to open on.
    #[default]
    Home,
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
pub const PANE_ORDER: [Pane; 7] = [
    Pane::Home,
    Pane::Playlists,
    Pane::Songs,
    Pane::Albums,
    Pane::Artists,
    Pane::Search,
    Pane::Queue,
];

/// One line of the Home pane.
///
/// The feed is carousels, but a terminal list is one column, so the shelves are
/// flattened with their titles as headings. Headings are rows so they scroll
/// with the content; `select_next`/`select_prev` skip them, because landing on
/// one and pressing Enter would do nothing and read as a broken key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeRow {
    Heading(String),
    Item(HomeItem),
}

/// Pixels to request for album art. Comfortably over the ~240px a 24-column
/// panel needs, so the art stays sharp if the panel or the font grows.
pub const ART_PX: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Sidebar,
    Main,
    SearchInput,
    /// Typing into the live row filter (`/`). Kept apart from `SearchInput`
    /// because a filter never issues a request — it narrows what is already on
    /// screen — and because both fields can hold text at once.
    FilterInput,
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
    /// Rows the list area can show, set by the loop from the frame size.
    ///
    /// Paging and centring are meaningless without it, and a widget cannot tell
    /// the reducer — `render` takes `&AppState`. Zero until the first frame, so
    /// every use goes through `page_height`, which floors it.
    pub viewport_rows: usize,

    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub open_playlist: Option<PlaylistId>,

    /// The home feed's shelves, flattened to rows with their headings (FR-B6).
    pub home_rows: Vec<HomeRow>,

    /// The artist whose tracks are on screen, and their name for the heading
    /// (FR-B7). `None` means the Artists pane is showing the list of artists.
    ///
    /// The name is stored rather than looked up: an artist opened from search is
    /// not in `artists`, so there would be nothing to look up.
    pub open_artist: Option<(ArtistId, String)>,
    /// The open artist's tracks. Kept apart from `tracks` so opening an artist
    /// cannot clobber the library songs or an open playlist's rows — the class
    /// of bug that made `selected_track` report rows that were not on screen.
    pub artist_tracks: Vec<Track>,

    /// Live substring filter over the rows on screen (`/`), independent of the
    /// Search pane's server-side query. Empty means no filter.
    pub filter: String,

    /// True while the Artists pane's own search field is showing results.
    ///
    /// The library list only holds artists you follow, so there was no way to
    /// reach anyone else. `S` in that pane searches YouTube Music and puts the
    /// results in the same list; this flag is what the heading and the "back to
    /// your library" path key off.
    pub artist_search_active: bool,

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
            AppEvent::ArtistSearchResults { query, artists } => {
                // Ignore results for a query the user has already moved past —
                // the same rule the song search follows.
                if query == self.search_query {
                    self.artists = artists;
                    self.selected = 0;
                    self.scroll_offset = 0;
                    self.loading = false;
                }
            }
            AppEvent::HomeLoaded(shelves) => {
                self.set_home_shelves(shelves);
                self.loading = false;
            }
            AppEvent::ArtistTracksLoaded { id, name, tracks } => {
                self.open_artist = Some((id, name));
                self.artist_tracks = tracks;
                self.selected = 0;
                self.scroll_offset = 0;
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
                // Ctrl+W. A prompt appends at the end and draws no caret, so
                // "delete the previous word" is the whole of line editing that
                // is well defined here — the motions need a caret to move, and
                // are deliberately not claimed for prompts.
                (Modal::Prompt { value, .. }, InputAction::DeleteWordBack) => {
                    let at = crate::util::text::prev_word_boundary(value, value.len());
                    value.truncate(at);
                }
                // Submission is the loop's job: it owns the API calls.
                _ => {}
            }
            return;
        }
        // While the search field has focus, printable keys are text. The keymap
        // already resolves them to `Char`/`Backspace` rather than commands, so
        // this arm only has to edit the buffer.
        // The filter field: same editing keys as search, but it never issues a
        // request and Esc *clears* rather than just leaving — a filter the user
        // cannot see the end of is worse than no filter.
        if self.focus == Focus::FilterInput {
            match a {
                InputAction::Char(c) => {
                    self.filter.push(c);
                    self.selected = 0;
                    self.scroll_offset = 0;
                    return;
                }
                InputAction::Backspace => {
                    self.filter.pop();
                    self.selected = 0;
                    self.scroll_offset = 0;
                    return;
                }
                InputAction::DeleteWordBack => {
                    let at = crate::util::text::prev_word_boundary(&self.filter, self.filter.len());
                    self.filter.truncate(at);
                    self.selected = 0;
                    return;
                }
                // Esc and Enter both just leave the field, keeping the filter.
                //
                // Esc used to clear it, which is what made `a` unusable: the user
                // pressed Esc expecting to be back in the list, the field kept
                // focus in their mind but the filter was gone, and the next
                // letter went somewhere they did not expect. Leaving the field
                // with the filter intact means the rows under the cursor are the
                // ones they filtered for, and `a` acts on them. A second Esc,
                // handled below in navigation focus, clears the filter.
                InputAction::Cancel | InputAction::Confirm => {
                    self.focus = Focus::Main;
                    self.selected = 0;
                    return;
                }
                // Arrows walk the filtered rows without leaving the field.
                InputAction::Down => {
                    self.select_next();
                    return;
                }
                InputAction::Up => {
                    self.select_prev();
                    return;
                }
                _ => return,
            }
        }

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
            // Half-page (Ctrl+D / Ctrl+U) and full-page (PageDown / PageUp)
            // motion. Both resolved in the keymap from the beginning but were
            // never handled here, so every one of them silently did nothing.
            InputAction::PageDown | InputAction::PageUp => {
                let step = self.page_step();
                if a == InputAction::PageDown {
                    let n = self.list_len();
                    if n > 0 {
                        self.selected = (self.selected + step).min(n - 1);
                    }
                } else {
                    self.selected = self.selected.saturating_sub(step);
                }
                self.skip_headings(a == InputAction::PageDown);
                self.refresh_visual_marks();
            }
            // The wheel scrolls the view and only drags the cursor when the view
            // would otherwise leave it behind — how a list behaves everywhere
            // else. Moving the cursor a row per notch instead would fight the
            // selection.
            InputAction::ScrollDown | InputAction::ScrollUp => {
                self.scroll_by(a == InputAction::ScrollDown);
                self.refresh_visual_marks();
            }
            // `zz`, from vim: centre the selected row.
            InputAction::CenterOnCursor => self.center_on_cursor(),
            // Tab / Shift+Tab cycle sources. These resolved in the keymap from
            // the very first version but no reducer ever handled them, so both
            // keys silently did nothing — the owner reported Tab as useless.
            InputAction::NextPane | InputAction::PrevPane => {
                let n = PANE_ORDER.len();
                let cur = PANE_ORDER.iter().position(|p| *p == self.pane).unwrap_or(0);
                let next = if a == InputAction::NextPane {
                    (cur + 1) % n
                } else {
                    (cur + n - 1) % n
                };
                // Reuse the number-key path so Tab and `1`-`7` cannot disagree
                // about where a source lands you.
                self.goto_source(next as u8 + 1);
            }
            InputAction::ToggleVisual => self.toggle_visual(),
            InputAction::OpenHelp => self.modal = Some(Modal::Help),
            InputAction::OpenSearch => {
                // In the Artists pane, `S` searches *artists* and keeps the
                // results in that pane — the library list holds only the artists
                // you follow, so without this there is no way to reach any other.
                if self.pane == Pane::Artists {
                    self.close_open_artist();
                    self.artist_search_active = true;
                    self.focus = Focus::SearchInput;
                    self.search_cursor = self.search_query.len();
                    return;
                }
                self.set_pane(Pane::Search);
                self.focus = Focus::SearchInput;
                // Reopening lands the caret after whatever query is still there.
                self.search_cursor = self.search_query.len();
            }
            // `/` filters the rows already on screen. Deliberately not a
            // request: the Search pane (`S`) is what asks YouTube.
            InputAction::OpenFilter => {
                if self.pane != Pane::Search {
                    self.focus = Focus::FilterInput;
                }
            }
            InputAction::OpenQueue => self.set_pane(Pane::Queue),
            // Esc with a filter up clears it and restores the full list. Checked
            // before visual mode so the more visible state wins: a filtered list
            // is on screen and labelled, so Esc reads as "get rid of that".
            InputAction::Cancel if self.is_filtering() => {
                // Keep the cursor on the row the user was looking at. Resetting to
                // 0 meant Esc-then-Enter played the first track in the pane rather
                // than the one they had picked out of the filtered list.
                let keep = self.unfiltered_index_of_selected();
                self.filter.clear();
                self.selected = keep;
                self.scroll_offset = 0;
            }
            // Esc means "undo this selection" while a range is being made.
            // Guarded so it only claims the key during visual mode; outside it,
            // Esc keeps whatever meaning it had.
            InputAction::Cancel if self.visual_anchor.is_some() => self.cancel_visual(),
            // `h` is "go up a level" first and "focus the sidebar" second, so
            // the pair reads like opening and closing a folder. Only the
            // playlist pane has a level to leave; everywhere else `h` keeps its
            // old meaning rather than swallowing the key.
            InputAction::Left => {
                // An open playlist or an open artist is a level to leave. Without
                // the artist branch `h` focused the sidebar while the artist's
                // tracks stayed on screen, so there was no way back to the list.
                if self.close_open_playlist()
                    || self.close_open_artist()
                    || self.close_artist_search()
                {
                    // Stay in the list: the user is navigating it, not leaving it.
                } else {
                    self.focus = Focus::Sidebar;
                }
            }
            // `l` descends where there is somewhere to go. On an artist row that
            // is their track list, which mirrors `l` on a playlist row.
            InputAction::Right => {
                if self.focus == Focus::Main && self.selected_artist().is_some() {
                    // The loop turns this into the fetch; state cannot call the
                    // API. Focus stays put so the rows replace the list in place.
                } else {
                    self.focus = Focus::Main;
                }
            }
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
            Pane::Home => self.home_rows.len(),
            Pane::Playlists if self.open_playlist.is_some() => self.visible_track_count(),
            Pane::Playlists => self.visible_playlist_count(),
            Pane::Songs => self.visible_track_count(),
            Pane::Albums => self.visible_album_count(),
            // An open artist shows their tracks; otherwise the list of artists.
            Pane::Artists if self.open_artist.is_some() => self.visible_track_count(),
            Pane::Artists => self.visible_artist_count(),
            // Search results are never filtered locally: the query already did it.
            Pane::Search => self.search_results.len(),
            Pane::Queue => self.visible_track_count(),
        }
    }

    /// True when a filter is narrowing the rows on screen.
    pub fn is_filtering(&self) -> bool {
        !self.filter.is_empty()
    }

    /// Whether the filter row is on screen.
    ///
    /// Shown while typing *and* while a filter is still narrowing rows after the
    /// field lost focus — otherwise a filtered list looks like a short list, with
    /// nothing to say why rows are missing or how to get them back.
    /// The art URL for what is playing, at a size that can fill the panel.
    ///
    /// One accessor because the fetch and the cache lookup must agree: keyed on
    /// different URLs, the image would be downloaded and then never found.
    ///
    /// The upsize is the whole point. YouTube volunteers 120px thumbnails and
    /// `ratatui-image`'s `Resize::Fit` never upscales, so the art filled only
    /// half the 24-column panel however wide the panel was — the empty right-hand
    /// strip the owner reported. Verified live: the same URL at `=w600-h600`
    /// returns a real 600x600 JPEG.
    pub fn art_url(&self) -> Option<String> {
        Some(ytm_core::mapping::thumbnail_at_size(
            self.now_playing.as_ref()?.thumbnail_url.as_deref()?,
            ART_PX,
        ))
    }

    /// Queue index of a visible row, or `None` when this is not the queue.
    ///
    /// `selected` counts *visible* rows; `JumpTo`, `RemoveFromQueue` and
    /// `MoveInQueue` all take real queue indices. Unfiltered the two are equal,
    /// which is why passing `selected` straight through worked until a filter was
    /// on — then Enter played whatever sat at that position in the full queue.
    ///
    /// Positional rather than by video id, because the queue may legitimately hold
    /// the same track twice and an id lookup would pick the first copy.
    pub fn queue_index_of_row(&self, row: usize) -> Option<usize> {
        if self.pane != Pane::Queue {
            return None;
        }
        if !self.is_filtering() {
            return (row < self.queue.len()).then_some(row);
        }
        self.queue
            .iter()
            .enumerate()
            .filter(|(_, t)| self.track_matches_filter(t))
            .map(|(i, _)| i)
            .nth(row)
    }

    /// Queue index of the row under the cursor.
    pub fn queue_index_of_selected(&self) -> Option<usize> {
        self.queue_index_of_row(self.selected)
    }

    /// The row `selected` would have if the filter were cleared.
    fn unfiltered_index_of_selected(&self) -> usize {
        if !self.is_filtering() {
            return self.selected;
        }
        let nth = self.selected;
        let found = match self.pane {
            // Home is never filtered, so the row index already is the real one.
            Pane::Home => Some(nth),
            Pane::Playlists if self.open_playlist.is_none() => self
                .playlists
                .iter()
                .enumerate()
                .filter(|(_, p)| self.matches_filter(&p.title))
                .map(|(i, _)| i)
                .nth(nth),
            Pane::Albums => self
                .albums
                .iter()
                .enumerate()
                .filter(|(_, a)| self.album_matches(a))
                .map(|(i, _)| i)
                .nth(nth),
            Pane::Artists if self.open_artist.is_none() => self
                .artists
                .iter()
                .enumerate()
                .filter(|(_, a)| self.matches_filter(&a.name))
                .map(|(i, _)| i)
                .nth(nth),
            // Every track pane, including an open playlist or artist.
            _ => {
                let src = self.unfiltered_tracks();
                src.iter()
                    .enumerate()
                    .filter(|(_, t)| self.track_matches_filter(t))
                    .map(|(i, _)| i)
                    .nth(nth)
            }
        };
        // Nothing matched (a filter with no hits, cleared while empty): the top is
        // the only index certain to be in range.
        found.unwrap_or(0)
    }

    pub fn filter_row_visible(&self) -> bool {
        self.focus == Focus::FilterInput || self.is_filtering()
    }

    /// Whether a query row is drawn above the list.
    ///
    /// The Artists pane borrows the same field for `S` (FR-B7), so it grows the
    /// row too. One method rather than a condition repeated in the layout, the
    /// click math, and the viewport count: when those disagreed, `S` in Artists
    /// focused a field that was never drawn, so every key went into an invisible
    /// query and the pane looked frozen.
    pub fn search_row_visible(&self) -> bool {
        match self.pane {
            Pane::Search => true,
            // Not once an artist is open: that is a track list, and the row
            // would claim a search is still on screen to leave.
            Pane::Artists => self.artist_search_active && self.open_artist.is_none(),
            _ => false,
        }
    }

    /// Does this text survive the filter? Case-insensitive substring.
    fn matches_filter(&self, text: &str) -> bool {
        self.filter.is_empty() || text.to_lowercase().contains(&self.filter.to_lowercase())
    }

    /// How many track rows survive the filter, without building the list.
    ///
    /// `list_len` is called several times per frame and on every cursor move, so
    /// going through `visible_tracks` cloned a 400-track playlist each time for
    /// nothing but its length. Counting shares `track_matches_filter` with the
    /// list itself, so the two cannot disagree about which rows are visible.
    fn visible_track_count(&self) -> usize {
        let source = self.unfiltered_tracks();
        if self.filter.is_empty() {
            return source.len();
        }
        source
            .iter()
            .filter(|t| self.track_matches_filter(t))
            .count()
    }

    /// Does this track survive the filter? Title, artist, or album.
    fn track_matches_filter(&self, t: &Track) -> bool {
        self.matches_filter(&t.title)
            || t.artists.iter().any(|a| self.matches_filter(a))
            || t.album.as_deref().is_some_and(|a| self.matches_filter(a))
    }

    /// The track rows the filter leaves on screen.
    ///
    /// Owned rather than borrowed: a filtered view is a new list, and returning
    /// a slice would mean either storing the filtered copy or lying about
    /// lifetimes. Callers that need indices use this, so what is on screen and
    /// what `Enter` acts on cannot disagree.
    pub fn visible_tracks(&self) -> Vec<Track> {
        let source = self.unfiltered_tracks();
        if self.filter.is_empty() {
            return source.to_vec();
        }
        source
            .iter()
            .filter(|t| self.track_matches_filter(t))
            .cloned()
            .collect()
    }

    /// Which track list this pane draws from, before filtering.
    fn unfiltered_tracks(&self) -> &[Track] {
        match self.pane {
            Pane::Artists if self.open_artist.is_some() => &self.artist_tracks,
            Pane::Search => &self.search_results,
            Pane::Queue => &self.queue,
            _ => &self.tracks,
        }
    }

    fn visible_playlist_count(&self) -> usize {
        if self.filter.is_empty() {
            return self.playlists.len();
        }
        self.playlists
            .iter()
            .filter(|p| self.matches_filter(&p.title))
            .count()
    }

    fn visible_album_count(&self) -> usize {
        if self.filter.is_empty() {
            return self.albums.len();
        }
        self.albums.iter().filter(|a| self.album_matches(a)).count()
    }

    fn visible_artist_count(&self) -> usize {
        if self.filter.is_empty() {
            return self.artists.len();
        }
        self.artists
            .iter()
            .filter(|a| self.matches_filter(&a.name))
            .count()
    }

    fn album_matches(&self, a: &Album) -> bool {
        self.matches_filter(&a.title) || a.artists.iter().any(|x| self.matches_filter(x))
    }

    pub fn visible_playlists(&self) -> Vec<Playlist> {
        self.playlists
            .iter()
            .filter(|p| self.matches_filter(&p.title))
            .cloned()
            .collect()
    }

    pub fn visible_albums(&self) -> Vec<Album> {
        self.albums
            .iter()
            .filter(|a| self.album_matches(a))
            .cloned()
            .collect()
    }

    pub fn visible_artists(&self) -> Vec<Artist> {
        self.artists
            .iter()
            .filter(|a| self.matches_filter(&a.name))
            .cloned()
            .collect()
    }

    /// The artist under the cursor, when the Artists pane is showing the list.
    pub fn selected_artist(&self) -> Option<Artist> {
        (self.pane == Pane::Artists && self.open_artist.is_none())
            .then(|| self.visible_artists().get(self.selected).cloned())
            .flatten()
    }

    /// The home row under the cursor.
    pub fn selected_home_item(&self) -> Option<&HomeItem> {
        match self.home_rows.get(self.selected)? {
            HomeRow::Item(i) => Some(i),
            HomeRow::Heading(_) => None,
        }
    }

    /// Leave an open artist and show the artist list again.
    ///
    /// Clears the tracks with the id for the same reason `close_open_playlist`
    /// does: rows left behind would make `list_len` and `selected_track`
    /// disagree with the screen.
    /// Leave artist search, so the next load restores the followed artists.
    ///
    /// Returns false when no search was active, letting `h` fall through to its
    /// other meanings rather than swallowing the key.
    pub fn close_artist_search(&mut self) -> bool {
        if !self.artist_search_active {
            return false;
        }
        self.artist_search_active = false;
        self.artists.clear();
        self.search_query.clear();
        self.search_cursor = 0;
        self.selected = 0;
        self.scroll_offset = 0;
        self.focus = Focus::Main;
        true
    }

    pub fn close_open_artist(&mut self) -> bool {
        if self.pane != Pane::Artists || self.open_artist.is_none() {
            return false;
        }
        self.open_artist = None;
        self.artist_tracks.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.marked.clear();
        self.visual_anchor = None;
        self.marks_before_visual.clear();
        true
    }

    /// Replace the feed's shelves, flattened into rows with their headings.
    pub fn set_home_shelves(&mut self, shelves: Vec<HomeShelf>) {
        self.home_rows = shelves
            .into_iter()
            .flat_map(|s| {
                std::iter::once(HomeRow::Heading(s.title))
                    .chain(s.items.into_iter().map(HomeRow::Item))
            })
            .collect();
        // A heading under the cursor has no Enter behaviour, so start on a real
        // row.
        self.selected = self.next_selectable(0, 1).unwrap_or(0);
    }

    /// The next row at or after `from` that is not a heading, walking `step`.
    fn next_selectable(&self, from: usize, step: isize) -> Option<usize> {
        if self.pane != Pane::Home {
            return Some(from);
        }
        let n = self.home_rows.len();
        if n == 0 {
            return None;
        }
        let mut i = from as isize;
        while i >= 0 && (i as usize) < n {
            if matches!(self.home_rows.get(i as usize), Some(HomeRow::Item(_))) {
                return Some(i as usize);
            }
            i += step;
        }
        None
    }

    /// Rows on screen, floored so a list stays navigable before the first frame
    /// has reported a real height.
    fn page_height(&self) -> usize {
        self.viewport_rows.max(1)
    }

    /// How far a page key moves: half a screen, like vim's Ctrl+D.
    fn page_step(&self) -> usize {
        (self.page_height() / 2).max(1)
    }

    /// Centre the selected row in the viewport (`zz`).
    ///
    /// Clamped at both ends: near the top or bottom there is nothing to scroll
    /// into view, and forcing it would pad the list with blank rows.
    pub fn center_on_cursor(&mut self) {
        let height = self.page_height();
        let max_start = self.list_len().saturating_sub(height);
        self.scroll_offset = self.selected.saturating_sub(height / 2).min(max_start);
    }

    /// Scroll the view a wheel notch, keeping the cursor inside it.
    fn scroll_by(&mut self, down: bool) {
        let n = self.list_len();
        if n == 0 {
            return;
        }
        let height = self.page_height();
        let max_start = n.saturating_sub(height);
        self.scroll_offset = if down {
            (self.scroll_offset + WHEEL_ROWS).min(max_start)
        } else {
            self.scroll_offset.saturating_sub(WHEEL_ROWS)
        };
        // Keep the cursor on a visible row rather than letting it drift off the
        // top or bottom of the window.
        let last_visible = (self.scroll_offset + height - 1).min(n - 1);
        self.selected = self.selected.clamp(self.scroll_offset, last_visible);
        self.skip_headings(true);
    }

    /// Step off a heading onto a real row (Home pane only).
    ///
    /// A heading has no `Enter` behaviour, so a cursor parked on one makes the
    /// next key look broken. Reverses direction at the ends of the list.
    fn skip_headings(&mut self, forward: bool) {
        if self.pane != Pane::Home {
            return;
        }
        let step = if forward { 1 } else { -1 };
        if let Some(i) = self.next_selectable(self.selected, step) {
            self.selected = i;
        } else if let Some(i) = self.next_selectable(self.selected, -step) {
            self.selected = i;
        }
    }

    pub fn select_next(&mut self) {
        let n = self.list_len();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
            self.skip_headings(true);
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.skip_headings(false);
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
        // The Artists pane borrows the Search pane's query buffer for `S`, so
        // leaving it has to put that buffer back. Without this, typing an artist
        // name here and then opening Search showed the name already in the field,
        // as if the user had searched for it there — and `artist_search_active`
        // stayed set, so the next search fired at `search_artists`.
        if self.pane == Pane::Artists && p != Pane::Artists {
            self.close_artist_search();
        }
        self.pane = p;
        self.selected = 0;
        // The filter narrows one pane's rows. Carrying it across would hide
        // rows in the new pane for a reason the user cannot see.
        self.filter.clear();
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
    pub fn track_rows(&self) -> Vec<Track> {
        match self.pane {
            // The home feed's playable cards, in the order they are drawn, so a
            // visual range or a bulk add matches what is on screen.
            Pane::Home => self
                .home_rows
                .iter()
                .filter_map(|r| match r {
                    HomeRow::Item(i) => match &i.target {
                        HomeTarget::Track(v) => Some(Track {
                            video_id: v.clone(),
                            set_video_id: None,
                            title: i.title.clone(),
                            artists: if i.subtitle.is_empty() {
                                Vec::new()
                            } else {
                                vec![i.subtitle.clone()]
                            },
                            album: None,
                            duration: TrackDuration::from_secs(0),
                            thumbnail_url: i.thumbnail_url.clone(),
                            is_explicit: false,
                        }),
                        _ => None,
                    },
                    HomeRow::Heading(_) => None,
                })
                .collect(),
            Pane::Search | Pane::Queue | Pane::Songs => self.visible_tracks(),
            Pane::Playlists if self.open_playlist.is_some() => self.visible_tracks(),
            Pane::Artists if self.open_artist.is_some() => self.visible_tracks(),
            Pane::Playlists | Pane::Albums | Pane::Artists => Vec::new(),
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
    pub fn selected_track(&self) -> Option<Track> {
        match self.pane {
            // A heading is not a track, and a card that opens a page is not one
            // either — Enter on those does something else entirely.
            Pane::Home => match &self.selected_home_item()?.target {
                HomeTarget::Track(_) => self.home_track_at(self.selected),
                _ => None,
            },
            Pane::Search | Pane::Queue | Pane::Songs => {
                self.visible_tracks().get(self.selected).cloned()
            }
            // Only an open playlist shows tracks; the list of playlists does not.
            Pane::Playlists => self
                .open_playlist
                .is_some()
                .then(|| self.visible_tracks().get(self.selected).cloned())
                .flatten(),
            // Likewise an open artist.
            Pane::Artists => self
                .open_artist
                .is_some()
                .then(|| self.visible_tracks().get(self.selected).cloned())
                .flatten(),
            Pane::Albums => None,
        }
    }

    /// The home row at `idx` as a playable track, if it is one.
    fn home_track_at(&self, idx: usize) -> Option<Track> {
        let HomeRow::Item(i) = self.home_rows.get(idx)? else {
            return None;
        };
        let HomeTarget::Track(v) = &i.target else {
            return None;
        };
        Some(Track {
            video_id: v.clone(),
            set_video_id: None,
            title: i.title.clone(),
            artists: if i.subtitle.is_empty() {
                Vec::new()
            } else {
                vec![i.subtitle.clone()]
            },
            album: None,
            duration: TrackDuration::from_secs(0),
            thumbnail_url: i.thumbnail_url.clone(),
            is_explicit: false,
        })
    }

    pub fn selected_playlist(&self) -> Option<Playlist> {
        (self.pane == Pane::Playlists && self.open_playlist.is_none())
            .then(|| self.visible_playlists().get(self.selected).cloned())
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
        // Home is the landing pane: a library of a few liked songs is a poor
        // thing to open on.
        assert_eq!(s.pane, Pane::Home);
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

    fn filtered_queue() -> AppState {
        AppState {
            pane: Pane::Queue,
            focus: Focus::Main,
            queue: vec![
                Track::stub("q0", "We Don't Talk Anymore"),
                Track::stub("q1", "blue"),
                Track::stub("q2", "Something Else"),
                Track::stub("q3", "I'm Good (Blue)"),
            ],
            filter: "blu".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_filtered_row_maps_to_its_real_queue_index() {
        // The owner's bug: "blu" leaves rows 1 and 3 on screen, so visible row 0
        // is queue 1 and visible row 1 is queue 3. Passing `selected` through
        // played queue[0] and queue[1] instead.
        let s = filtered_queue();
        assert_eq!(s.queue_index_of_row(0), Some(1));
        assert_eq!(s.queue_index_of_row(1), Some(3));
    }

    #[test]
    fn an_unfiltered_row_is_its_own_queue_index() {
        let s = AppState {
            filter: String::new(),
            ..filtered_queue()
        };
        assert_eq!(s.queue_index_of_row(2), Some(2));
    }

    #[test]
    fn a_row_past_the_end_maps_to_nothing() {
        // Better no command than one the actor would index past the queue with.
        assert_eq!(filtered_queue().queue_index_of_row(2), None);
        assert_eq!(
            AppState {
                filter: String::new(),
                ..filtered_queue()
            }
            .queue_index_of_row(9),
            None
        );
    }

    #[test]
    fn duplicate_tracks_map_by_position_not_by_id() {
        // The queue can hold the same video twice. An id lookup would send both
        // rows to the first copy.
        let s = AppState {
            queue: vec![
                Track::stub("dup", "blue"),
                Track::stub("x", "nope"),
                Track::stub("dup", "blue"),
            ],
            ..filtered_queue()
        };
        assert_eq!(s.queue_index_of_row(0), Some(0));
        assert_eq!(s.queue_index_of_row(1), Some(2));
    }

    #[test]
    fn the_queue_mapping_is_only_for_the_queue() {
        let s = AppState {
            pane: Pane::Songs,
            ..filtered_queue()
        };
        assert_eq!(s.queue_index_of_row(0), None);
    }

    #[test]
    fn clearing_the_filter_keeps_the_cursor_on_the_same_track() {
        // The other half of the report: Esc then Enter played the queue's first
        // song, because clearing the filter reset the cursor to row 0.
        let mut s = filtered_queue();
        s.selected = 1; // "I'm Good (Blue)", which is queue 3
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert!(!s.is_filtering(), "Esc still clears the filter");
        assert_eq!(s.selected, 3, "the cursor follows the track it was on");
        assert_eq!(
            s.selected_track().map(|t| t.title),
            Some("I'm Good (Blue)".to_owned())
        );
    }

    #[test]
    fn clearing_a_filter_that_matched_nothing_does_not_dangle() {
        let mut s = AppState {
            filter: "zzzz".into(),
            ..filtered_queue()
        };
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert!(s.selected < s.queue.len());
    }

    #[test]
    fn clearing_the_filter_keeps_the_cursor_in_a_list_pane_too() {
        // Same fix, and the same reason: the row under the cursor is what the
        // user was aiming at, whichever pane they are in.
        let mut s = AppState {
            pane: Pane::Playlists,
            focus: Focus::Main,
            playlists: vec![
                Playlist::stub("p0", "Alpha"),
                Playlist::stub("p1", "Focus mix"),
                Playlist::stub("p2", "Beta"),
            ],
            filter: "focus".into(),
            selected: 0,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert_eq!(s.selected, 1, "Focus mix is row 1 unfiltered");
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
        // 1-7 match the sidebar order top to bottom.
        let mut s = AppState::default();
        s.apply_input(InputAction::GoTo(4));
        assert_eq!(s.pane, Pane::Albums);
        assert_eq!(s.sidebar_selected, 3, "the sidebar highlight follows");
        assert_eq!(s.focus, Focus::Main, "a source jump lands in the list");
    }

    #[test]
    fn number_keys_cover_every_source_in_sidebar_order() {
        for (n, want) in [
            (1, Pane::Home),
            (2, Pane::Playlists),
            (3, Pane::Songs),
            (4, Pane::Albums),
            (5, Pane::Artists),
            (6, Pane::Search),
            (7, Pane::Queue),
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
        // Otherwise `6` lands you in a search pane where letters scroll.
        let mut s = AppState::default();
        s.apply_input(InputAction::GoTo(6));
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

    #[test]
    fn typing_and_editing_a_query_with_real_key_presses() {
        // The full path: crossterm KeyEvent -> keymap -> reducer -> the text.
        // Every earlier test stopped at one of those seams, which is how the
        // chords shipped unreachable.
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let km = crate::keymap::KeyMap::default();
        let mut s = AppState::default();

        let send = |s: &mut AppState, code: KeyCode, ctrl: bool| {
            let m = if ctrl {
                KeyModifiers::CONTROL
            } else {
                KeyModifiers::NONE
            };
            if let Some(a) = km.resolve(KeyEvent::new(code, m), s.input_focus()) {
                s.apply(AppEvent::Input(a));
            }
        };

        // `S` opens the search pane, then type a query. (`/` filters the list
        // in place now, at the owner's request.)
        send(&mut s, KeyCode::Char('S'), false);
        assert_eq!(s.focus, Focus::SearchInput);
        for c in "boards of canada".chars() {
            send(&mut s, KeyCode::Char(c), false);
        }
        assert_eq!(s.search_query, "boards of canada");

        // Ctrl+W drops the last word.
        send(&mut s, KeyCode::Char('w'), true);
        assert_eq!(s.search_query, "boards of ");

        // Ctrl+Left twice, then type at the caret.
        send(&mut s, KeyCode::Left, true);
        send(&mut s, KeyCode::Left, true);
        assert_eq!(s.search_cursor, 0);
        for c in "the ".chars() {
            send(&mut s, KeyCode::Char(c), false);
        }
        assert_eq!(s.search_query, "the boards of ");

        // Ctrl+E to the end, Ctrl+A back to the start.
        send(&mut s, KeyCode::Char('e'), true);
        assert_eq!(s.search_cursor, s.search_query.len());
        send(&mut s, KeyCode::Char('a'), true);
        assert_eq!(s.search_cursor, 0);
    }

    #[test]
    fn ctrl_w_deletes_a_word_in_a_playlist_name_prompt() {
        // A prompt reports SearchInput focus, so the chords resolve there. If the
        // reducer ignores them, Ctrl+W silently does nothing while naming a
        // playlist — the field would accept text but not editing. Only Ctrl+W:
        // a prompt has no caret, so the motions have nothing to move.
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let km = crate::keymap::KeyMap::default();
        let mut s = AppState {
            modal: Some(Modal::Prompt {
                title: "New playlist".into(),
                value: String::new(),
                action: PromptAction::CreatePlaylist,
            }),
            ..Default::default()
        };
        let send = |s: &mut AppState, code: KeyCode, ctrl: bool| {
            let m = if ctrl {
                KeyModifiers::CONTROL
            } else {
                KeyModifiers::NONE
            };
            if let Some(a) = km.resolve(KeyEvent::new(code, m), s.input_focus()) {
                s.apply(AppEvent::Input(a));
            }
        };
        for c in "late night".chars() {
            send(&mut s, KeyCode::Char(c), false);
        }
        send(&mut s, KeyCode::Char('w'), true);
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => {
                assert_eq!(value, "late ", "Ctrl+W must edit a prompt as well")
            }
            other => panic!("expected the prompt to survive, got {other:?}"),
        }
    }

    /// Rows for a Home pane: one shelf heading, then cards.
    fn home_state() -> AppState {
        let mut s = AppState {
            pane: Pane::Home,
            focus: Focus::Main,
            viewport_rows: 10,
            ..Default::default()
        };
        s.set_home_shelves(vec![
            HomeShelf {
                title: "Quick picks".into(),
                items: vec![
                    HomeItem {
                        title: "One".into(),
                        subtitle: "Artist".into(),
                        target: HomeTarget::Track(VideoId::from("v1")),
                        thumbnail_url: None,
                    },
                    HomeItem {
                        title: "Two".into(),
                        subtitle: "Artist".into(),
                        target: HomeTarget::Track(VideoId::from("v2")),
                        thumbnail_url: None,
                    },
                ],
            },
            HomeShelf {
                title: "Albums for you".into(),
                items: vec![HomeItem {
                    title: "An album".into(),
                    subtitle: "Someone".into(),
                    target: HomeTarget::Album(AlbumId::from("MPREb_1")),
                    thumbnail_url: None,
                }],
            },
        ]);
        s
    }

    #[test]
    fn the_home_pane_starts_on_a_card_not_a_heading() {
        // Row 0 is "Quick picks". Landing there would make Enter do nothing,
        // which reads as a broken key rather than as an unselectable row.
        let s = home_state();
        assert_eq!(s.selected, 1);
        assert!(s.selected_home_item().is_some());
    }

    #[test]
    fn moving_through_the_home_pane_steps_over_headings() {
        let mut s = home_state();
        // rows: 0 heading, 1 One, 2 Two, 3 heading, 4 An album
        s.apply_input(InputAction::Down);
        assert_eq!(s.selected, 2);
        // The next row is a heading, so the cursor must land past it.
        s.apply_input(InputAction::Down);
        assert_eq!(s.selected, 4, "a heading must not hold the cursor");
        assert!(s.selected_home_item().is_some());
    }

    #[test]
    fn going_back_up_also_steps_over_a_heading() {
        let mut s = home_state();
        s.selected = 4;
        s.apply_input(InputAction::Up);
        assert_eq!(s.selected, 2, "row 3 is a heading");
    }

    #[test]
    fn a_home_track_row_is_playable_but_an_album_row_is_not() {
        // One carousel mixes kinds, so this is per row, not per pane. Enter on
        // the album row must not play whatever track was last in view.
        let mut s = home_state();
        s.selected = 1;
        assert_eq!(
            s.selected_track().map(|t| t.video_id),
            Some(VideoId::from("v1"))
        );
        s.selected = 4;
        assert_eq!(s.selected_track(), None, "an album row is not a track");
    }

    #[test]
    fn the_filter_narrows_the_rows_on_screen() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: vec![
                Track::stub("v1", "Blue Monday"),
                Track::stub("v2", "Ceremony"),
                Track::stub("v3", "Blue Jeans"),
            ],
            ..Default::default()
        };
        s.apply_input(InputAction::OpenFilter);
        assert_eq!(s.focus, Focus::FilterInput);
        for c in "blue".chars() {
            s.apply_input(InputAction::Char(c));
        }
        assert_eq!(s.list_len(), 2, "only the two Blue rows survive");
        // And what Enter acts on must be a row that is actually on screen.
        assert_eq!(
            s.selected_track().map(|t| t.title),
            Some("Blue Monday".to_owned())
        );
    }

    #[test]
    fn escaping_the_filter_field_keeps_the_filter_and_a_second_esc_clears_it() {
        // Esc used to clear the filter the moment it left the field, which is
        // what made `a` unusable: the user pressed Esc to get back to the rows
        // they had filtered for, the rows changed under them, and the next letter
        // went somewhere unexpected. One Esc leaves the field; a second clears.
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: vec![Track::stub("v1", "Keep"), Track::stub("v2", "Drop")],
            ..Default::default()
        };
        s.apply_input(InputAction::OpenFilter);
        s.apply_input(InputAction::Char('k'));
        assert_eq!(s.list_len(), 1);

        s.apply_input(InputAction::Cancel);
        assert_eq!(s.focus, Focus::Main, "the field is left");
        assert!(s.is_filtering(), "but the filter survives");
        assert_eq!(s.list_len(), 1);
        // The row under the cursor is one the user filtered for, so `a` acts on
        // it — that is the whole point of not clearing here.
        assert_eq!(s.selected_track().map(|t| t.title), Some("Keep".to_owned()));

        s.apply_input(InputAction::Cancel);
        assert!(!s.is_filtering(), "a second Esc clears it");
        assert_eq!(s.list_len(), 2);
    }

    #[test]
    fn the_filter_row_is_visible_while_a_filter_is_narrowing_rows() {
        // Otherwise a filtered list just looks like a short list, with nothing
        // to say why rows are missing. The owner reported this as "the filter
        // text not show".
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: vec![Track::stub("v1", "Keep"), Track::stub("v2", "Drop")],
            ..Default::default()
        };
        assert!(!s.filter_row_visible(), "nothing to show yet");
        s.apply_input(InputAction::OpenFilter);
        assert!(s.filter_row_visible(), "visible while typing");
        s.apply_input(InputAction::Char('k'));
        s.apply_input(InputAction::Cancel);
        assert!(
            s.filter_row_visible(),
            "still visible once the field is left, because rows are still hidden"
        );
        s.apply_input(InputAction::Cancel);
        assert!(!s.filter_row_visible(), "gone once the filter is cleared");
    }

    #[test]
    fn tab_and_shift_tab_cycle_through_the_sources() {
        // Both resolved in the keymap from the first version but no reducer
        // handled either, so the keys did nothing at all.
        let mut s = AppState::default();
        assert_eq!(s.pane, Pane::Home);
        s.apply_input(InputAction::NextPane);
        assert_eq!(s.pane, Pane::Playlists);
        s.apply_input(InputAction::NextPane);
        assert_eq!(s.pane, Pane::Songs);
        s.apply_input(InputAction::PrevPane);
        assert_eq!(s.pane, Pane::Playlists);
    }

    #[test]
    fn tab_wraps_at_both_ends_rather_than_stopping() {
        let mut s = AppState::default();
        // Backwards from the first source reaches the last.
        s.apply_input(InputAction::PrevPane);
        assert_eq!(s.pane, Pane::Queue);
        // And forwards from the last comes back to the first.
        s.apply_input(InputAction::NextPane);
        assert_eq!(s.pane, Pane::Home);
    }

    #[test]
    fn tab_moves_the_sidebar_highlight_with_the_pane() {
        // Tab reuses `goto_source`, so the highlight cannot drift from the pane
        // the way it would with a second, separate code path.
        let mut s = AppState::default();
        s.apply_input(InputAction::NextPane);
        assert_eq!(s.pane, Pane::Playlists);
        assert_eq!(s.sidebar_selected, 1);
    }

    #[test]
    fn enter_keeps_the_filter_and_leaves_the_field() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: vec![Track::stub("v1", "Keep"), Track::stub("v2", "Drop")],
            ..Default::default()
        };
        s.apply_input(InputAction::OpenFilter);
        s.apply_input(InputAction::Char('k'));
        s.apply_input(InputAction::Confirm);
        assert_eq!(s.focus, Focus::Main);
        assert!(s.is_filtering(), "Enter keeps the filter");
        assert_eq!(s.list_len(), 1);
    }

    #[test]
    fn changing_pane_drops_the_filter() {
        // Otherwise the new pane hides rows for a reason the user cannot see.
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            tracks: vec![Track::stub("v1", "Keep")],
            ..Default::default()
        };
        s.apply_input(InputAction::OpenFilter);
        s.apply_input(InputAction::Char('z'));
        s.set_pane(Pane::Playlists);
        assert!(!s.is_filtering());
    }

    #[test]
    fn zz_centres_the_selected_row() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 10,
            tracks: (0..50)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            selected: 30,
            ..Default::default()
        };
        s.apply_input(InputAction::CenterOnCursor);
        // Row 30 with a 10-row window sits 5 from the top.
        assert_eq!(s.scroll_offset, 25);
    }

    #[test]
    fn centring_near_the_top_does_not_scroll_past_it() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 10,
            tracks: (0..50)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            selected: 2,
            ..Default::default()
        };
        s.apply_input(InputAction::CenterOnCursor);
        assert_eq!(s.scroll_offset, 0, "there is nothing above row 0");
    }

    #[test]
    fn centring_near_the_bottom_keeps_the_window_full() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 10,
            tracks: (0..20)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            selected: 19,
            ..Default::default()
        };
        s.apply_input(InputAction::CenterOnCursor);
        // 20 rows, 10 visible: the furthest the window can start is 10, or the
        // list would render blank rows below the last track.
        assert_eq!(s.scroll_offset, 10);
    }

    #[test]
    fn half_page_keys_move_half_a_screen() {
        // Ctrl+D/Ctrl+U resolved from the very first keymap but nothing handled
        // them, so both keys silently did nothing.
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 20,
            tracks: (0..100)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            ..Default::default()
        };
        s.apply_input(InputAction::PageDown);
        assert_eq!(s.selected, 10);
        s.apply_input(InputAction::PageDown);
        assert_eq!(s.selected, 20);
        s.apply_input(InputAction::PageUp);
        assert_eq!(s.selected, 10);
    }

    #[test]
    fn paging_stops_at_the_ends_rather_than_wrapping() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 20,
            tracks: (0..5).map(|i| Track::stub(&format!("v{i}"), "T")).collect(),
            ..Default::default()
        };
        s.apply_input(InputAction::PageDown);
        assert_eq!(s.selected, 4, "clamped to the last row");
        s.apply_input(InputAction::PageUp);
        assert_eq!(s.selected, 0);
        s.apply_input(InputAction::PageUp);
        assert_eq!(s.selected, 0, "no wrap to the bottom");
    }

    #[test]
    fn the_wheel_scrolls_the_view_without_dragging_the_cursor() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 10,
            tracks: (0..50)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            selected: 8,
            ..Default::default()
        };
        s.apply_input(InputAction::ScrollDown);
        assert_eq!(s.scroll_offset, 3);
        // Row 8 is still inside rows 3..13, so the cursor stays put.
        assert_eq!(s.selected, 8);
    }

    #[test]
    fn the_wheel_pulls_the_cursor_along_once_it_would_leave_the_view() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 5,
            tracks: (0..50)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            selected: 0,
            ..Default::default()
        };
        s.apply_input(InputAction::ScrollDown);
        // The window is now 3..8, so row 0 is off screen and the cursor follows
        // to the first visible row.
        assert_eq!(s.scroll_offset, 3);
        assert_eq!(s.selected, 3);
    }

    #[test]
    fn scrolling_up_at_the_top_is_a_no_op() {
        let mut s = AppState {
            pane: Pane::Songs,
            focus: Focus::Main,
            viewport_rows: 10,
            tracks: (0..50)
                .map(|i| Track::stub(&format!("v{i}"), "T"))
                .collect(),
            ..Default::default()
        };
        s.apply_input(InputAction::ScrollUp);
        assert_eq!(s.scroll_offset, 0);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn an_open_artist_shows_their_tracks_and_h_leaves_again() {
        let mut s = AppState {
            pane: Pane::Artists,
            focus: Focus::Main,
            artists: vec![Artist {
                id: ArtistId::from("UC1"),
                name: "Someone".into(),
                subscribers: None,
                thumbnail_url: None,
            }],
            ..Default::default()
        };
        assert_eq!(s.list_len(), 1, "the artist list");
        s.apply(AppEvent::ArtistTracksLoaded {
            id: ArtistId::from("UC1"),
            name: "Someone".into(),
            tracks: vec![Track::stub("v1", "A song")],
        });
        assert_eq!(s.list_len(), 1);
        assert_eq!(
            s.selected_track().map(|t| t.title),
            Some("A song".to_owned()),
            "an open artist's rows are tracks"
        );
        assert!(s.close_open_artist());
        assert!(
            s.selected_track().is_none(),
            "closing must clear the tracks, not leave them behind"
        );
    }

    #[test]
    fn the_artists_pane_grows_a_query_row_when_its_search_opens() {
        // The bug: `S` focused the field but nothing drew it, so every later key
        // went into an invisible query and the pane looked frozen.
        let mut s = AppState {
            pane: Pane::Artists,
            focus: Focus::Main,
            ..Default::default()
        };
        assert!(!s.search_row_visible(), "no row before `S`");
        s.apply(AppEvent::Input(InputAction::OpenSearch));
        assert!(
            s.search_row_visible(),
            "`S` must put a field on screen, not just take focus"
        );
        assert_eq!(s.focus, Focus::SearchInput);
    }

    #[test]
    fn an_open_artist_hides_the_query_row() {
        // Their tracks are a list, not a search. Leaving the row up would claim
        // there is still a field to type in.
        let mut s = AppState {
            pane: Pane::Artists,
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::OpenSearch));
        s.apply(AppEvent::ArtistTracksLoaded {
            id: ArtistId::from("UC1"),
            name: "Someone".into(),
            tracks: vec![Track::stub("v1", "A song")],
        });
        assert!(!s.search_row_visible());
    }

    #[test]
    fn the_search_pane_always_has_its_query_row() {
        // It is drawn unconditionally, empty query or not. The click math used to
        // decide otherwise, so a click in an unused Search pane hit the row above.
        let s = AppState {
            pane: Pane::Search,
            ..Default::default()
        };
        assert!(s.search_row_visible());
    }

    #[test]
    fn leaving_the_artists_pane_clears_its_search() {
        // The two panes share one query buffer. Without this, typing an artist
        // name and then switching to Search showed that name already in the
        // field, as if the user had searched for it there.
        let mut s = AppState {
            pane: Pane::Artists,
            focus: Focus::Main,
            ..Default::default()
        };
        s.apply(AppEvent::Input(InputAction::OpenSearch));
        s.search_query = "sabrina".into();
        s.set_pane(Pane::Search);
        assert!(s.search_query.is_empty(), "the query must not follow");
        assert!(!s.artist_search_active);
    }

    #[test]
    fn opening_an_artist_does_not_clobber_the_library_songs() {
        // artist_tracks is a separate field for exactly this reason: reusing
        // `tracks` would lose the Fav pane's rows and make list_len disagree
        // with the screen after going back.
        let mut s = AppState {
            pane: Pane::Artists,
            focus: Focus::Main,
            tracks: vec![Track::stub("lib1", "Library song")],
            ..Default::default()
        };
        s.apply(AppEvent::ArtistTracksLoaded {
            id: ArtistId::from("UC1"),
            name: "Someone".into(),
            tracks: vec![Track::stub("v1", "Artist song")],
        });
        s.close_open_artist();
        s.set_pane(Pane::Songs);
        assert_eq!(
            s.selected_track().map(|t| t.title),
            Some("Library song".to_owned())
        );
    }
}
