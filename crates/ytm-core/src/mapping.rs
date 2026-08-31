//! ytmapi-rs types -> our model types. The only place upstream types appear
//! besides ytmusic.rs, so an upstream rename breaks exactly one file.

use crate::model::*;
use ytmapi_rs::common::{Explicit, Thumbnail, YoutubeID};
use ytmapi_rs::parse::{
    GetPlaylistDetails, LibraryArtist, LibraryArtistSubscription, LibraryPlaylist, PlaylistItem,
    SearchResultAlbum, SearchResultArtist, SearchResultCommunityPlaylist,
    SearchResultFeaturedPlaylist, SearchResultSong, TableListSong,
};

/// Playlist ids YouTube Music owns and refuses to let us edit.
/// Accepts either id form. A library listing reports Liked Music as `VLLM`, so
/// matching the bare form alone left every system playlist unflagged: no
/// read-only marker, and the rename/delete guards never fired.
pub fn is_system_playlist(id: &str) -> bool {
    let bare = id.strip_prefix("VL").unwrap_or(id);
    matches!(bare, "LM" | "SE") || bare.starts_with("RDAMPL")
}

/// ytmapi-rs 0.3.3 reports durations as display strings ("2:29", "1:02:05").
/// Anything unparseable becomes 0 rather than failing the whole load.
pub fn parse_duration(s: &str) -> u64 {
    let mut secs = 0u64;
    for part in s.trim().split(':') {
        match part.trim().parse::<u64>() {
            Ok(n) => secs = secs * 60 + n,
            Err(_) => return 0,
        }
    }
    secs
}

/// Largest thumbnail, which is the one worth showing as album art.
fn best_thumbnail(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| t.width * t.height)
        .map(|t| t.url.clone())
}

fn is_explicit(e: &Explicit) -> bool {
    matches!(e, Explicit::IsExplicit)
}

/// Leading number in strings like "42 songs".
fn leading_count(s: &str) -> Option<u32> {
    s.split_whitespace().next()?.replace(',', "").parse().ok()
}

/// The fields we need from any playlist entry, regardless of source type.
pub struct TrackParts {
    pub video_id: String,
    pub set_video_id: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_secs: u64,
    pub thumbnail_url: Option<String>,
    pub is_explicit: bool,
}

pub fn track_from_parts(p: impl Into<TrackParts>) -> Track {
    let p = p.into();
    Track {
        video_id: VideoId(p.video_id),
        set_video_id: p.set_video_id.map(SetVideoId),
        title: p.title,
        artists: p.artists,
        album: p.album,
        duration: TrackDuration::from_secs(p.duration_secs),
        thumbnail_url: p.thumbnail_url,
        is_explicit: p.is_explicit,
    }
}

/// A playlist entry. Songs, videos, and uploads all become tracks; episodes
/// (podcasts) are out of scope and map to `None`.
///
/// Note: `PlaylistSong` in 0.3.3 carries no `setVideoId`, so tracks read from a
/// playlist are not removable until it does. See PROGRESS.md's open question.
pub fn track_from_playlist_item(item: &PlaylistItem) -> Option<Track> {
    let parts = match item {
        PlaylistItem::Song(s) => TrackParts {
            video_id: s.video_id.get_raw().to_owned(),
            set_video_id: None,
            title: s.title.clone(),
            artists: s.artists.iter().map(|a| a.name.clone()).collect(),
            album: Some(s.album.name.clone()),
            duration_secs: parse_duration(&s.duration),
            thumbnail_url: best_thumbnail(&s.thumbnails),
            is_explicit: is_explicit(&s.explicit),
        },
        PlaylistItem::Video(v) => TrackParts {
            video_id: v.video_id.get_raw().to_owned(),
            set_video_id: None,
            title: v.title.clone(),
            artists: vec![v.channel_name.clone()],
            album: None,
            duration_secs: parse_duration(&v.duration),
            thumbnail_url: best_thumbnail(&v.thumbnails),
            is_explicit: false,
        },
        PlaylistItem::UploadSong(u) => TrackParts {
            video_id: u.video_id.get_raw().to_owned(),
            set_video_id: None,
            title: u.title.clone(),
            artists: u.artists.iter().map(|a| a.name.clone()).collect(),
            album: u.album.as_ref().map(|a| a.name.clone()),
            duration_secs: parse_duration(&u.duration),
            thumbnail_url: best_thumbnail(&u.thumbnails),
            is_explicit: false,
        },
        // Podcasts are out of scope.
        PlaylistItem::Episode(_) => return None,
    };
    Some(track_from_parts(parts))
}

pub fn track_from_search_song(s: &SearchResultSong) -> Track {
    track_from_parts(TrackParts {
        video_id: s.video_id.get_raw().to_owned(),
        set_video_id: None,
        title: s.title.clone(),
        artists: vec![s.artist.clone()],
        album: s.album.as_ref().map(|a| a.name.clone()),
        duration_secs: parse_duration(&s.duration),
        thumbnail_url: best_thumbnail(&s.thumbnails),
        is_explicit: is_explicit(&s.explicit),
    })
}

pub fn playlist_from_library(p: &LibraryPlaylist) -> Playlist {
    let id = p.playlist_id.get_raw().to_owned();
    Playlist {
        is_system: is_system_playlist(&id),
        id: PlaylistId(id),
        title: p.title.clone(),
        description: None,
        track_count: leading_count(&p.tracks),
        // The library listing does not report privacy; details do.
        privacy: Privacy::Private,
        thumbnail_url: best_thumbnail(&p.thumbnails),
    }
}

pub fn playlist_from_details(d: &GetPlaylistDetails) -> Playlist {
    use ytmapi_rs::query::playlist::PrivacyStatus;
    let id = d.id.get_raw().to_owned();
    Playlist {
        is_system: is_system_playlist(&id),
        id: PlaylistId(id),
        title: d.title.clone(),
        description: d.description.clone(),
        track_count: leading_count(&d.track_count_text),
        privacy: match d.privacy {
            Some(PrivacyStatus::Public) => Privacy::Public,
            Some(PrivacyStatus::Unlisted) => Privacy::Unlisted,
            _ => Privacy::Private,
        },
        thumbnail_url: best_thumbnail(&d.thumbnails),
    }
}

pub fn track_from_table_list(s: &TableListSong) -> Track {
    track_from_parts(TrackParts {
        video_id: s.video_id.get_raw().to_owned(),
        set_video_id: None,
        title: s.title.clone(),
        artists: s.artists.iter().map(|a| a.name.clone()).collect(),
        album: Some(s.album.name.clone()),
        duration_secs: parse_duration(&s.duration),
        thumbnail_url: best_thumbnail(&s.thumbnails),
        is_explicit: is_explicit(&s.explicit),
    })
}

pub fn playlist_from_search_featured(p: &SearchResultFeaturedPlaylist) -> Playlist {
    let id = p.playlist_id.get_raw().to_owned();
    Playlist {
        is_system: is_system_playlist(&id),
        id: PlaylistId(id),
        title: p.title.clone(),
        description: None,
        track_count: leading_count(&p.songs),
        privacy: Privacy::Public,
        thumbnail_url: best_thumbnail(&p.thumbnails),
    }
}

pub fn playlist_from_search_community(p: &SearchResultCommunityPlaylist) -> Playlist {
    let id = p.playlist_id.get_raw().to_owned();
    Playlist {
        is_system: is_system_playlist(&id),
        id: PlaylistId(id),
        title: p.title.clone(),
        description: None,
        track_count: None,
        privacy: Privacy::Public,
        thumbnail_url: best_thumbnail(&p.thumbnails),
    }
}

pub fn album_from_search(a: &SearchResultAlbum) -> Album {
    Album {
        id: AlbumId(a.album_id.get_raw().to_owned()),
        title: a.title.clone(),
        artists: vec![a.artist.clone()],
        year: Some(a.year.clone()),
        thumbnail_url: best_thumbnail(&a.thumbnails),
    }
}

pub fn artist_from_search(a: &SearchResultArtist) -> Artist {
    Artist {
        id: ArtistId(a.browse_id.get_raw().to_owned()),
        name: a.artist.clone(),
        subscribers: a.subscribers.clone(),
        thumbnail_url: best_thumbnail(&a.thumbnails),
    }
}

pub fn artist_from_library(a: &LibraryArtist) -> Artist {
    Artist {
        id: ArtistId(a.channel_id.get_raw().to_owned()),
        name: a.artist.clone(),
        // The library listing gives a byline ("16 songs"), not a subscriber count.
        subscribers: None,
        thumbnail_url: None,
    }
}

pub fn artist_from_subscription(a: &LibraryArtistSubscription) -> Artist {
    Artist {
        id: ArtistId(a.channel_id.get_raw().to_owned()),
        name: a.name.clone(),
        subscribers: Some(a.subscribers.clone()),
        thumbnail_url: best_thumbnail(&a.thumbnails),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the fields we read off an upstream playlist entry, so mapping is
    /// testable without constructing `ytmapi-rs` types (several have private
    /// fields or are `#[non_exhaustive]`).
    struct TestPlaylistItem {
        video_id: String,
        set_video_id: Option<String>,
        title: String,
        artists: Vec<String>,
        album: Option<String>,
        duration_secs: u64,
    }

    impl TestPlaylistItem {
        fn stub() -> Self {
            Self {
                video_id: "v1".into(),
                set_video_id: None,
                title: "Title".into(),
                artists: vec!["Artist".into()],
                album: None,
                duration_secs: 180,
            }
        }
    }

    impl From<TestPlaylistItem> for TrackParts {
        fn from(t: TestPlaylistItem) -> Self {
            TrackParts {
                video_id: t.video_id,
                set_video_id: t.set_video_id,
                title: t.title,
                artists: t.artists,
                album: t.album,
                duration_secs: t.duration_secs,
                thumbnail_url: None,
                is_explicit: false,
            }
        }
    }

    #[test]
    fn maps_a_playlist_item_into_our_track() {
        let item = TestPlaylistItem {
            video_id: "abc123".into(),
            set_video_id: Some("set789".into()),
            title: "Roygbiv".into(),
            artists: vec!["Boards of Canada".into()],
            album: Some("Music Has the Right".into()),
            duration_secs: 149,
        };
        let t = track_from_parts(item);
        assert_eq!(t.video_id, VideoId::from("abc123"));
        assert_eq!(t.set_video_id, Some(SetVideoId::from("set789")));
        assert_eq!(t.duration.to_string(), "2:29");
        assert!(
            t.is_removable(),
            "playlist tracks must keep set_video_id (see FR-C5)"
        );
    }

    #[test]
    fn missing_duration_maps_to_zero_not_a_panic() {
        let t = track_from_parts(TestPlaylistItem {
            duration_secs: 0,
            ..TestPlaylistItem::stub()
        });
        assert_eq!(t.duration.as_secs(), 0);
        assert_eq!(t.duration.to_string(), "0:00");
    }

    #[test]
    fn system_playlists_are_flagged_not_editable() {
        // "Your Likes" (LM) cannot be edited; the UI must refuse before calling the API.
        assert!(is_system_playlist("LM"));
        assert!(is_system_playlist("SE"));
        assert!(!is_system_playlist("PLxxxx"));
    }

    #[test]
    fn a_system_playlist_is_recognised_in_its_browse_form_too() {
        // The library reports Liked Music as "VLLM", not "LM", so matching the
        // bare form only left it unflagged: the read-only marker never showed and
        // the rename/delete guards never fired.
        assert!(is_system_playlist("VLLM"));
        assert!(is_system_playlist("VLSE"));
        assert!(is_system_playlist("VLRDAMPL123"));
        // And a real playlist stays editable in either form.
        assert!(!is_system_playlist("VLPLxxxx"));
    }

    #[test]
    fn fixture_parses_into_playlists() {
        // A REAL scrubbed capture since 2026-08-31, so this can assert the wire
        // shape rather than just "is JSON" — the old hand-written fixture could
        // only do the latter, which is why it never caught the empty-library
        // hazard that broke the albums pane (see library_raw.rs).
        let raw = include_str!("../tests/fixtures/library_playlists.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("fixture must be valid JSON");

        let items = v
            .pointer(
                "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer\
                 /content/sectionListRenderer/contents/0/gridRenderer/items",
            )
            .and_then(serde_json::Value::as_array)
            .expect("the real response keeps library playlists in a gridRenderer");
        assert!(items.len() > 1, "fixture should hold several playlists");

        // Every row carries the two fields mapping actually reads.
        let rows: Vec<_> = items
            .iter()
            .filter_map(|i| i.get("musicTwoRowItemRenderer"))
            .collect();
        assert!(!rows.is_empty(), "rows are musicTwoRowItemRenderer");
        for r in &rows {
            assert!(
                r.pointer("/title/runs/0/text")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|t| !t.is_empty()),
                "every row needs a title"
            );
        }

        // The scrub must hold: no tracking params, no auth-bearing tokens.
        let body = v.to_string();
        for leaked in ["clickTrackingParams", "visitorData", "SAPISID"] {
            assert!(
                !body.contains(leaked),
                "{leaked} must never be committed in a fixture"
            );
        }
    }

    #[test]
    fn the_fixture_is_a_real_capture_not_a_hand_written_one() {
        // Guards the thing PROGRESS.md tracked as open question 2 for two
        // sessions: a SYNTHETIC fixture validates JSON parsing only, not the
        // response shape. If someone replaces this with a hand-written stub,
        // this test says so.
        let raw = include_str!("../tests/fixtures/library_playlists.json");
        assert!(
            !raw.contains("SYNTHETIC fixture"),
            "the fixture should be a real scrubbed capture"
        );
    }

    #[test]
    fn duration_strings_from_the_api_parse_into_seconds() {
        // ytmapi-rs 0.3.3 gives durations as display strings ("2:29"), not seconds.
        assert_eq!(parse_duration("2:29"), 149);
        assert_eq!(parse_duration("0:09"), 9);
        assert_eq!(parse_duration("1:02:05"), 3725);
        // Anything unexpected is 0 rather than a panic — a bad duration must
        // never take down a whole playlist load.
        assert_eq!(parse_duration(""), 0);
        assert_eq!(parse_duration("unknown"), 0);
        assert_eq!(parse_duration("12"), 12);
    }
}
