//! Everything that can change the app. The event loop's only vocabulary.

use ytm_core::{Album, Artist, Playlist, PlaylistId, Track};
use ytm_player::player::PlayerEvent;

/// A key press already resolved through the keymap into an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Confirm,
    Cancel,
    NextPane,
    PrevPane,
    GoTo(u8),
    TogglePause,
    NextTrack,
    PrevTrack,
    SeekForward,
    SeekBack,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    OpenSearch,
    OpenQueue,
    OpenHelp,
    AddToQueue,
    PlayNext,
    /// Reorder the selected queue entry (FR-Q3). Queue-only.
    MoveEntryUp,
    MoveEntryDown,
    ClearQueue,
    CreatePlaylist,
    RenamePlaylist,
    DeletePlaylist,
    RemoveFromPlaylist,
    AddToPlaylist,
    Refresh,
    ToggleMark,
    Char(char),
    Backspace,
}

#[derive(Debug)]
pub enum AppEvent {
    Input(InputAction),
    Player(PlayerEvent),
    Tick,
    Resize,

    PlaylistsLoaded(Vec<Playlist>),
    LibrarySongsLoaded(Vec<Track>),
    AlbumsLoaded(Vec<Album>),
    ArtistsLoaded(Vec<Artist>),
    PlaylistTracksLoaded {
        id: PlaylistId,
        tracks: Vec<Track>,
    },
    SearchResults {
        query: String,
        tracks: Vec<Track>,
    },

    /// A mutation succeeded server-side; `token` matches the optimistic edit.
    MutationOk {
        token: u64,
        message: String,
    },
    /// A mutation failed; roll back the edit tagged with `token`.
    MutationFailed {
        token: u64,
        message: String,
    },

    Error(String),
    LoginNeeded {
        user_code: String,
        url: String,
    },
    LoginComplete,
}
