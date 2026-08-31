//! Manual check of the three new source methods against the live account:
//! the home feed (FR-B6), recommended albums (FR-B3), and an artist's top
//! tracks (FR-B7).
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//!   cargo run -p ytm-core --example home_spike

mod common;

use ytm_core::MusicSource;
use ytm_core::ytmusic::YtMusicSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let common::AuthChoice::Cookie(path) = common::auth_choice()? else {
        return Err("cookie auth only".into());
    };
    let source = YtMusicSource::from_cookie_file(&path).await?;

    println!("=== home shelves ===");
    for s in source.home_shelves().await? {
        println!("  {} ({} items)", s.title, s.items.len());
        for i in s.items.iter().take(3) {
            println!("      [{}] {} — {}", i.kind_label(), i.title, i.subtitle);
        }
    }

    println!("\n=== recommended albums ===");
    let albums = source.recommended_albums().await?;
    println!("  {} albums", albums.len());
    for a in albums.iter().take(5) {
        println!("      {} — {}", a.title, a.artists.join(", "));
    }

    println!("\n=== artist tracks ===");
    let artists = source.library_artists().await?;
    if let Some(a) = artists.first() {
        let tracks = source.artist_tracks(a.id.clone()).await?;
        println!("  {} -> {} tracks", a.name, tracks.len());
        for t in tracks.iter().take(5) {
            println!("      {} — {}", t.title, t.artists.join(", "));
        }
    }
    Ok(())
}
