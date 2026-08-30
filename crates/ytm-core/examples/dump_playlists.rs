//! Task 9 Step 1: capture a real `GetLibraryPlaylistsQuery` response to use as
//! a test fixture, and Step 7's Phase 2 gate — real playlist titles printing
//! from the owner's account.
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule.
//!
//! Credentials come from `~/.config/ytm-cli/config.toml` (see common::load).
//! Run after `login_spike` has stored a token:
//!
//!   cargo run -p ytm-core --example dump_playlists            # gate: titles
//!   cargo run -p ytm-core --example dump_playlists -- --raw > /tmp/raw.json
//!
//! SCRUB /tmp/raw.json before committing it as a fixture: remove account ids,
//! emails, and any browseId tied to the owner's channel.

mod common;

use ytm_core::MusicSource;
use ytm_core::auth::{KeyringStore, TokenStore};
use ytm_core::oauth::oauth_token_from_stored;
use ytm_core::ytmusic::YtMusicSource;

/// Both auth paths implement MusicSource; box them so the rest of the example
/// does not care which one config chose.
fn boxed<S: MusicSource + 'static>(s: S) -> Box<dyn MusicSource> {
    Box::new(s)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_mode = std::env::args().any(|a| a == "--raw");

    let source: Box<dyn MusicSource> = match common::auth_choice()? {
        common::AuthChoice::Cookie(path) => {
            eprintln!("auth: browser cookie ({})", path.display());
            if raw_mode {
                let api = ytmapi_rs::YtMusic::from_cookie_file(&path).await?;
                let json: String = api
                    .raw_json_query(ytmapi_rs::query::GetLibraryPlaylistsQuery)
                    .await?;
                println!("{json}");
                return Ok(());
            }
            boxed(YtMusicSource::from_cookie_file(&path).await?)
        }
        common::AuthChoice::OAuth(common::Creds {
            client_id,
            client_secret,
        }) => {
            eprintln!("auth: oauth (keyring token)");
            let stored = KeyringStore::default_store()
                .load()?
                .ok_or("no token in the keyring — run the login_spike example first")?;
            let token = oauth_token_from_stored(&stored, &client_id, &client_secret)?;
            if raw_mode {
                // Raw JSON for the fixture. Goes to stdout so it can be redirected.
                let api = ytmapi_rs::YtMusic::from_auth_token(token);
                let json: String = api
                    .raw_json_query(ytmapi_rs::query::GetLibraryPlaylistsQuery)
                    .await?;
                println!("{json}");
                return Ok(());
            }
            boxed(YtMusicSource::from_oauth(token))
        }
    };

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
        println!("\nGATE: no playlists returned — auth may have succeeded with an empty library");
    } else {
        println!("\nGATE: live API OK");
    }
    Ok(())
}
