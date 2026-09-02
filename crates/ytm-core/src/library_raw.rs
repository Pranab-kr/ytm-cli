//! Telling "your library is empty" apart from "the response broke" (FR-B3).
//!
//! `ytmapi-rs` 0.3.3 cannot parse an empty library section. When the account has
//! no saved albums, YouTube does not send an empty `gridRenderer` — it sends a
//! `messageRenderer` ("No albums yet") in its place, and upstream's parser fails
//! with `Key /contents/.../gridRenderer not found in Api response`.
//!
//! That surfaced to the user as an error toast on a pane that should simply have
//! read "nothing here". Confirmed against the live account on 2026-08-31: the
//! captured response is `tests/fixtures/library_albums_empty.json`.
//!
//! This is deliberately a *narrow* check, not a general parser. It answers one
//! question — "is this the empty-library shape?" — and anything it does not
//! recognise returns false, so a genuine breakage still surfaces as an error
//! rather than being silently reported as an empty library.

use serde_json::Value;

/// Where a library browse response keeps its section list.
const SECTIONS_PATH: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";

/// True when the response says the library section is empty. False for a
/// populated section and false for anything unrecognised — a response that broke
/// for some other reason must still reach the user as an error.
pub fn is_empty_library(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let Some(sections) = v.pointer(SECTIONS_PATH).and_then(Value::as_array) else {
        return false;
    };

    // A populated library carries one of these; if either is present the
    // response is parseable and this is not the empty case.
    let has_content = sections.iter().any(|s| {
        let text = s.to_string();
        text.contains("gridRenderer") || text.contains("musicShelfRenderer")
    });
    if has_content {
        return false;
    }

    sections
        .iter()
        .any(|s| s.to_string().contains("messageRenderer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real response from an account with no saved albums, scrubbed of
    /// tracking params. This is the shape that used to produce an error toast.
    const EMPTY_ALBUMS: &str = include_str!("../tests/fixtures/library_albums_empty.json");

    #[test]
    fn the_real_empty_albums_response_is_recognised() {
        assert!(
            is_empty_library(EMPTY_ALBUMS),
            "the live 'No albums yet' response must read as empty, not as a parse failure"
        );
    }

    #[test]
    fn a_populated_grid_is_not_empty() {
        let json = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [
                { "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
                    { "gridRenderer": { "items": [{ "musicTwoRowItemRenderer": {} }] } }
                ]}}}}
            ]}}
        })
        .to_string();
        assert!(!is_empty_library(&json));
    }

    #[test]
    fn a_populated_shelf_is_not_empty() {
        // Artists come back as a shelf rather than a grid, and would hit the
        // same upstream failure the day the list empties.
        let json = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [
                { "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
                    { "musicShelfRenderer": { "contents": [{}] } }
                ]}}}}
            ]}}
        })
        .to_string();
        assert!(!is_empty_library(&json));
    }

    #[test]
    fn an_unrecognisable_response_is_not_called_empty() {
        // The important negative: a real breakage must still reach the user as
        // an error. Reporting it as "empty library" would hide it.
        assert!(!is_empty_library("{}"));
        assert!(!is_empty_library("not json at all"));
        assert!(!is_empty_library(""));
    }
}
