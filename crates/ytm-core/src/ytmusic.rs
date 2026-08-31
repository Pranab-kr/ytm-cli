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
                    let raw = self.api.get_library_albums().await.map_err(classify)?;
                    Ok(raw.iter().map(mapping::album_from_search).collect())
                })
            }

            fn library_artists(&self) -> BoxFut<'_, Vec<Artist>> {
                Box::pin(async move {
                    let raw = self.api.get_library_artists().await.map_err(classify)?;
                    Ok(raw.iter().map(mapping::artist_from_library).collect())
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
