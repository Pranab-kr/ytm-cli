//! Tolerantly reading a song-search response, row by row (regression fix).
//!
//! `ytmapi-rs` 0.3.3's typed `search_songs` treats every three-field byline as
//! `artist • album • duration`. That is wrong for UGC music-video rows, whose
//! byline is `artist • view count • duration` — the middle `43K views` run has no
//! album `browseId`. The typed parser requires one, so it aborts the *entire*
//! response on a single such row: an intermittent guest-search failure where one
//! fan upload erased every other result (PROGRESS.md 2026-09-02).
//!
//! So song search now goes through `raw_json_query` and this parser instead of
//! the typed call, in both the authenticated and the guest source (see
//! `song_search_from_raw` in `ytmusic.rs`). Each
//! `musicResponsiveListItemRenderer` is parsed independently:
//!
//! - required: a title, a video id, and a parseable duration in the last byline
//!   field;
//! - the first byline field is the artist;
//! - the byline field *before the duration* is the album, but **only when that
//!   field actually carries an album `browseId`** — a view count is ignored;
//! - anything else (a row whose required fields cannot be read) is skipped, and
//!   a skipped row never erases the rows around it.
//!
//! The wire shape is the one upstream's typed parse reads, confirmed against
//! live responses: rows live under
//! `/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/
//! sectionListRenderer/contents/<shelf>/musicShelfRenderer/contents`, and each
//! row's title/byline are flex columns whose text runs separate the " • "
//! delimiters into their own runs.

use crate::model::{Track, TrackDuration, VideoId};
use serde_json::Value;

/// Where the filtered song-search shelves live.
const SEARCH_SECTIONS: &str =
    "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";

/// One content-bearing byline field. Separator runs (" • ") are dropped before
/// this; a field may or may not carry a `browseId`.
struct BylineField {
    text: String,
    /// A `navigationEndpoint.browseEndpoint.browseId`, present only for fields
    /// that link somewhere (an artist or an album). A view count has none.
    browse_id: Option<String>,
}

/// Parse a raw song-search response into playable tracks.
///
/// An empty result means "no usable songs", which the UI shows as an empty list —
/// the same treatment as a search with no hits, and never an error: one odd row
/// must not cost the user the songs around it.
pub fn tracks_from_raw(json: &str) -> Vec<Track> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(sections) = v.pointer(SEARCH_SECTIONS).and_then(Value::as_array) else {
        return Vec::new();
    };
    for section in sections {
        let Some(rows) = section
            .pointer("/musicShelfRenderer/contents")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let tracks: Vec<Track> = rows.iter().filter_map(track_from_row).collect();
        if !tracks.is_empty() {
            return tracks;
        }
    }
    Vec::new()
}

/// One row, best-effort. A row whose required fields cannot be read is dropped;
/// a dropped row never erases the rows around it.
fn track_from_row(item: &Value) -> Option<Track> {
    let row = item.get("musicResponsiveListItemRenderer")?;

    let title = row
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    // `playlistItemData.videoId` is the reliable place; the title run's
    // watchEndpoint is the fallback (see home_feed.rs).
    let video_id = row
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .or_else(|| {
            row.pointer(
                "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer\
                 /text/runs/0/navigationEndpoint/watchEndpoint/videoId",
            )
            .and_then(Value::as_str)
        })?;

    let fields = byline_fields(row);
    let artist = fields.first()?.text.clone();
    // The last byline field is the duration; if it does not read as one, this is
    // not a usable song row (a row with only "Artist • Album", say).
    let duration_secs = fields
        .last()
        .map(|f| crate::mapping::parse_duration(&f.text))
        .unwrap_or(0);
    if duration_secs == 0 {
        return None;
    }

    // The field before the duration is the album — but only when it really is
    // one. An album field carries a `browseId`; a UGC row's middle `43K views`
    // field carries none and must be ignored, not treated as an album. The
    // `>= 3` guard keeps the album slot clear of the artist when a row has only
    // two fields ("Artist • 3:26", say).
    let album = if fields.len() >= 3 {
        let candidate = &fields[fields.len() - 2];
        candidate.browse_id.as_ref().map(|_| candidate.text.clone())
    } else {
        None
    };

    let thumbnail_url = row
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(Track {
        video_id: VideoId::from(video_id),
        set_video_id: None,
        title: title.to_owned(),
        artists: vec![artist],
        album,
        duration: TrackDuration::from_secs(duration_secs),
        thumbnail_url,
        is_explicit: false,
    })
}

/// The byline's content fields, in order, with the " • " separators removed.
fn byline_fields(row: &Value) -> Vec<BylineField> {
    let Some(runs) = row
        .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    runs.iter()
        .filter_map(|run| {
            let text = run
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            // YouTube emits the " • " delimiter as its own text run.
            if text.is_empty() || text == "•" {
                return None;
            }
            let browse_id = run
                .pointer("/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(BylineField {
                text: text.to_owned(),
                browse_id,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VideoId;

    /// A scrubbed minimal capture preserving the UGC `artist • views • duration`
    /// byline shape (row `SYNVID00002`) next to ordinary album rows, plus one
    /// broken row that must be skipped without erasing the rest. No real
    /// identifiers: video/album/channel ids and art URLs are all `SYN*` or
    /// `example.invalid`.
    const UGC: &str = include_str!("../tests/fixtures/search_songs_ugc.json");

    fn by_video<'a>(tracks: &'a [Track], id: &str) -> Option<&'a Track> {
        tracks.iter().find(|t| t.video_id == VideoId::from(id))
    }

    #[test]
    fn the_view_count_row_yields_a_track_instead_of_an_error() {
        // The regression: ytmapi-rs read `Some Uploader • 43K views • 3:26` as
        // artist • album • duration and demanded an album browseId on `43K views`.
        let tracks = tracks_from_raw(UGC);
        let ugc = by_video(&tracks, "SYNVID00002")
            .expect("the UGC view-count row must parse into a track");
        assert_eq!(ugc.title, "Freak (official upload)");
        assert_eq!(
            ugc.artists,
            vec!["Some Uploader"],
            "first byline field is the artist"
        );
        assert_eq!(ugc.album, None, "a view count is not an album");
        assert_eq!(ugc.duration.as_secs(), 206, "3:26 in the last byline field");
    }

    #[test]
    fn an_album_is_recognised_only_when_a_byline_field_has_a_browse_id() {
        let tracks = tracks_from_raw(UGC);
        let album = by_video(&tracks, "SYNVID00001").expect("the ordinary album row must parse");
        assert_eq!(
            album.album.as_deref(),
            Some("Follow the Leader"),
            "the byline field carrying an album browseId is the album"
        );
        assert_eq!(album.duration.as_secs(), 295, "4:55");
        assert!(
            album.thumbnail_url.is_some(),
            "thumbnails are kept when present"
        );
        // And the view-count row right next to it stays album-less.
        let ugc = by_video(&tracks, "SYNVID00002").expect("view-count row parses");
        assert_eq!(ugc.album, None);
    }

    #[test]
    fn a_bad_row_is_skipped_without_erasing_the_songs_around_it() {
        // Row SYNVID00002 (UGC) and the broken "Unavailable track" row sit
        // between two valid rows. The typed parser died on the UGC row and lost
        // everything after it; ours must keep all three playable rows in order.
        let tracks = tracks_from_raw(UGC);
        let ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["SYNVID00001", "SYNVID00002", "SYNVID00003"],
            "valid rows survive the view-count row and the broken row"
        );
    }

    #[test]
    fn an_unrecognisable_response_yields_no_tracks_rather_than_panicking() {
        // A shape change upstream must cost an empty search list, never an error
        // or a crash.
        assert!(tracks_from_raw("{}").is_empty());
        assert!(tracks_from_raw("not json").is_empty());
        assert!(tracks_from_raw("").is_empty());
    }
}
