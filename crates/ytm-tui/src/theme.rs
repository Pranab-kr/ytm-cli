//! One accent, three neutrals, plus error and success (spec §6). No more —
//! extra colors are how a TUI starts looking accidental.

use ratatui::style::Color;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("theme is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("theme key `{0}` is not a valid hex color like \"#7aa2f7\"")]
    BadColor(String),
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub fg_bright: Color,
    pub bg: Color,
    pub bg_sel: Color,
    pub error: Color,
    pub success: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Color::Rgb(0x7a, 0xa2, 0xf7),
            fg: Color::Rgb(0xc0, 0xca, 0xf5),
            fg_dim: Color::Rgb(0x56, 0x5f, 0x89),
            fg_bright: Color::Rgb(0xff, 0xff, 0xff),
            bg: Color::Reset, // inherit the user's terminal background
            bg_sel: Color::Rgb(0x2a, 0x2f, 0x41),
            error: Color::Rgb(0xf7, 0x76, 0x8e),
            success: Color::Rgb(0x9e, 0xce, 0x6a),
        }
    }
}

#[derive(serde::Deserialize)]
struct ThemeFile {
    accent: Option<String>,
    fg: Option<String>,
    fg_dim: Option<String>,
    fg_bright: Option<String>,
    bg_sel: Option<String>,
    error: Option<String>,
    success: Option<String>,
}

impl Theme {
    /// Every role except `bg`, which intentionally stays `Reset`.
    pub fn roles(&self) -> [(&'static str, Color); 7] {
        [
            ("accent", self.accent),
            ("fg", self.fg),
            ("fg_dim", self.fg_dim),
            ("fg_bright", self.fg_bright),
            ("bg_sel", self.bg_sel),
            ("error", self.error),
            ("success", self.success),
        ]
    }

    pub fn from_toml_str(s: &str) -> Result<Self, ThemeError> {
        let f: ThemeFile = toml::from_str(s)?;
        let mut t = Self::default();
        let set = |key: &str, val: &Option<String>, slot: &mut Color| -> Result<(), ThemeError> {
            if let Some(v) = val {
                *slot = parse_hex(v).ok_or_else(|| ThemeError::BadColor(key.to_owned()))?;
            }
            Ok(())
        };
        set("accent", &f.accent, &mut t.accent)?;
        set("fg", &f.fg, &mut t.fg)?;
        set("fg_dim", &f.fg_dim, &mut t.fg_dim)?;
        set("fg_bright", &f.fg_bright, &mut t.fg_bright)?;
        set("bg_sel", &f.bg_sel, &mut t.bg_sel)?;
        set("error", &f.error, &mut t.error)?;
        set("success", &f.success, &mut t.success)?;
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(parse_hex("#7aa2f7"), Some(Color::Rgb(0x7a, 0xa2, 0xf7)));
        assert_eq!(parse_hex("7aa2f7"), Some(Color::Rgb(0x7a, 0xa2, 0xf7)));
    }

    #[test]
    fn rejects_malformed_hex_instead_of_panicking() {
        assert_eq!(parse_hex("#xyz"), None);
        assert_eq!(parse_hex("#7aa2"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn default_theme_defines_every_role() {
        let t = Theme::default();
        // A None anywhere means a widget would render an invisible element.
        for (name, c) in t.roles() {
            assert!(
                !matches!(c, Color::Reset),
                "role {name} must be explicit, not Reset"
            );
        }
    }

    #[test]
    fn accent_override_from_toml_wins() {
        // r##"…"## because the value contains `"#`, which would close r#"…"#.
        let t = Theme::from_toml_str(r##"accent = "#ff0000""##).unwrap();
        assert_eq!(t.accent, Color::Rgb(0xff, 0, 0));
        // Unspecified roles keep the defaults.
        assert_eq!(t.error, Theme::default().error);
    }

    #[test]
    fn invalid_color_in_toml_is_an_error_naming_the_key() {
        let e = Theme::from_toml_str(r#"accent = "not-a-color""#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("accent"), "got: {e}");
    }
}
