//! The Player seam. The TUI speaks only these types — never libmpv2.

use ytm_core::{Track, TrackDuration, VideoId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    /// A track is resolving or buffering; show a spinner (FR-U4).
    Loading,
    Playing,
    Paused,
}

impl PlaybackState {
    /// True only when audio is actually coming out.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Playing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }
}

pub fn clamp_volume(v: i64) -> u8 {
    v.clamp(0, 100) as u8
}

/// Apply a relative seek, clamped to [0, duration].
pub fn apply_seek(pos: i64, delta: i64, duration: i64) -> i64 {
    (pos + delta).clamp(0, duration)
}

/// Sent from the event loop to the player actor.
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    /// Resolve and play immediately, replacing whatever is playing.
    PlayNow(Track),
    Pause,
    Resume,
    TogglePause,
    Stop,
    Next,
    Previous,
    SeekRelative(i64),
    SeekAbsolute(u64),
    SetVolume(u8),
    ToggleMute,
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    EnqueueBack(Vec<Track>),
    EnqueueNext(Vec<Track>),
    RemoveFromQueue(usize),
    MoveInQueue {
        from: usize,
        to: usize,
    },
    ClearQueue,
    Shutdown,
}

/// Sent from the player actor back to the event loop.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    /// Now-playing changed. `None` means the queue ran out.
    TrackChanged(Option<Track>),
    /// Emitted at least 4x/second while playing (FR-P7).
    Progress {
        position: TrackDuration,
        duration: TrackDuration,
    },
    VolumeChanged(u8),
    ShuffleChanged(bool),
    RepeatChanged(RepeatMode),
    QueueChanged {
        tracks: Vec<Track>,
        current: Option<usize>,
    },
    /// Human-readable; goes straight into a toast (NFR-9).
    Error(String),
    /// The current track finished naturally.
    TrackEnded(VideoId),
}

/// Implemented by `MpvPlayer` and `MockPlayer`. Commands are fire-and-forget;
/// everything observable comes back as a `PlayerEvent`.
pub trait Player: Send {
    fn send(&self, cmd: PlayerCommand) -> Result<(), PlayerError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("the audio player stopped responding")]
    ActorGone,
    #[error("mpv is not available: {0} — install libmpv to play audio")]
    MpvUnavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_mode_cycles_off_one_all() {
        // FR-P5: one key cycles through the three modes in this order.
        assert_eq!(RepeatMode::Off.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::Off);
    }

    #[test]
    fn playback_state_knows_when_it_is_audible() {
        assert!(PlaybackState::Playing.is_active());
        assert!(!PlaybackState::Paused.is_active());
        assert!(!PlaybackState::Stopped.is_active());
        assert!(!PlaybackState::Loading.is_active());
    }

    #[test]
    fn volume_is_clamped_to_the_valid_range() {
        assert_eq!(clamp_volume(150), 100);
        assert_eq!(clamp_volume(-10), 0);
        assert_eq!(clamp_volume(64), 64);
    }

    #[test]
    fn seek_relative_never_goes_below_zero() {
        assert_eq!(apply_seek(10, -30, 200), 0);
        assert_eq!(apply_seek(100, 30, 200), 130);
        assert_eq!(apply_seek(190, 30, 200), 200, "clamps to duration");
    }
}
