//! Config loaded from TOML. Secrets are NOT stored here beyond the OAuth
//! client id/secret, which Google treats as non-confidential for device flow.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("config is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("config key `playback.volume` must be 0-100, got {0}")]
    Volume(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    OAuth,
    Cookie,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub kind: AuthKind,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cookie_file: Option<PathBuf>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            kind: AuthKind::OAuth,
            client_id: None,
            client_secret: None,
            cookie_file: None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct PlaybackConfig {
    pub volume: u16,
    pub shuffle: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            volume: 70,
            shuffle: false,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub vim_keys: bool,
    pub tick_ms: u64,
    pub accent: Option<String>,
    pub album_art: bool,
    pub theme_file: Option<PathBuf>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            vim_keys: true,
            tick_ms: 250,
            accent: None,
            album_art: true,
            theme_file: None,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub auth: AuthConfig,
    pub playback: PlaybackConfig,
    pub ui: UiConfig,
}

impl Config {
    pub fn from_toml_str(s: &str) -> Result<Self, ConfigError> {
        let c: Config = toml::from_str(s)?;
        if c.playback.volume > 100 {
            return Err(ConfigError::Volume(c.playback.volume));
        }
        Ok(c)
    }

    /// Missing file is not an error — defaults are valid.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = path.map(PathBuf::from).unwrap_or_else(Self::default_path);
        match std::fs::read_to_string(&path) {
            Ok(s) => Self::from_toml_str(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io { path, source }),
        }
    }

    pub fn default_path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }
}

pub mod paths {
    use directories::ProjectDirs;
    use std::path::PathBuf;

    fn dirs() -> Option<ProjectDirs> {
        ProjectDirs::from("", "", "ytm-cli")
    }

    pub fn config_dir() -> PathBuf {
        dirs()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
    pub fn cache_dir() -> PathBuf {
        dirs()
            .map(|d| d.cache_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
    pub fn log_dir() -> PathBuf {
        cache_dir().join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_file_is_absent() {
        let c = Config::from_toml_str("").unwrap();
        assert_eq!(c.auth.kind, AuthKind::OAuth);
        assert_eq!(c.playback.volume, 70);
        assert!(c.ui.vim_keys);
        assert_eq!(c.ui.tick_ms, 250);
    }

    #[test]
    fn parses_a_full_config() {
        let c = Config::from_toml_str(
            r##"
            [auth]
            kind = "cookie"
            cookie_file = "/tmp/c.txt"

            [playback]
            volume = 40

            [ui]
            vim_keys = false
            accent = "#7aa2f7"
            "##,
        )
        .unwrap();
        assert_eq!(c.auth.kind, AuthKind::Cookie);
        assert_eq!(
            c.auth.cookie_file.as_deref(),
            Some(std::path::Path::new("/tmp/c.txt"))
        );
        assert_eq!(c.playback.volume, 40);
        assert!(!c.ui.vim_keys);
        assert_eq!(c.ui.accent.as_deref(), Some("#7aa2f7"));
    }

    #[test]
    fn volume_out_of_range_is_rejected_with_a_clear_message() {
        let err = Config::from_toml_str("[playback]\nvolume = 500")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("volume"),
            "message should name the offending key, got: {err}"
        );
    }

    #[test]
    fn oauth_credentials_are_optional_at_parse_time() {
        // Missing creds is a login-time error with a helpful message, not a parse error.
        let c = Config::from_toml_str("[auth]\nkind = \"oauth\"").unwrap();
        assert!(c.auth.client_id.is_none());
    }
}
