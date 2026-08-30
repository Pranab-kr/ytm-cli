//! Domain models, the MusicSource seam, auth, and the metadata cache.
pub mod model;
pub use model::*;
pub mod source;
pub use source::{BoxFut, MusicSource, SourceError};
