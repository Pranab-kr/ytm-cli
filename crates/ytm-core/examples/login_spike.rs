//! Manual go/no-go for Task 8 Step 5: does OAuth device-code login actually
//! work against the owner's Google account?
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule
//! because no TUI frame is on screen.
//!
//! Needs an OAuth client of type "TVs and Limited Input devices" from Google
//! Cloud Console, and the `https://www.googleapis.com/auth/youtube` scope added
//! under "Data Access".
//!
//! Credentials come from `~/.config/ytm-cli/config.toml` (see common::load), so
//! the secret stays in one file outside the repo:
//!
//!   cargo run -p ytm-core --example login_spike

mod common;

use ytm_core::auth::{KeyringStore, TokenStore};
use ytm_core::oauth::{begin_device_login, complete_device_login};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Nothing to do when config selects the cookie path — there is no login step.
    let common::Creds {
        client_id,
        client_secret,
    } = match common::auth_choice()? {
        common::AuthChoice::OAuth(c) => c,
        common::AuthChoice::Cookie(path) => {
            println!(
                "config has auth.kind = \"cookie\" ({}), so no OAuth login is needed.\n\
                 Run the dump_playlists example instead.",
                path.display()
            );
            return Ok(());
        }
    };

    let client = ytmapi_rs::Client::new()?;
    let (info, code) = begin_device_login(&client, &client_id).await?;

    println!("1. Open: {}", info.verification_url);
    println!("2. Enter code: {}", info.user_code);
    println!("3. Approve the request, then wait here.\n");
    println!(
        "polling every {}s, giving up after 300s...",
        info.interval_secs
    );

    let store = KeyringStore::default_store();
    match complete_device_login(
        &client,
        code,
        &client_id,
        &client_secret,
        &store,
        info.interval_secs,
        300,
    )
    .await
    {
        Ok(t) => {
            // Debug is redacted by hand, so this cannot leak the token.
            println!("token stored: {t:?}");
            println!("keyring round-trip: {}", store.load()?.is_some());
            println!("\nGATE: login OK");
        }
        Err(e) => {
            println!("\nGATE: login FAILED — {e}");
            return Err(e.into());
        }
    }
    Ok(())
}
