//! Domain model. Deliberately independent of ytmapi-rs so the API layer can be
//! swapped without touching the UI.

use std::fmt;

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
        )]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    VideoId,
    "A YouTube video id — identifies the audio to play."
);
id_type!(PlaylistId, "A YouTube Music playlist id.");
id_type!(
    SetVideoId,
    "Identifies a specific *entry* in a specific playlist. Required to remove \
     that entry; the VideoId alone is not enough because a track may appear twice."
);
id_type!(AlbumId, "A YouTube Music album/browse id.");
id_type!(ArtistId, "A YouTube Music artist/channel id.");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TrackDuration(pub u64);

impl TrackDuration {
    pub fn from_secs(s: u64) -> Self {
        Self(s)
    }
    pub fn as_secs(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for TrackDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (h, m, s) = (self.0 / 3600, (self.0 % 3600) / 60, self.0 % 60);
        if h > 0 {
            write!(f, "{h}:{m:02}:{s:02}")
        } else {
            write!(f, "{m}:{s:02}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Privacy {
    #[default]
    Private,
    Public,
    Unlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub video_id: VideoId,
    /// Present only for tracks read from a playlist. Removal requires it.
    pub set_video_id: Option<SetVideoId>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration: TrackDuration,
    pub thumbnail_url: Option<String>,
    pub is_explicit: bool,
}

impl Track {
    /// Test helper: a minimal valid track.
    pub fn stub(video_id: &str, title: &str) -> Self {
        Self {
            video_id: video_id.into(),
            set_video_id: None,
            title: title.to_owned(),
            artists: vec!["Test Artist".into()],
            album: None,
            duration: TrackDuration::from_secs(180),
            thumbnail_url: None,
            is_explicit: false,
        }
    }

    pub fn artist_display(&self) -> String {
        if self.artists.is_empty() {
            "Unknown artist".to_owned()
        } else {
            self.artists.join(", ")
        }
    }

    pub fn is_removable(&self) -> bool {
        self.set_video_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub description: Option<String>,
    pub track_count: Option<u32>,
    pub privacy: Privacy,
    pub thumbnail_url: Option<String>,
    /// True when this playlist cannot be edited (e.g. "Your Likes").
    pub is_system: bool,
}

impl Playlist {
    pub fn stub(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            title: title.to_owned(),
            description: None,
            track_count: Some(0),
            privacy: Privacy::Private,
            thumbnail_url: None,
            is_system: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artists: Vec<String>,
    pub year: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub subscribers: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formats_as_mmss_under_an_hour() {
        assert_eq!(TrackDuration::from_secs(0).to_string(), "0:00");
        assert_eq!(TrackDuration::from_secs(9).to_string(), "0:09");
        assert_eq!(TrackDuration::from_secs(215).to_string(), "3:35");
        assert_eq!(TrackDuration::from_secs(3599).to_string(), "59:59");
    }

    #[test]
    fn duration_formats_with_hours_when_an_hour_or_more() {
        assert_eq!(TrackDuration::from_secs(3600).to_string(), "1:00:00");
        assert_eq!(TrackDuration::from_secs(3725).to_string(), "1:02:05");
    }

    #[test]
    fn track_artist_display_joins_multiple_artists() {
        let t = Track {
            artists: vec!["Boards of Canada".into(), "Autechre".into()],
            ..Track::stub("v1", "Title")
        };
        assert_eq!(t.artist_display(), "Boards of Canada, Autechre");
    }

    #[test]
    fn track_artist_display_is_placeholder_when_empty() {
        let t = Track {
            artists: vec![],
            ..Track::stub("v1", "Title")
        };
        assert_eq!(t.artist_display(), "Unknown artist");
    }

    #[test]
    fn track_without_set_video_id_cannot_be_removed_from_playlist() {
        // SetVideoId is per-playlist-entry; without it the API cannot remove the row.
        let t = Track::stub("v1", "Title");
        assert!(!t.is_removable());
        let t = Track {
            set_video_id: Some(SetVideoId("s1".into())),
            ..Track::stub("v1", "Title")
        };
        assert!(t.is_removable());
    }
}
