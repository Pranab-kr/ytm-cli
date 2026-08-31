//! YouTube Music's home feed — the recommendation carousels (FR-B6).
//!
//! `ytmapi-rs` 0.3.3 has no home query at all, so both the request and the
//! parse are ours. `Query`/`PostQuery` are public and documented as
//! user-implementable, which is what makes `browseId: FEmusic_home` reachable
//! without forking upstream; `GetHomeQuery` in `ytmusic.rs` is that impl.
//!
//! Shape confirmed against the live account on 2026-08-31 and captured in
//! `tests/fixtures/home_feed.json`. Three things worth knowing:
//!
//! 1. **Which shelves come back is YouTube's decision, not ours.** The web
//!    player shows "Quick picks" and "Albums for you"; this client is served
//!    "Listen again", "From your library", and "Listen together" for the same
//!    endpoint. So the shelf title is *data* — we render what arrives rather
//!    than looking for named shelves that may never appear.
//! 2. **One carousel mixes kinds.** A single shelf holds tracks, playlists,
//!    albums, and artists together, distinguished only by the endpoint on each
//!    card. `HomeTarget` is what lets the UI know what Enter should do.
//! 3. **Every field is best-effort.** A card missing a title or an endpoint is
//!    skipped rather than failing the whole feed — one odd row must not cost
//!    the user their entire home pane.

use crate::model::{AlbumId, ArtistId, HomeItem, HomeShelf, HomeTarget, PlaylistId, VideoId};
use serde_json::Value;

/// Where the first page keeps its carousels.
const SECTIONS: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
/// Where a *continuation* keeps them — a different path for the same shape.
const CONT_SECTIONS: &str = "/continuationContents/sectionListContinuation/contents";
/// Where each response advertises the next page.
const CONT_TOKEN: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/continuations/0/nextContinuationData/continuation";
const CONT_TOKEN_NEXT: &str = "/continuationContents/sectionListContinuation/continuations/0/nextContinuationData/continuation";

/// Parse the feed into shelves, dropping anything unrecognisable.
///
/// An empty result means "no feed", which the UI shows as an empty pane — the
/// same treatment as an empty library, and never an error, because a home feed
/// is a convenience rather than something the user asked to see.
pub fn shelves_from_raw(json: &str) -> Vec<HomeShelf> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    // The first page and a continuation put identical shelves under different
    // paths, so try both rather than keeping two near-identical parsers.
    //
    // Order matters and cost a debugging round: a continuation response carries
    // *both* an empty first-page shell under `/contents` and the real shelves
    // under `/continuationContents`. Taking the first path that merely exists
    // yields nothing, so take the first that actually holds shelves.
    [SECTIONS, CONT_SECTIONS]
        .iter()
        .filter_map(|path| v.pointer(path).and_then(Value::as_array))
        .map(|sections| sections.iter().filter_map(shelf).collect::<Vec<_>>())
        .find(|shelves: &Vec<HomeShelf>| !shelves.is_empty())
        .unwrap_or_default()
}

/// The token for the next page of shelves, if the feed offers one.
///
/// This matters more than it looks: the **first page is not the interesting
/// one**. Measured live on 2026-08-31, page 1 returned "Listen again", "From
/// your library", and "Listen together", while page 2 held "Quick picks",
/// "Covers and remixes", and "Heard in Shorts" — the shelves the web player
/// leads with. A client that reads only page 1 shows none of the actual
/// recommendations.
pub fn continuation_token(json: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(json).ok()?;
    v.pointer(CONT_TOKEN)
        .or_else(|| v.pointer(CONT_TOKEN_NEXT))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn shelf(section: &Value) -> Option<HomeShelf> {
    let shelf = section.pointer("/musicCarouselShelfRenderer")?;
    let title = shelf
        .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
        .and_then(Value::as_str)
        .unwrap_or("Recommended")
        .to_owned();
    let items: Vec<HomeItem> = shelf
        .pointer("/contents")?
        .as_array()?
        .iter()
        .filter_map(card)
        .collect();
    // A shelf whose every card we failed to read is worse than no shelf: it
    // renders as a heading over nothing.
    if items.is_empty() {
        return None;
    }
    Some(HomeShelf { title, items })
}

/// One card, in whichever of the two shapes the shelf used.
///
/// The feed uses two item renderers and they are not interchangeable:
/// `musicTwoRowItemRenderer` for the artwork cards on page 1, and
/// `musicResponsiveListItemRenderer` for the list rows that "Quick picks" and
/// the other page-2 shelves are built from. Handling only the first parses page
/// 1 and silently returns nothing for the shelves the user actually wants.
fn card(item: &Value) -> Option<HomeItem> {
    if let Some(r) = item.pointer("/musicResponsiveListItemRenderer") {
        return list_row(r);
    }
    two_row_card(item)
}

/// The list-row shape: title and byline live in `flexColumns`, and the video id
/// in `playlistItemData`.
fn list_row(r: &Value) -> Option<HomeItem> {
    let columns = r.pointer("/flexColumns").and_then(Value::as_array)?;

    /// The joined text of one flex column.
    fn column_text(col: &Value) -> Option<String> {
        let runs = col
            .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")?
            .as_array()?;
        let text: String = runs
            .iter()
            .filter_map(|x| x.get("text").and_then(Value::as_str))
            .collect();
        (!text.is_empty()).then_some(text)
    }

    let title = columns.first().and_then(column_text)?;
    // Column 2 is the byline ("Artist • 60M plays"); a row may have none.
    let subtitle = columns.get(1).and_then(column_text).unwrap_or_default();

    // These rows are songs, so the video id is the point. `playlistItemData` is
    // the reliable place; the title run's endpoint is the fallback.
    let video = r
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .or_else(|| {
            columns
                .first()?
                .pointer(
                    "/musicResponsiveListItemFlexColumnRenderer/text/runs/0\
                     /navigationEndpoint/watchEndpoint/videoId",
                )?
                .as_str()
        })?;

    let thumbnail_url = r
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(HomeItem {
        title,
        subtitle,
        target: HomeTarget::Track(VideoId::from(video)),
        thumbnail_url,
    })
}

fn two_row_card(item: &Value) -> Option<HomeItem> {
    let r = item.pointer("/musicTwoRowItemRenderer")?;
    let title = r
        .pointer("/title/runs/0/text")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_owned();

    // The subtitle arrives as runs including separators (" • "); joining them
    // verbatim reproduces what the web player shows on the card's second line.
    let subtitle = r
        .pointer("/subtitle/runs")
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|x| x.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default();

    let thumbnail_url = r
        .pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(HomeItem {
        title,
        subtitle,
        target: target(r.pointer("/navigationEndpoint")?)?,
        thumbnail_url,
    })
}

/// What the card points at. A `watchEndpoint` is directly playable; a
/// `browseEndpoint` needs its `pageType` to say what kind of page it opens.
fn target(nav: &Value) -> Option<HomeTarget> {
    if let Some(v) = nav
        .pointer("/watchEndpoint/videoId")
        .and_then(Value::as_str)
    {
        return Some(HomeTarget::Track(VideoId::from(v)));
    }
    let browse = nav.pointer("/browseEndpoint")?;
    let id = browse.get("browseId").and_then(Value::as_str)?;
    let page_type = browse
        .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match page_type {
        "MUSIC_PAGE_TYPE_PLAYLIST" => Some(HomeTarget::Playlist(PlaylistId::from(id))),
        "MUSIC_PAGE_TYPE_ALBUM" => Some(HomeTarget::Album(AlbumId::from(id))),
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
            Some(HomeTarget::Artist(ArtistId::from(id)))
        }
        // An unknown page type has no sensible Enter behaviour, so the card is
        // dropped rather than rendered as a dead row.
        _ => None,
    }
}

/// The album cards from a parsed feed, as `Album`s (FR-B3).
///
/// The library albums pane is empty for most accounts — YouTube only lists
/// albums you explicitly saved — so an empty pane is a dead end rather than
/// information. These are the albums the feed recommends.
///
/// `year` is left `None`: the feed's second line is a byline ("Example Name •
/// EP"), not a year, and inventing one from it would show the user a wrong date.
pub fn albums_from_shelves(shelves: &[HomeShelf]) -> Vec<crate::model::Album> {
    let mut seen = std::collections::HashSet::new();
    shelves
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|i| match &i.target {
            HomeTarget::Album(id) => Some((id, i)),
            _ => None,
        })
        // One album can appear in several shelves; a list repeating it reads as
        // a bug.
        .filter(|(id, _)| seen.insert((*id).clone()))
        .map(|(id, i)| crate::model::Album {
            id: id.clone(),
            title: i.title.clone(),
            artists: if i.subtitle.is_empty() {
                Vec::new()
            } else {
                vec![i.subtitle.clone()]
            },
            year: None,
            thumbnail_url: i.thumbnail_url.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real capture of FEmusic_home, scrubbed. Trimmed to three cards per
    /// shelf; the structure is untouched.
    const FEED: &str = include_str!("../tests/fixtures/home_feed.json");

    #[test]
    fn the_real_feed_parses_into_shelves_with_titles() {
        let shelves = shelves_from_raw(FEED);
        assert!(!shelves.is_empty(), "the live feed must yield shelves");
        // Shelf titles are YouTube's own labels and are what the UI heads each
        // group with, so an empty one would render as a blank heading.
        for s in &shelves {
            assert!(!s.title.is_empty(), "every shelf needs a title");
            assert!(!s.items.is_empty(), "an empty shelf should be dropped");
        }
        let titles: Vec<&str> = shelves.iter().map(|s| s.title.as_str()).collect();
        assert!(
            titles.contains(&"Listen again"),
            "expected the captured shelves, got {titles:?}"
        );
    }

    #[test]
    fn cards_carry_a_title_and_something_to_do_with_them() {
        for s in shelves_from_raw(FEED) {
            for i in &s.items {
                assert!(!i.title.is_empty());
                // kind_label is what the list shows as the row's type tag.
                assert!(!i.kind_label().is_empty());
            }
        }
    }

    #[test]
    fn the_feed_mixes_playable_tracks_with_pages_to_open() {
        // This is the property the UI has to cope with: one carousel is not one
        // kind of thing. If a future capture flattens to a single kind, the
        // pane's per-row Enter behaviour is being tested against less than the
        // real feed.
        let items: Vec<HomeItem> = shelves_from_raw(FEED)
            .into_iter()
            .flat_map(|s| s.items)
            .collect();
        let tracks = items
            .iter()
            .filter(|i| matches!(i.target, HomeTarget::Track(_)))
            .count();
        let pages = items.len() - tracks;
        assert!(tracks > 0, "the feed had no playable tracks");
        assert!(pages > 0, "the feed had no openable pages");
    }

    #[test]
    fn subtitles_read_as_one_line() {
        // Runs arrive split around " • " separators; a card showing only the
        // first run loses the track count or the artist.
        let items: Vec<HomeItem> = shelves_from_raw(FEED)
            .into_iter()
            .flat_map(|s| s.items)
            .collect();
        assert!(
            items.iter().any(|i| i.subtitle.contains('•')),
            "at least one card should keep its joined subtitle"
        );
    }

    #[test]
    fn a_card_without_an_endpoint_is_dropped_not_rendered_dead() {
        let json = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [
                { "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
                    { "musicCarouselShelfRenderer": {
                        "header": { "musicCarouselShelfBasicHeaderRenderer": {
                            "title": { "runs": [{ "text": "Mixed" }] } } },
                        "contents": [
                            { "musicTwoRowItemRenderer": {
                                "title": { "runs": [{ "text": "playable" }] },
                                "navigationEndpoint": { "watchEndpoint": { "videoId": "abc" } } } },
                            { "musicTwoRowItemRenderer": {
                                "title": { "runs": [{ "text": "no endpoint" }] } } }
                        ] } }
                ]}}}}
            ]}}
        })
        .to_string();
        let shelves = shelves_from_raw(&json);
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].items.len(), 1, "the dead card must be dropped");
        assert_eq!(shelves[0].items[0].title, "playable");
    }

    #[test]
    fn an_unrecognisable_response_yields_no_shelves_rather_than_panicking() {
        // A home feed is a convenience. A shape change should cost the user an
        // empty pane, never an error or a crash.
        assert!(shelves_from_raw("{}").is_empty());
        assert!(shelves_from_raw("not json").is_empty());
        assert!(shelves_from_raw("").is_empty());
    }

    #[test]
    fn a_shelf_whose_cards_all_fail_is_dropped_entirely() {
        let json = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [
                { "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
                    { "musicCarouselShelfRenderer": {
                        "header": { "musicCarouselShelfBasicHeaderRenderer": {
                            "title": { "runs": [{ "text": "All broken" }] } } },
                        "contents": [{ "musicTwoRowItemRenderer": {} }] } }
                ]}}}}
            ]}}
        })
        .to_string();
        assert!(
            shelves_from_raw(&json).is_empty(),
            "a heading over nothing is worse than no shelf"
        );
    }

    #[test]
    fn album_cards_become_albums_without_inventing_a_year() {
        let shelves = shelves_from_raw(FEED);
        let albums = albums_from_shelves(&shelves);
        // The captured feed may or may not carry album cards; assert the
        // properties that must hold either way.
        for a in &albums {
            assert!(!a.title.is_empty());
            // The feed's byline is not a year. Guessing one shows a wrong date.
            assert_eq!(a.year, None, "a year must never be inferred from a byline");
        }
    }

    #[test]
    fn the_same_album_in_two_shelves_is_listed_once() {
        let dup = HomeItem {
            title: "Repeat".into(),
            subtitle: "Someone".into(),
            target: HomeTarget::Album(AlbumId::from("MPREb_same")),
            thumbnail_url: None,
        };
        let shelves = vec![
            HomeShelf {
                title: "One".into(),
                items: vec![dup.clone()],
            },
            HomeShelf {
                title: "Two".into(),
                items: vec![dup],
            },
        ];
        assert_eq!(albums_from_shelves(&shelves).len(), 1);
    }

    #[test]
    fn non_album_cards_are_not_offered_as_albums() {
        let shelves = vec![HomeShelf {
            title: "Mixed".into(),
            items: vec![
                HomeItem {
                    title: "a track".into(),
                    subtitle: String::new(),
                    target: HomeTarget::Track(VideoId::from("v1")),
                    thumbnail_url: None,
                },
                HomeItem {
                    title: "an album".into(),
                    subtitle: String::new(),
                    target: HomeTarget::Album(AlbumId::from("MPREb_1")),
                    thumbnail_url: None,
                },
            ],
        }];
        let albums = albums_from_shelves(&shelves);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "an album");
    }

    /// Page 2 of the same feed. This is where the shelves the web player leads
    /// with actually live.
    const FEED_PAGE2: &str = include_str!("../tests/fixtures/home_feed_page2.json");

    #[test]
    fn a_continuation_page_parses_from_its_own_path() {
        // Continuations put the same shelves under /continuationContents rather
        // than /contents. A parser that only knows the first path silently
        // returns nothing for every page after the first.
        let shelves = shelves_from_raw(FEED_PAGE2);
        assert!(!shelves.is_empty(), "page 2 must parse too");
        let titles: Vec<&str> = shelves.iter().map(|s| s.title.as_str()).collect();
        assert!(
            titles.contains(&"Quick picks"),
            "expected Quick picks on page 2, got {titles:?}"
        );
    }

    #[test]
    fn quick_picks_is_not_on_the_first_page() {
        // Measured live: page 1 is "Listen again" / "From your library" /
        // "Listen together". Anything that reads only page 1 shows the user
        // none of the recommendations they see on the web. If a future capture
        // changes this, the two-page fetch may be simplifiable — but check
        // before assuming.
        let titles: Vec<String> = shelves_from_raw(FEED)
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert!(
            !titles.iter().any(|t| t == "Quick picks"),
            "page 1 unexpectedly held Quick picks: {titles:?}"
        );
    }

    #[test]
    fn the_first_page_offers_a_continuation_token() {
        // Without this the second page is unreachable.
        let raw = include_str!("../tests/fixtures/home_feed.json");
        // The stored fixture has continuations scrubbed out, so assert on the
        // parser's contract with a minimal document instead.
        let _ = raw;
        let json = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [
                { "tabRenderer": { "content": { "sectionListRenderer": {
                    "contents": [],
                    "continuations": [{ "nextContinuationData": { "continuation": "TOKEN123" } }]
                }}}}
            ]}}
        })
        .to_string();
        assert_eq!(continuation_token(&json).as_deref(), Some("TOKEN123"));
    }

    #[test]
    fn a_continuation_can_offer_a_further_token() {
        let json = serde_json::json!({
            "continuationContents": { "sectionListContinuation": {
                "contents": [],
                "continuations": [{ "nextContinuationData": { "continuation": "PAGE3" } }]
            }}
        })
        .to_string();
        assert_eq!(continuation_token(&json).as_deref(), Some("PAGE3"));
    }

    #[test]
    fn a_feed_with_no_more_pages_offers_no_token() {
        assert_eq!(continuation_token("{}"), None);
        assert_eq!(continuation_token("not json"), None);
    }

    #[test]
    fn page_two_carries_playable_tracks() {
        // Quick picks is a track carousel — this is what makes the Home pane
        // useful rather than a list of things to open.
        let tracks = shelves_from_raw(FEED_PAGE2)
            .into_iter()
            .flat_map(|s| s.items)
            .filter(|i| matches!(i.target, HomeTarget::Track(_)))
            .count();
        assert!(tracks > 0, "page 2 should hold playable tracks");
    }
}
