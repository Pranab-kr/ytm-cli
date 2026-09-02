//! Reading `setVideoId` out of a raw playlist response (FR-C5).
//!
//! `ytmapi-rs` 0.3.3 does not parse this field: `PlaylistSong` has no
//! `setVideoId`, and the only place upstream exposes one is `AddPlaylistItem`,
//! the *result of adding* a track. But `remove_playlist_items` requires it, so
//! without this a track read from a playlist could never be removed from it.
//! The field is in the wire JSON — upstream just drops it — so we read it
//! ourselves. This is option A of PROGRESS.md open question 1.
//!
//! Three things were established against a live playlist on 2026-08-31 rather
//! than assumed, and each one changed the design:
//!
//! 1. **Ids must be read per row.** An 83-track playlist carried 85
//!    `setVideoId` occurrences, so a document-wide scan misaligns. Each row's id
//!    comes from that row's own subtree.
//! 2. **The menu index is not fixed.** The id sat at `items/6` for those rows,
//!    but nothing guarantees it, so we search the row's menu items.
//! 3. **Positional pairing cannot work.** The shelf had 85 rows and upstream's
//!    parse returned 83 tracks — it drops rows internally (episodes, unavailable
//!    entries) before we ever see them. So each row carries its `videoId` and
//!    matching is by id, walking forward so a video that appears twice in one
//!    playlist still gets its two distinct entry ids in order.
//!
//! Every lookup is best-effort: a shape change upstream leaves `set_video_id`
//! as `None`, which surfaces as "these tracks cannot be removed" — the
//! behaviour we had before — rather than an error or a panic.

use crate::model::{SetVideoId, Track, VideoId};
use serde_json::Value;

/// Where the playlist rows live in a `GetPlaylistTracksQuery` response.
const SHELF_PATH: [&str; 5] = [
    "contents",
    "twoColumnBrowseResultsRenderer",
    "secondaryContents",
    "sectionListRenderer",
    "contents",
];

/// One `(videoId, setVideoId)` per playlist row, in response order. The `videoId`
/// is what makes this usable: upstream's typed parse silently drops rows, so the
/// two lists have different lengths and only the id can pair them.
pub fn entry_ids_from_raw(json: &str) -> Vec<(VideoId, Option<SetVideoId>)> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut node = &v;
    for key in SHELF_PATH {
        match node.get(key) {
            Some(next) => node = next,
            None => return Vec::new(),
        }
    }
    // The shelf is one of the section list's contents; find it rather than
    // assuming index 0.
    let Some(rows) = node
        .as_array()
        .and_then(|sections| {
            sections
                .iter()
                .find_map(|s| s.pointer("/musicPlaylistShelfRenderer/contents"))
        })
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    rows.iter()
        .filter_map(|row| {
            let video = row
                .pointer("/musicResponsiveListItemRenderer/playlistItemData/videoId")?
                .as_str()?;
            Some((VideoId::from(video), row_set_video_id(row)))
        })
        .collect()
}

/// The `setVideoId` belonging to one row, searched within that row only.
fn row_set_video_id(row: &Value) -> Option<SetVideoId> {
    let items = row
        .pointer("/musicResponsiveListItemRenderer/menu/menuRenderer/items")?
        .as_array()?;
    items.iter().find_map(|item| {
        let actions = item
            .pointer("/menuServiceItemRenderer/serviceEndpoint/playlistEditEndpoint/actions")?
            .as_array()?;
        actions
            .iter()
            .find_map(|a| a.get("setVideoId")?.as_str())
            .map(SetVideoId::from)
    })
}

/// Attach each row's entry id to the parsed track with the same `videoId`.
/// Matching walks forward and never revisits a row, so the same song twice gets
/// its two distinct `setVideoId`s — without the cursor, both copies get the first.
pub fn attach_entry_ids(tracks: Vec<Track>, rows: &[(VideoId, Option<SetVideoId>)]) -> Vec<Track> {
    let mut cursor = 0usize;
    tracks
        .into_iter()
        .map(|t| {
            let Some(offset) = rows[cursor.min(rows.len())..]
                .iter()
                .position(|(v, _)| *v == t.video_id)
            else {
                // No row claims this track: leave it unremovable rather than
                // attaching an id that belongs to a different entry.
                return t;
            };
            let at = cursor + offset;
            cursor = at + 1;
            Track {
                set_video_id: rows[at].1.clone(),
                ..t
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One playlist row, with its own menu containing its own setVideoId.
    fn item(video_id: &str, set_video_id: Option<&str>) -> serde_json::Value {
        let mut menu_items = vec![serde_json::json!({
            "menuNavigationItemRenderer": { "text": "Start radio" }
        })];
        if let Some(sv) = set_video_id {
            menu_items.push(serde_json::json!({
                "menuServiceItemRenderer": {
                    "serviceEndpoint": {
                        "playlistEditEndpoint": {
                            "actions": [{
                                "action": "ACTION_REMOVE_VIDEO",
                                "removedVideoId": video_id,
                                "setVideoId": sv,
                            }]
                        }
                    }
                }
            }));
        }
        serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": video_id },
                "menu": { "menuRenderer": { "items": menu_items } }
            }
        })
    }

    /// The real response shape, confirmed against a live playlist on 2026-08-31.
    fn response(items: Vec<serde_json::Value>) -> String {
        serde_json::json!({
            "contents": { "twoColumnBrowseResultsRenderer": {
                "secondaryContents": { "sectionListRenderer": { "contents": [
                    { "musicPlaylistShelfRenderer": { "contents": items } }
                ]}}
            }}
        })
        .to_string()
    }

    fn ids(json: &str) -> Vec<(VideoId, Option<SetVideoId>)> {
        entry_ids_from_raw(json)
    }

    #[test]
    fn each_row_yields_its_video_id_and_entry_id() {
        let got = ids(&response(vec![
            item("v1", Some("sv1")),
            item("v2", Some("sv2")),
        ]));
        assert_eq!(
            got,
            vec![
                (VideoId::from("v1"), Some(SetVideoId::from("sv1"))),
                (VideoId::from("v2"), Some(SetVideoId::from("sv2"))),
            ]
        );
    }

    #[test]
    fn a_row_without_an_entry_id_still_reports_its_video() {
        let got = ids(&response(vec![item("v1", None)]));
        assert_eq!(got, vec![(VideoId::from("v1"), None)]);
    }

    #[test]
    fn ids_are_taken_per_row_not_from_the_whole_document() {
        // The live response has more setVideoId occurrences than rows (85 for an
        // 83-track parse), so a document-wide scan would misalign every row
        // after the first extra.
        let mut v: serde_json::Value =
            serde_json::from_str(&response(vec![item("v1", Some("sv1"))])).unwrap();
        v["contents"]["twoColumnBrowseResultsRenderer"]["tabs"] =
            serde_json::json!([{ "setVideoId": "STRAY" }]);
        let got = ids(&v.to_string());
        assert_eq!(got.len(), 1, "one entry per row, not per occurrence");
        assert_eq!(got[0].1, Some(SetVideoId::from("sv1")));
    }

    #[test]
    fn an_unrecognisable_response_yields_nothing_rather_than_panicking() {
        // A shape change upstream must degrade to "cannot remove", which is the
        // behaviour we already have, not take the app down.
        assert!(ids("{}").is_empty());
        assert!(ids("not json at all").is_empty());
        assert!(ids("").is_empty());
    }

    #[test]
    fn attaching_matches_tracks_to_rows_by_video_id() {
        let rows = ids(&response(vec![
            item("v1", Some("sv1")),
            item("v2", Some("sv2")),
        ]));
        let got = attach_entry_ids(vec![Track::stub("v1", "A"), Track::stub("v2", "B")], &rows);
        assert_eq!(got[0].set_video_id, Some(SetVideoId::from("sv1")));
        assert_eq!(got[1].set_video_id, Some(SetVideoId::from("sv2")));
    }

    #[test]
    fn a_row_upstream_dropped_does_not_shift_the_rest() {
        // The bug that made the first attempt return zero removable tracks: the
        // live shelf had 85 rows and upstream parsed 83, so anything positional
        // discarded every id. Here row v2 never reaches us.
        let rows = ids(&response(vec![
            item("v1", Some("sv1")),
            item("v2", Some("sv2")),
            item("v3", Some("sv3")),
        ]));
        let got = attach_entry_ids(vec![Track::stub("v1", "A"), Track::stub("v3", "C")], &rows);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].set_video_id, Some(SetVideoId::from("sv1")));
        assert_eq!(
            got[1].set_video_id,
            Some(SetVideoId::from("sv3")),
            "v3 must keep its own id, not inherit v2's"
        );
    }

    #[test]
    fn a_duplicated_track_gets_its_own_entry_id() {
        // The same video twice in one playlist is exactly why setVideoId exists;
        // matching without a forward cursor would give both copies the first id
        // and delete the wrong row.
        let rows = ids(&response(vec![
            item("v1", Some("first")),
            item("v1", Some("second")),
        ]));
        let got = attach_entry_ids(vec![Track::stub("v1", "A"), Track::stub("v1", "A")], &rows);
        assert_eq!(got[0].set_video_id, Some(SetVideoId::from("first")));
        assert_eq!(got[1].set_video_id, Some(SetVideoId::from("second")));
    }

    #[test]
    fn a_track_no_row_claims_is_left_unremovable() {
        let rows = ids(&response(vec![item("v1", Some("sv1"))]));
        let got = attach_entry_ids(vec![Track::stub("nope", "X")], &rows);
        assert_eq!(got[0].set_video_id, None);
    }

    #[test]
    fn no_rows_at_all_leaves_every_track_untouched() {
        let got = attach_entry_ids(vec![Track::stub("v1", "A")], &[]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].set_video_id, None);
    }
}
