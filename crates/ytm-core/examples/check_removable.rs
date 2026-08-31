//! Does FR-C5 work now? Reads a real playlist through `MusicSource` and reports
//! how many tracks came back with a `set_video_id`. Read-only — changes nothing.
//!
//!   cargo run -p ytm-core --example check_removable -- <playlistId>

mod common;

use ytm_core::MusicSource;
use ytm_core::ytmusic::YtMusicSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args()
        .nth(1)
        .ok_or("usage: check_removable <playlistId>")?;

    let common::AuthChoice::Cookie(path) = common::auth_choice()?;
    let src = YtMusicSource::from_cookie_file(&path).await?;
    let tracks = src.playlist_tracks(id.as_str().into()).await?;

    let removable = tracks.iter().filter(|t| t.is_removable()).count();
    println!("{} tracks, {removable} removable", tracks.len());
    for t in tracks.iter().take(3) {
        println!("  {} -> set_video_id={:?}", t.title, t.set_video_id);
    }
    if removable == tracks.len() && !tracks.is_empty() {
        println!("FR-C5: OK");
    } else {
        println!("FR-C5: INCOMPLETE");
    }
    Ok(())
}
