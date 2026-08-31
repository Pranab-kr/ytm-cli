//! Domain models, the MusicSource seam, and the metadata cache.
//!
//! Auth is browser cookies only. The OAuth device flow was removed on
//! 2026-08-31: Google stopped honouring device-flow tokens on the InnerTube
//! endpoints this app uses, so it could not work, and correct-looking code that
//! always fails is worse than no code.
pub mod model;
pub use model::*;
pub mod source;
pub use source::{BoxFut, MusicSource, SourceError};
pub mod cache;
pub mod home_feed;
pub mod library_raw;
pub mod mapping;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod playlist_raw;
pub mod ytmusic;
