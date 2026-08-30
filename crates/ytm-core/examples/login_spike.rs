//! Manual go/no-go for Task 8 Step 5: does OAuth device-code login actually
//! work against the owner's Google account?
//!
//! `println!` is allowed here — examples are exempt from the no-stdout rule
//! because no TUI frame is on screen.
//!
//! Needs an OAuth client of type "TV and Limited Input" from Google Cloud
//! Console. Pass the credentials via the environment so they never touch git:
//!
//!   YTM_CLIENT_ID=... YTM_CLIENT_SECRET=... \
//!     cargo run -p ytm-core --example login_spike

use ytm_core::auth::{KeyringStore, TokenStore};
use ytm_core::oauth::{begin_device_login, complete_device_login};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = std::env::var("YTM_CLIENT_ID")
        .map_err(|_| "set YTM_CLIENT_ID (see the module docs at the top of this file)")?;
    let client_secret = std::env::var("YTM_CLIENT_SECRET")
        .map_err(|_| "set YTM_CLIENT_SECRET (see the module docs at the top of this file)")?;

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
