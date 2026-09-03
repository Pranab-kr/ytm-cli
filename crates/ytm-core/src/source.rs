//! The seam between the app and YouTube Music. Everything fragile lives behind
//! this trait so a breaking upstream change is contained to one impl.

use crate::model::*;
use std::future::Future;
use std::pin::Pin;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = Result<T, SourceError>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("sign in by setting auth.cookie_file in config.toml (see the README)")]
    NotAuthenticated,

    #[error("sign-in expired — re-export cookies and set auth.cookie_file in config.toml")]
    TokenRefreshFailed,

    #[error("too many requests — YouTube is rate limiting; try again in a minute")]
    RateLimited,

    #[error("network problem: {0}")]
    Network(String),

    #[error("YouTube sent something unexpected ({0}) — the API may have changed")]
    Parse(String),

    #[error("the playlist \"{0}\" cannot be edited")]
    NotEditable(String),

    #[error("{0} was not found")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

/// Read and write access to the user's YouTube Music account. Object-safe on
/// purpose: the UI holds `Arc<dyn MusicSource>` so it can be swapped for
/// `MockSource` in tests with no network.
pub trait MusicSource: Send + Sync {
    fn is_authenticated(&self) -> bool {
        true
    }

    fn library_playlists(&self) -> BoxFut<'_, Vec<Playlist>>;
    fn library_songs(&self) -> BoxFut<'_, Vec<Track>>;
    fn library_albums(&self) -> BoxFut<'_, Vec<Album>>;
    fn library_artists(&self) -> BoxFut<'_, Vec<Artist>>;

    /// YouTube Music's home feed — the recommendation carousels (FR-B6).
    /// `ytmapi-rs` has no home query; ours is hand-rolled on its public query
    /// traits. An unreadable feed is an empty list, not an error.
    fn home_shelves(&self) -> BoxFut<'_, Vec<HomeShelf>>;

    /// Albums YouTube recommends, for when the user has saved none (FR-B3).
    /// The library albums pane is empty for most accounts, so these come from the
    /// home feed's album cards instead.
    fn recommended_albums(&self) -> BoxFut<'_, Vec<Album>>;

    /// An artist's top tracks, so the Artists pane can do something on Enter
    /// (FR-B7). Upstream's `GetArtistQuery` returns these as `top_releases`.
    fn artist_tracks(&self, id: ArtistId) -> BoxFut<'_, Vec<Track>>;

    /// An album's songs, so album rows open onto tracks like playlists do.
    /// Upstream's `GetAlbumQuery` returns these with header-level artists/art.
    fn album_tracks(&self, id: AlbumId) -> BoxFut<'_, Vec<Track>>;

    fn playlist_tracks(&self, id: PlaylistId) -> BoxFut<'_, Vec<Track>>;
    fn playlist_details(&self, id: PlaylistId) -> BoxFut<'_, Playlist>;

    fn search_songs(&self, query: String) -> BoxFut<'_, Vec<Track>>;
    fn search_albums(&self, query: String) -> BoxFut<'_, Vec<Album>>;
    fn search_artists(&self, query: String) -> BoxFut<'_, Vec<Artist>>;
    fn search_playlists(&self, query: String) -> BoxFut<'_, Vec<Playlist>>;

    fn create_playlist(
        &self,
        title: String,
        description: Option<String>,
        privacy: Privacy,
    ) -> BoxFut<'_, PlaylistId>;

    fn edit_playlist(
        &self,
        id: PlaylistId,
        new_title: Option<String>,
        new_description: Option<String>,
        new_privacy: Option<Privacy>,
    ) -> BoxFut<'_, ()>;

    fn delete_playlist(&self, id: PlaylistId) -> BoxFut<'_, ()>;

    fn add_tracks(&self, id: PlaylistId, videos: Vec<VideoId>) -> BoxFut<'_, ()>;

    /// Needs `SetVideoId`, not `VideoId` — see the doc comment on `SetVideoId`.
    fn remove_tracks(&self, id: PlaylistId, entries: Vec<SetVideoId>) -> BoxFut<'_, ()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_error_messages_are_human_readable() {
        // NFR-9: errors reach the user as sentences, never Debug dumps.
        let e = SourceError::NotAuthenticated;
        assert_eq!(
            e.to_string(),
            "sign in by setting auth.cookie_file in config.toml (see the README)"
        );

        let e = SourceError::TokenRefreshFailed;
        assert_eq!(
            e.to_string(),
            "sign-in expired — re-export cookies and set auth.cookie_file in config.toml"
        );

        let e = SourceError::RateLimited;
        assert!(e.to_string().contains("too many requests"));

        let e = SourceError::NotEditable("Your Likes".into());
        assert_eq!(
            e.to_string(),
            "the playlist \"Your Likes\" cannot be edited"
        );
    }

    #[test]
    fn trait_object_is_usable_behind_arc() {
        // The UI holds Arc<dyn MusicSource>; this must compile.
        fn assert_object_safe(_: std::sync::Arc<dyn MusicSource>) {}
        let _ = assert_object_safe as fn(_);
    }
}
