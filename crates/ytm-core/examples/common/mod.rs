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

/// Which auth path config selects. OAuth is primary; cookie is the documented
/// fallback for when Google refuses device-flow tokens on InnerTube.
pub enum AuthChoice {
    OAuth(Creds),
    Cookie(PathBuf),
}

/// Read `auth.kind` and return whichever credential set it selects.
pub fn auth_choice() -> Result<AuthChoice, String> {
    let path = config_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;
    let kind = value
        .get("auth")
        .and_then(|a| a.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("oauth");

    if kind == "cookie" {
        let file = value
            .get("auth")
            .and_then(|a| a.get("cookie_file"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && !s.starts_with("PASTE"))
            .ok_or_else(|| {
                format!(
                    "auth.kind = \"cookie\" but auth.cookie_file is not set in {}",
                    path.display()
                )
            })?;
        let file = PathBuf::from(shellexpand_tilde(file));
        if !file.exists() {
            return Err(format!("cookie file {} does not exist", file.display()));
        }
        return Ok(AuthChoice::Cookie(file));
    }
    load().map(AuthChoice::OAuth)
}

/// Minimal `~` expansion; no need for a dependency just for this.
fn shellexpand_tilde(s: &str) -> String {
    match s.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => s.to_owned(),
        },
        None => s.to_owned(),
    }
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
