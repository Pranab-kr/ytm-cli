//! One accent, three neutrals, plus error and success (spec §6). No more —
//! extra colors are how a TUI starts looking accidental.

use ratatui::style::Color;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("theme is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("theme key `{0}` is not a valid hex color like \"#7aa2f7\"")]
    BadColor(String),
    #[error("theme key `preset` names no built-in theme: {0:?} (try one of: {1})")]
    UnknownPreset(String, String),
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
    /// Start from a built-in theme; individual keys below still override it.
    preset: Option<String>,
    accent: Option<String>,
    fg: Option<String>,
    fg_dim: Option<String>,
    fg_bright: Option<String>,
    bg_sel: Option<String>,
    error: Option<String>,
    success: Option<String>,
}

/// The built-in themes, in cycle order. `bg` stays `Reset` on the dark ones so
/// they inherit the terminal's own background; the light ones must paint it, or
/// dark text lands on a dark terminal.
const PRESETS: [(&str, ThemeSpec); 6] = [
    (
        "tokyonight",
        ThemeSpec {
            accent: 0x7aa2f7,
            fg: 0xc0caf5,
            fg_dim: 0x565f89,
            fg_bright: 0xffffff,
            bg: None,
            bg_sel: 0x2a2f41,
            error: 0xf7768e,
            success: 0x9ece6a,
        },
    ),
    (
        "gruvbox",
        ThemeSpec {
            accent: 0xfabd2f,
            fg: 0xebdbb2,
            fg_dim: 0x928374,
            fg_bright: 0xfbf1c7,
            bg: None,
            bg_sel: 0x3c3836,
            error: 0xfb4934,
            success: 0xb8bb26,
        },
    ),
    (
        "nord",
        ThemeSpec {
            accent: 0x88c0d0,
            fg: 0xd8dee9,
            fg_dim: 0x616e88,
            fg_bright: 0xeceff4,
            bg: None,
            bg_sel: 0x3b4252,
            error: 0xbf616a,
            success: 0xa3be8c,
        },
    ),
    (
        "dracula",
        ThemeSpec {
            accent: 0xbd93f9,
            fg: 0xf8f8f2,
            fg_dim: 0x6272a4,
            fg_bright: 0xffffff,
            bg: None,
            bg_sel: 0x44475a,
            error: 0xff5555,
            success: 0x50fa7b,
        },
    ),
    (
        "dawn",
        ThemeSpec {
            accent: 0x286983,
            fg: 0x575279,
            fg_dim: 0x9893a5,
            fg_bright: 0x1f1d2e,
            bg: Some(0xfaf4ed),
            bg_sel: 0xdfdad9,
            error: 0xb4637a,
            success: 0x286983,
        },
    ),
    (
        "paper",
        ThemeSpec {
            accent: 0x1c6b8c,
            fg: 0x33322e,
            fg_dim: 0x8a8778,
            fg_bright: 0x111110,
            bg: Some(0xfffef7),
            bg_sel: 0xe8e5d8,
            error: 0xa32b3a,
            success: 0x2c6b32,
        },
    ),
];

/// A preset as plain hex, so the table above stays readable and `const`.
struct ThemeSpec {
    accent: u32,
    fg: u32,
    fg_dim: u32,
    fg_bright: u32,
    /// `None` inherits the terminal background; `Some` paints it.
    bg: Option<u32>,
    bg_sel: u32,
    error: u32,
    success: u32,
}

fn rgb(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Theme {
    /// Names of the built-in themes, in the order the toggle cycles them.
    pub fn preset_names() -> &'static [&'static str] {
        const NAMES: [&str; PRESETS.len()] = {
            let mut n = [""; PRESETS.len()];
            let mut i = 0;
            while i < PRESETS.len() {
                n[i] = PRESETS[i].0;
                i += 1;
            }
            n
        };
        &NAMES
    }

    /// One built-in theme by name, or `None` — a typo should be reported, not
    /// silently swapped for the default.
    pub fn preset(name: &str) -> Option<Self> {
        let (_, spec) = PRESETS.iter().find(|(n, _)| *n == name)?;
        Some(Self {
            accent: rgb(spec.accent),
            fg: rgb(spec.fg),
            fg_dim: rgb(spec.fg_dim),
            fg_bright: rgb(spec.fg_bright),
            bg: spec.bg.map(rgb).unwrap_or(Color::Reset),
            bg_sel: rgb(spec.bg_sel),
            error: rgb(spec.error),
            success: rgb(spec.success),
        })
    }

    /// The next theme in the cycle. An unknown name starts the cycle over
    /// rather than dead-ending, so a stale config value cannot wedge the key.
    pub fn next_preset(current: &str) -> &'static str {
        let names = Self::preset_names();
        match names.iter().position(|n| *n == current) {
            Some(i) => names[(i + 1) % names.len()],
            None => names[0],
        }
    }

    /// Whether this theme is meant for a light terminal. Measured from `fg` luminance
    /// rather than stored: a light theme is one whose text is dark, and that stays
    /// true for a hand-written theme file.
    pub fn is_light(&self) -> bool {
        match self.fg {
            // Rec. 601 luma, the usual cheap approximation.
            Color::Rgb(r, g, b) => (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) < 128.0,
            _ => false,
        }
    }

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
        // A preset is the starting point; the explicit keys below still win, so
        // "gruvbox but a different accent" is one line plus one override.
        let mut t = match &f.preset {
            Some(name) => Self::preset(name).ok_or_else(|| {
                ThemeError::UnknownPreset(name.clone(), Self::preset_names().join(", "))
            })?,
            None => Self::default(),
        };
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

    #[test]
    fn every_named_preset_resolves_and_defines_every_role() {
        // A preset with a Reset role renders an invisible element.
        for name in Theme::preset_names() {
            let t = Theme::preset(name).unwrap_or_else(|| panic!("{name} must resolve"));
            for (role, c) in t.roles() {
                assert!(
                    !matches!(c, Color::Reset),
                    "{name}: role {role} must be explicit"
                );
            }
        }
    }

    #[test]
    fn presets_include_both_dark_and_light_options() {
        // The toggle is only meaningful if the set spans both.
        let names = Theme::preset_names();
        assert!(names.iter().any(|n| Theme::preset(n).unwrap().is_light()));
        assert!(names.iter().any(|n| !Theme::preset(n).unwrap().is_light()));
    }

    #[test]
    fn an_unknown_preset_name_is_rejected_rather_than_silently_defaulted() {
        assert!(Theme::preset("mauve-dream").is_none());
    }

    #[test]
    fn cycling_visits_every_preset_and_returns_to_the_start() {
        let names = Theme::preset_names();
        let mut at = names[0].to_owned();
        let mut seen = vec![at.clone()];
        for _ in 1..names.len() {
            at = Theme::next_preset(&at).to_owned();
            assert!(!seen.contains(&at), "cycle repeated {at} early");
            seen.push(at.clone());
        }
        assert_eq!(
            Theme::next_preset(&at),
            names[0],
            "the cycle must wrap to the first"
        );
    }

    #[test]
    fn cycling_from_an_unknown_name_lands_somewhere_valid() {
        // A stale name in config must not wedge the toggle.
        let n = Theme::next_preset("not-a-theme");
        assert!(Theme::preset(n).is_some(), "got {n}");
    }

    #[test]
    fn a_light_preset_is_reported_light_and_a_dark_one_dark() {
        // The flag drives the auto choice, so it must reflect real luminance
        // rather than the name.
        assert!(Theme::preset("dawn").unwrap().is_light());
        assert!(!Theme::preset("tokyonight").unwrap().is_light());
    }

    #[test]
    fn a_theme_file_still_overrides_a_preset_role() {
        let t = Theme::from_toml_str(
            r##"preset = "dawn"
accent = "#ff0000""##,
        )
        .unwrap();
        assert_eq!(t.accent, Color::Rgb(0xff, 0, 0), "explicit key wins");
        assert_eq!(
            t.fg,
            Theme::preset("dawn").unwrap().fg,
            "unspecified roles come from the preset"
        );
    }

    #[test]
    fn a_bad_preset_name_in_a_theme_file_names_the_key() {
        let e = Theme::from_toml_str(r#"preset = "nope""#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("preset"), "got: {e}");
        assert!(e.contains("nope"), "name the bad value, got: {e}");
    }
}
