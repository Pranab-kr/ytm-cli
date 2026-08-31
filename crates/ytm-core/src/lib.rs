//! Domain models, the MusicSource seam, auth, and the metadata cache.
pub mod model;
pub use model::*;
pub mod source;
pub use source::{BoxFut, MusicSource, SourceError};
pub mod auth;
pub mod cache;
pub mod library_raw;
pub mod mapping;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod oauth;
pub mod playlist_raw;
pub mod ytmusic;
