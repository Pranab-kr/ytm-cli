//! Google OAuth device-code flow (FR-A1..A3).
//!
//! The user must create their own OAuth client of type "TV and Limited Input"
//! in Google Cloud Console; see README. Google does not treat the device-flow
//! client secret as confidential, so it may live in config.toml.

use crate::auth::{StoredToken, TokenStore};
use ytmapi_rs::Client;
use ytmapi_rs::auth::OAuthTokenGenerator;
use ytmapi_rs::auth::oauth::OAuthDeviceCode;

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error(
        "no OAuth client configured — set `auth.client_id` and `auth.client_secret` in config.toml (see README: Google Cloud setup)"
    )]
    MissingCredentials,
    #[error("Google rejected the sign-in: {0}")]
    Rejected(String),
    #[error("sign-in timed out — the code expired before it was authorized")]
    TimedOut,
    #[error("could not store credentials: {0}")]
    Store(#[from] crate::auth::TokenStoreError),
    #[error("network problem during sign-in: {0}")]
    Network(String),
}

/// Never poll Google faster than this, whatever it suggests.
const MIN_POLL_INTERVAL_SECS: u64 = 5;

/// What the login pane shows the user.
#[derive(Debug, Clone)]
pub struct DeviceCodeInfo {
    pub user_code: String,
    pub verification_url: String,
    /// Never poll faster than this; Google rate limits.
    pub interval_secs: u64,
}

/// Step 1: get a code to show the user. Returns the display info plus the
/// opaque code to hand back to `complete_device_login`.
///
/// Uses `OAuthTokenGenerator` rather than `generate_oauth_code_and_url`: the
/// latter discards `user_code` and `interval`, which the login pane needs.
pub async fn begin_device_login(
    client: &Client,
    client_id: &str,
) -> Result<(DeviceCodeInfo, OAuthDeviceCode), OAuthError> {
    if client_id.is_empty() {
        return Err(OAuthError::MissingCredentials);
    }
    let generator = OAuthTokenGenerator::new(client, client_id)
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    let info = DeviceCodeInfo {
        user_code: generator.user_code,
        // The bare verification_url; the user types the code shown above.
        verification_url: generator.verification_url,
        interval_secs: (generator.interval as u64).max(MIN_POLL_INTERVAL_SECS),
    };
    Ok((info, generator.device_code))
}

/// Step 2: poll until the user authorizes, then persist.
///
/// `generate_oauth_token` errors until authorization completes, so retry on the
/// interval until `deadline_secs` elapses.
pub async fn complete_device_login(
    client: &Client,
    code: OAuthDeviceCode,
    client_id: &str,
    client_secret: &str,
    store: &dyn TokenStore,
    interval_secs: u64,
    deadline_secs: u64,
) -> Result<StoredToken, OAuthError> {
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(OAuthError::MissingCredentials);
    }
    let start = std::time::Instant::now();
    loop {
        match ytmapi_rs::generate_oauth_token(client, code.clone(), client_id, client_secret).await
        {
            Ok(tok) => {
                // OAuthToken's fields are private (verified in the 0.3.3
                // source) but it derives Serialize, so go through JSON rather
                // than guessing at accessors.
                let json =
                    serde_json::to_value(&tok).map_err(|e| OAuthError::Rejected(e.to_string()))?;
                let access = json["access_token"].as_str().unwrap_or_default().to_owned();
                let refresh = json["refresh_token"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let expires_in = json["expires_in"].as_i64().unwrap_or(3600);
                let ttype = json["token_type"].as_str().unwrap_or("Bearer").to_owned();
                let now = chrono::Utc::now().timestamp();
                return persist_token(store, access, refresh, expires_in, ttype, now)
                    .map_err(OAuthError::from);
            }
            Err(_) if start.elapsed().as_secs() < deadline_secs => {
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            }
            Err(_) => return Err(OAuthError::TimedOut),
        }
    }
}

/// Convert Google's relative `expires_in` into an absolute instant and store it.
pub fn persist_token(
    store: &dyn TokenStore,
    access_token: String,
    refresh_token: String,
    expires_in_secs: i64,
    token_type: String,
    now_unix: i64,
) -> Result<StoredToken, crate::auth::TokenStoreError> {
    let t = StoredToken {
        access_token,
        refresh_token,
        expires_at: now_unix + expires_in_secs,
        token_type,
    };
    store.save(&t)?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;

    #[test]
    fn device_code_info_carries_what_the_ui_must_display() {
        // FR-A1: the login pane needs a code and a URL to show the user.
        let i = DeviceCodeInfo {
            user_code: "ABCD-EFGH".into(),
            verification_url: "https://google.com/device".into(),
            interval_secs: 5,
        };
        assert_eq!(i.user_code, "ABCD-EFGH");
        assert!(i.verification_url.starts_with("https://"));
        assert!(
            i.interval_secs >= 5,
            "polling faster than 5s risks a rate limit"
        );
    }

    #[test]
    fn persist_stores_token_and_computes_absolute_expiry() {
        let store = MemoryStore::default();
        let now = 1_000_000;
        persist_token(&store, "at".into(), "rt".into(), 3600, "Bearer".into(), now).unwrap();
        let got = store.load().unwrap().unwrap();
        assert_eq!(
            got.expires_at,
            now + 3600,
            "expires_in is relative; we store absolute"
        );
        assert!(!got.is_expired_at(now));
    }

    #[test]
    fn missing_client_id_is_an_actionable_error() {
        // FR-A6: name the config key, don't dump an error.
        let e = OAuthError::MissingCredentials;
        let msg = e.to_string();
        assert!(
            msg.contains("auth.client_id"),
            "must name the config key, got: {msg}"
        );
    }
}
