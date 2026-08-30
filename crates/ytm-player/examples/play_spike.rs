//! Proves the whole audio path: yt-dlp -> mpv -> speakers.
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//! Run: cargo run -p ytm-player --example play_spike -- <videoId>

use ytm_core::VideoId;
use ytm_player::{mpv_backend::MpvHandle, resolver::StreamResolver};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dQw4w9WgXcQ".to_owned());
    let id = VideoId::from(id.as_str());

    let resolver = StreamResolver::new();
    println!("resolving {id} ...");
    let url = resolver.resolve(&id).await?;
    println!("resolved: {}...", &url[..60.min(url.len())]);

    let h = MpvHandle::new()?;
    h.set_volume(60)?;
    h.load(&url)?;
    println!("playing 10s ...");

    let start = std::time::Instant::now();
    let mut progressed = false;
    while start.elapsed().as_secs() < 10 {
        if let Some(Ok(ev)) = h.poll_event(0.5) {
            println!("event: {ev:?}");
        }
        if let (Some(p), Some(d)) = (h.position(), h.duration()) {
            println!("  {p}s / {d}s");
            if p > 0 {
                progressed = true;
            }
        }
    }
    if progressed {
        println!("\nGATE: mpv advanced the playback clock. Confirm you HEARD audio.");
    } else {
        println!("\nGATE: FAILED — the clock never advanced past 0s.");
    }
    Ok(())
}
