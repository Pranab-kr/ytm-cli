//! Task 9 Step 7's gate, and the fastest way to check auth: real playlist titles
//! printing from the owner's account.
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//!   cargo run -p ytm-core --example dump_playlists            # gate: titles
//!   cargo run -p ytm-core --example dump_playlists -- --raw > /tmp/raw.json
//!
//! SCRUB /tmp/raw.json before committing it as a fixture: remove account ids,
//! emails, and any browseId tied to the owner's channel.

mod common;

use ytm_core::MusicSource;
use ytm_core::ytmusic::YtMusicSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_mode = std::env::args().any(|a| a == "--raw");
    let common::AuthChoice::Cookie(path) = common::auth_choice()?;
    eprintln!("auth: browser cookie ({})", path.display());

    if raw_mode {
        let api = ytmapi_rs::YtMusic::from_cookie_file(&path).await?;
        let json: String = api
            .raw_json_query(ytmapi_rs::query::GetLibraryPlaylistsQuery)
            .await?;
        println!("{json}");
        return Ok(());
    }

    let source = YtMusicSource::from_cookie_file(&path).await?;
    let playlists = source.library_playlists().await?;

    println!("{} playlists:", playlists.len());
    for p in &playlists {
        let count = p
            .track_count
            .map(|c| format!("{c} tracks"))
            .unwrap_or_else(|| "? tracks".into());
        let flag = if p.is_system { " [system]" } else { "" };
        println!("  {} ({count}){flag}", p.title);
    }

    if playlists.is_empty() {
        println!("\nGATE: no playlists returned — the cookie may have expired");
    } else {
        println!("\nGATE: live API OK");
    }
    Ok(())
}
