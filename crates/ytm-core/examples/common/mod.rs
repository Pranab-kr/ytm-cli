//! Credential loading for the manual spike examples.
//!
//! Reads `~/.config/ytm-cli/config.toml` so the client secret lives in one file
//! outside the repo and never has to be pasted into a shell command, a
//! transcript, or a process listing. Environment variables still win when set,
//! which is handy for a one-off.
//!
//! In a subdirectory so Cargo does not treat it as its own example target.

use std::path::PathBuf;

pub struct Creds {
    pub client_id: String,
    pub client_secret: String,
}

pub fn config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "ytm-cli")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

/// Env first, then the config file. Returns a message naming the fix rather
/// than a bare error.
pub fn load() -> Result<Creds, String> {
    if let (Ok(id), Ok(secret)) = (
        std::env::var("YTM_CLIENT_ID"),
        std::env::var("YTM_CLIENT_SECRET"),
    ) {
        return Ok(Creds {
            client_id: id,
            client_secret: secret,
        });
    }

    let path = config_path();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "could not read {}: {e}\n\
             Create it with:\n\n\
             [auth]\n\
             kind = \"oauth\"\n\
             client_id = \"...\"\n\
             client_secret = \"...\"",
            path.display()
        )
    })?;

    let value: toml::Value =
        toml::from_str(&text).map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;
    let auth = value
        .get("auth")
        .ok_or_else(|| format!("{} has no [auth] section", path.display()))?;

    let get = |key: &str| -> Result<String, String> {
        auth.get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && !s.starts_with("PASTE"))
            .map(str::to_owned)
            .ok_or_else(|| format!("set `auth.{key}` in {}", path.display()))
    };

    Ok(Creds {
        client_id: get("client_id")?,
        client_secret: get("client_secret")?,
    })
}
