//! The live MusicSource. Wraps ytmapi-rs; the only file besides mapping.rs that
//! calls it.

use crate::{mapping, model::*, source::*};
use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::{BrowserToken, LoggedIn, OAuthToken};
use ytmapi_rs::common::{ApiOutcome, YoutubeID};
use ytmapi_rs::parse::SearchResultPlaylist;
use ytmapi_rs::query::playlist::PrivacyStatus;
use ytmapi_rs::query::{CreatePlaylistQuery, EditPlaylistQuery};

/// Generic over the auth token type so OAuth and cookie auth share one impl.
pub struct YtMusicSource<A: LoggedIn> {
    api: YtMusic<A>,
}

impl YtMusicSource<BrowserToken> {
    /// Cookie fallback (FR-A5).
    pub async fn from_cookie_file(path: impl AsRef<std::path::Path>) -> Result<Self, SourceError> {
        let api = YtMusic::from_cookie_file(path).await.map_err(classify)?;
        Ok(Self { api })
    }
}

impl YtMusicSource<OAuthToken> {
    /// Primary path (FR-A1..A3). The token comes from `oauth::complete_device_login`.
    pub fn from_oauth(token: OAuthToken) -> Self {
        Self {
            api: YtMusic::from_auth_token(token),
        }
    }
}

/// Upstream errors are opaque strings; classify them into our variants so the
/// UI can show a sentence (NFR-9). Refine the substrings against real failures.
fn classify(e: ytmapi_rs::Error) -> SourceError {
    let s = e.to_string();
    let l = s.to_lowercase();
    if l.contains("401") || l.contains("unauthor") {
        SourceError::NotAuthenticated
    } else if l.contains("429") || l.contains("rate") {
        SourceError::RateLimited
    } else if l.contains("404") || l.contains("not found") {
        SourceError::NotFound(s)
    } else if l.contains("parse") || l.contains("navigation") {
        SourceError::Parse(s)
    } else {
        SourceError::Network(s)
    }
}

/// Run upstream's typed parse over a response we already hold.
///
/// `raw_json_query` + this is equivalent to the typed call but lets us inspect
/// the JSON first — needed because upstream's parsers fail on an empty library
/// section instead of yielding an empty list. `ProcessedResult`'s fields are
/// public and `parse_into` runs on JSON in hand, so this costs no extra request.
fn parse_json<Q, O>(query: &Q, json: String) -> Result<O, SourceError>
where
    O: ytmapi_rs::parse::ParseFrom<Q>,
{
    let value: ytmapi_rs::json::Json =
        serde_json::from_str(&json).map_err(|e| SourceError::Parse(e.to_string()))?;
    ytmapi_rs::parse::ProcessedResult {
        query,
        source: json,
        json: value,
    }
    .parse_into()
    .map_err(classify)
}

/// YouTube answers a mutation with an outcome rather than an HTTP error, so a
/// silent `Failure` would look like success to the UI.
fn check_outcome(o: ApiOutcome) -> Result<(), SourceError> {
    match o {
        ApiOutcome::Success => Ok(()),
        ApiOutcome::Failure => Err(SourceError::Other("YouTube rejected the change".to_owned())),
    }
}

fn to_privacy_status(p: Privacy) -> PrivacyStatus {
    match p {
        Privacy::Public => PrivacyStatus::Public,
        Privacy::Private => PrivacyStatus::Private,
        Privacy::Unlisted => PrivacyStatus::Unlisted,
    }
}

/// YouTube Music's home feed (FR-B6).
///
/// `ytmapi-rs` 0.3.3 exposes no home query, but `Query`/`PostQuery` are public
/// and documented as user-implementable, so `browseId: FEmusic_home` is
/// reachable without forking upstream. Verified live on 2026-08-31.
///
/// The output is deliberately raw: upstream's parsers cannot help with a feed it
/// has no types for, so `home_feed::shelves_from_raw` does the work and this
/// type exists only to satisfy the trait bound.
#[derive(Debug)]
pub struct HomeRaw;

impl ytmapi_rs::parse::ParseFrom<GetHomeQuery> for HomeRaw {
    fn parse_from(_: ytmapi_rs::parse::ProcessedResult<GetHomeQuery>) -> ytmapi_rs::Result<Self> {
        // Never called: we always go through `raw_json_query`.
        Ok(HomeRaw)
    }
}

/// The `FEmusic_home` browse query.
#[derive(Debug, Clone)]
pub struct GetHomeQuery;

impl<A: LoggedIn> ytmapi_rs::query::Query<A> for GetHomeQuery {
    type Output = HomeRaw;
    type Method = ytmapi_rs::query::PostMethod;
}

impl ytmapi_rs::query::PostQuery for GetHomeQuery {
    fn header(&self) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::from_iter([("browseId".to_string(), serde_json::json!("FEmusic_home"))])
    }
    fn params(&self) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        vec![]
    }
    fn path(&self) -> &str {
        "browse"
    }
}

/// The next page of the home feed.
///
/// Needed because the first page is not the useful one: measured live, page 1 is
/// "Listen again" / "From your library" / "Listen together", while page 2 holds
/// "Quick picks", "Covers and remixes", and "Heard in Shorts" — the shelves the
/// web player leads with.
#[derive(Debug, Clone)]
pub struct GetHomeContinuationQuery(pub String);

impl ytmapi_rs::parse::ParseFrom<GetHomeContinuationQuery> for HomeRaw {
    fn parse_from(
        _: ytmapi_rs::parse::ProcessedResult<GetHomeContinuationQuery>,
    ) -> ytmapi_rs::Result<Self> {
        Ok(HomeRaw)
    }
}

impl<A: LoggedIn> ytmapi_rs::query::Query<A> for GetHomeContinuationQuery {
    type Output = HomeRaw;
    type Method = ytmapi_rs::query::PostMethod;
}

impl ytmapi_rs::query::PostQuery for GetHomeContinuationQuery {
    fn header(&self) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::from_iter([("browseId".to_string(), serde_json::json!("FEmusic_home"))])
    }
    fn params(&self) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        vec![
            ("continuation", std::borrow::Cow::from(self.0.as_str())),
            ("type", std::borrow::Cow::from("next")),
        ]
    }
    fn path(&self) -> &str {
        "browse"
    }
}

/// How many home-feed pages to walk.
///
/// Page 1 carries none of the shelves the web player leads with, so one page is
/// not enough; three pages is where the returns flatten and each page is a
/// round trip the user waits on. Measured live 2026-08-31.
const HOME_PAGES: usize = 3;

macro_rules! impl_feed {
    ($token:ty) => {
        impl YtMusicSource<$token> {
            /// Walk the home feed, following continuations (FR-B6).
            ///
            /// Errors on the *first* page propagate — that is a real failure to
            /// reach YouTube. A later page failing is not worth losing the
            /// shelves already in hand, so it just stops the walk.
            async fn feed_shelves(&self) -> Result<Vec<HomeShelf>, SourceError> {
                let first = self
                    .api
                    .raw_json_query::<GetHomeQuery>(&GetHomeQuery)
                    .await
                    .map_err(classify)?;
                let mut shelves = crate::home_feed::shelves_from_raw(&first);
                let mut token = crate::home_feed::continuation_token(&first);

                for _ in 1..HOME_PAGES {
                    let Some(t) = token.take() else { break };
                    let q = GetHomeContinuationQuery(t);
                    let Ok(page) = self
                        .api
                        .raw_json_query::<GetHomeContinuationQuery>(&q)
                        .await
                    else {
                        break;
                    };
                    shelves.extend(crate::home_feed::shelves_from_raw(&page));
                    token = crate::home_feed::continuation_token(&page);
                }
                Ok(shelves)
            }
        }
    };
}

impl_feed!(BrowserToken);
impl_feed!(OAuthToken);

/// One `MusicSource` impl per concrete token type.
///
/// This cannot be a single `impl<A: LoggedIn>`: upstream's
/// `AuthToken::headers` returns an opaque `impl IntoIterator` with no `Send`
/// bound, so `Send` is unprovable through a generic `A` and the `BoxFut` cast
/// fails. With a concrete `A`, auto-trait leakage supplies `Send`.
macro_rules! impl_music_source {
    ($token:ty) => {
        impl MusicSource for YtMusicSource<$token> {
            fn library_playlists(&self) -> BoxFut<'_, Vec<Playlist>> {
                Box::pin(async move {
                    let raw = self.api.get_library_playlists().await.map_err(classify)?;
                    Ok(raw.iter().map(mapping::playlist_from_library).collect())
                })
            }

            fn library_songs(&self) -> BoxFut<'_, Vec<Track>> {
                Box::pin(async move {
                    let raw = self.api.get_library_songs().await.map_err(classify)?;
                    Ok(raw.iter().map(mapping::track_from_table_list).collect())
                })
            }

            fn library_albums(&self) -> BoxFut<'_, Vec<Album>> {
                Box::pin(async move {
                    // Parsed from raw JSON rather than via `get_library_albums`
                    // because upstream cannot parse an *empty* library: with no
                    // saved albums YouTube sends a `messageRenderer` ("No albums
                    // yet") where the parser demands a `gridRenderer`, and the
                    // failure reached the user as an error toast on a pane that
                    // should just have read empty (FR-B3). Measured against the
                    // live account 2026-08-31; fixture in library_raw.rs.
                    let query = ytmapi_rs::query::GetLibraryAlbumsQuery::default();
                    let json = self
                        .api
                        .raw_json_query::<ytmapi_rs::query::GetLibraryAlbumsQuery>(&query)
                        .await
                        .map_err(classify)?;
                    if crate::library_raw::is_empty_library(&json) {
                        return Ok(Vec::new());
                    }
                    let raw: Vec<ytmapi_rs::parse::SearchResultAlbum> = parse_json(&query, json)?;
                    Ok(raw.iter().map(mapping::album_from_search).collect())
                })
            }

            fn library_artists(&self) -> BoxFut<'_, Vec<Artist>> {
                Box::pin(async move {
                    // Same empty-library hazard as albums: an artist list that
                    // empties would fail to parse rather than render empty.
                    let query = ytmapi_rs::query::GetLibraryArtistsQuery::default();
                    let json = self
                        .api
                        .raw_json_query::<ytmapi_rs::query::GetLibraryArtistsQuery>(&query)
                        .await
                        .map_err(classify)?;
                    if crate::library_raw::is_empty_library(&json) {
                        return Ok(Vec::new());
                    }
                    let raw: Vec<ytmapi_rs::parse::LibraryArtist> = parse_json(&query, json)?;
                    Ok(raw.iter().map(mapping::artist_from_library).collect())
                })
            }

            fn home_shelves(&self) -> BoxFut<'_, Vec<HomeShelf>> {
                Box::pin(async move { self.feed_shelves().await })
            }

            fn recommended_albums(&self) -> BoxFut<'_, Vec<Album>> {
                Box::pin(async move {
                    // Most accounts save no albums, so the library pane is a dead
                    // end. The feed's album cards fill it instead.
                    Ok(crate::home_feed::albums_from_shelves(
                        &self.feed_shelves().await?,
                    ))
                })
            }

            fn artist_tracks(&self, id: ArtistId) -> BoxFut<'_, Vec<Track>> {
                Box::pin(async move {
                    let raw = self
                        .api
                        .get_artist(ytmapi_rs::common::ArtistChannelID::from_raw(id.as_str()))
                        .await
                        .map_err(classify)?;
                    // `top_releases.songs` is the artist page's song shelf. An
                    // artist with no shelf yields an empty list rather than an
                    // error — nothing is broken, there is just nothing to play.
                    Ok(raw
                        .top_releases
                        .songs
                        .map(|s| {
                            s.results
                                .iter()
                                .map(mapping::track_from_artist_song)
                                .collect()
                        })
                        .unwrap_or_default())
                })
            }

            fn playlist_tracks(&self, id: PlaylistId) -> BoxFut<'_, Vec<Track>> {
                Box::pin(async move {
                    // One request, parsed twice: upstream's typed parse for the
                    // track data, and our own pass for `setVideoId`, which
                    // `ytmapi-rs` 0.3.3 discards but removal requires (FR-C5).
                    // `ProcessedResult`'s fields are public and `parse_into`
                    // runs on JSON we already hold, so this costs no extra
                    // round trip. Verified against the crate source.
                    // Browse endpoint: VL-prefixed form (see PlaylistId's docs).
                    let browse_id = id.browse_form();
                    let query = ytmapi_rs::query::GetPlaylistTracksQuery::new(
                        ytmapi_rs::common::PlaylistID::from_raw(&browse_id),
                    );
                    // Turbofished: `impl Borrow<Q>` cannot infer Q from a reference.
                    let json = self
                        .api
                        .raw_json_query::<ytmapi_rs::query::GetPlaylistTracksQuery>(&query)
                        .await
                        .map_err(classify)?;

                    let value: ytmapi_rs::json::Json = serde_json::from_str(&json)
                        .map_err(|e| SourceError::Parse(e.to_string()))?;
                    let items: Vec<ytmapi_rs::parse::PlaylistItem> =
                        ytmapi_rs::parse::ProcessedResult {
                            query: &query,
                            source: json.clone(),
                            json: value,
                        }
                        .parse_into()
                        .map_err(classify)?;

                    // Episodes (podcasts) are out of scope and map to None.
                    let tracks: Vec<Track> = items
                        .iter()
                        .filter_map(mapping::track_from_playlist_item)
                        .collect();
                    // Paired by videoId, not position: upstream returned 83
                    // tracks for an 85-row shelf, so it drops rows internally
                    // and nothing positional can line up. Measured live.
                    let rows = crate::playlist_raw::entry_ids_from_raw(&json);
                    Ok(crate::playlist_raw::attach_entry_ids(tracks, &rows))
                })
            }

            fn playlist_details(&self, id: PlaylistId) -> BoxFut<'_, Playlist> {
                Box::pin(async move {
                    // Browse endpoint: VL-prefixed form.
                    let browse_id = id.browse_form();
                    let raw = self
                        .api
                        .get_playlist_details(ytmapi_rs::common::PlaylistID::from_raw(&browse_id))
                        .await
                        .map_err(classify)?;
                    Ok(mapping::playlist_from_details(&raw))
                })
            }

            fn search_songs(&self, query: String) -> BoxFut<'_, Vec<Track>> {
                Box::pin(async move {
                    let raw = self
                        .api
                        .search_songs(query.as_str())
                        .await
                        .map_err(classify)?;
                    Ok(raw.iter().map(mapping::track_from_search_song).collect())
                })
            }

            fn search_albums(&self, query: String) -> BoxFut<'_, Vec<Album>> {
                Box::pin(async move {
                    let raw = self
                        .api
                        .search_albums(query.as_str())
                        .await
                        .map_err(classify)?;
                    Ok(raw.iter().map(mapping::album_from_search).collect())
                })
            }

            fn search_artists(&self, query: String) -> BoxFut<'_, Vec<Artist>> {
                Box::pin(async move {
                    let raw = self
                        .api
                        .search_artists(query.as_str())
                        .await
                        .map_err(classify)?;
                    Ok(raw.iter().map(mapping::artist_from_search).collect())
                })
            }

            fn search_playlists(&self, query: String) -> BoxFut<'_, Vec<Playlist>> {
                Box::pin(async move {
                    let raw = self
                        .api
                        .search_playlists(query.as_str())
                        .await
                        .map_err(classify)?;
                    // Podcast results are out of scope.
                    Ok(raw
                        .iter()
                        .filter_map(|p| match p {
                            SearchResultPlaylist::Featured(f) => {
                                Some(mapping::playlist_from_search_featured(f))
                            }
                            SearchResultPlaylist::Community(c) => {
                                Some(mapping::playlist_from_search_community(c))
                            }
                            _ => None,
                        })
                        .collect())
                })
            }

            fn create_playlist(
                &self,
                title: String,
                description: Option<String>,
                privacy: Privacy,
            ) -> BoxFut<'_, PlaylistId> {
                Box::pin(async move {
                    let query = CreatePlaylistQuery::new(
                        title.as_str(),
                        description.as_deref(),
                        to_privacy_status(privacy),
                    );
                    let id = self.api.create_playlist(query).await.map_err(classify)?;
                    Ok(PlaylistId(id.get_raw().to_owned()))
                })
            }

            fn edit_playlist(
                &self,
                id: PlaylistId,
                new_title: Option<String>,
                new_description: Option<String>,
                new_privacy: Option<Privacy>,
            ) -> BoxFut<'_, ()> {
                Box::pin(async move {
                    if mapping::is_system_playlist(id.as_str()) {
                        return Err(SourceError::NotEditable(id.to_string()));
                    }
                    // Mutation endpoint: bare form. The VL form is answered with
                    // 400 INVALID_ARGUMENT.
                    let raw_id = id.mutation_form();
                    let pid = ytmapi_rs::common::PlaylistID::from_raw(&raw_id);
                    // EditPlaylistQuery is built per-change upstream; issue one call per
                    // field the caller actually wants changed.
                    if let Some(t) = new_title.as_deref() {
                        let mut q = EditPlaylistQuery::new_title(pid.clone(), t);
                        if let Some(d) = new_description.as_deref() {
                            q = q.with_new_description(d);
                        }
                        check_outcome(self.api.edit_playlist(q).await.map_err(classify)?)?;
                    } else if let Some(d) = new_description.as_deref() {
                        let q = EditPlaylistQuery::new_description(pid.clone(), d);
                        check_outcome(self.api.edit_playlist(q).await.map_err(classify)?)?;
                    }
                    if let Some(p) = new_privacy {
                        let q = EditPlaylistQuery::new_privacy_status(pid, to_privacy_status(p));
                        check_outcome(self.api.edit_playlist(q).await.map_err(classify)?)?;
                    }
                    Ok(())
                })
            }

            fn delete_playlist(&self, id: PlaylistId) -> BoxFut<'_, ()> {
                Box::pin(async move {
                    if mapping::is_system_playlist(id.as_str()) {
                        return Err(SourceError::NotEditable(id.to_string()));
                    }
                    // Mutation endpoint: bare form.
                    let raw_id = id.mutation_form();
                    self.api
                        .delete_playlist(ytmapi_rs::common::PlaylistID::from_raw(&raw_id))
                        .await
                        .map_err(classify)
                })
            }

            fn add_tracks(&self, id: PlaylistId, videos: Vec<VideoId>) -> BoxFut<'_, ()> {
                Box::pin(async move {
                    if mapping::is_system_playlist(id.as_str()) {
                        return Err(SourceError::NotEditable(id.to_string()));
                    }
                    let ids: Vec<_> = videos
                        .iter()
                        .map(|v| ytmapi_rs::common::VideoID::from_raw(v.as_str()))
                        .collect();
                    // Mutation endpoint: bare form.
                    let raw_id = id.mutation_form();
                    self.api
                        .add_video_items_to_playlist(
                            ytmapi_rs::common::PlaylistID::from_raw(&raw_id),
                            ids,
                        )
                        .await
                        .map_err(classify)?;
                    Ok(())
                })
            }

            /// Needs `SetVideoId`, not `VideoId` — see the doc comment on `SetVideoId`.
            fn remove_tracks(&self, id: PlaylistId, entries: Vec<SetVideoId>) -> BoxFut<'_, ()> {
                Box::pin(async move {
                    // Refuse before the API call: a system playlist can never be edited.
                    if mapping::is_system_playlist(id.as_str()) {
                        return Err(SourceError::NotEditable(id.to_string()));
                    }
                    let ids: Vec<_> = entries
                        .iter()
                        .map(|s| ytmapi_rs::common::SetVideoID::from_raw(s.as_str()))
                        .collect();
                    // Mutation endpoint: bare form.
                    let raw_id = id.mutation_form();
                    self.api
                        .remove_playlist_items(
                            ytmapi_rs::common::PlaylistID::from_raw(&raw_id),
                            ids,
                        )
                        .await
                        .map_err(classify)
                })
            }
        }
    };
}

impl_music_source!(BrowserToken);
impl_music_source!(OAuthToken);
