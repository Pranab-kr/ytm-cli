//! Column-accurate text helpers. Terminal layout is measured in display
//! columns, never bytes or chars — `str::len()` breaks on CJK and emoji.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn display_width(s: &str) -> usize {
    s.width()
}

/// Truncate to at most `width` columns, appending `…` when text was dropped.
/// Never splits a multi-column character.
pub fn truncate_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if s.width() <= width {
        return s.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }

    let budget = width - 1; // reserve one column for the ellipsis
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Right-pad with spaces to exactly `width` columns, truncating if too long.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let t = truncate_to_width(s, width);
    let w = t.width();
    let mut out = t;
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(w)));
    out
}

/// Keep the **end** of a string within `width` columns. `truncate_to_width`
/// keeps the start, which would hide the characters just typed.
pub fn tail_to_width(s: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if width == 0 {
        return String::new();
    }
    let mut used = 0usize;
    let mut take_from = s.len();
    for (i, c) in s.char_indices().rev() {
        let cw = c.width().unwrap_or(0);
        if used + cw > width {
            break;
        }
        used += cw;
        take_from = i;
    }
    s[take_from..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_shorter_than_the_width_is_unchanged() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn ascii_longer_than_the_width_gets_an_ellipsis() {
        assert_eq!(truncate_to_width("hello world", 8), "hello w…");
        assert_eq!(display_width(&truncate_to_width("hello world", 8)), 8);
    }

    #[test]
    fn wide_cjk_characters_count_as_two_columns() {
        // "日本語" is 3 chars but 6 columns. str::len() would report 9 bytes.
        assert_eq!(display_width("日本語"), 6);
        let t = truncate_to_width("日本語テスト", 6);
        assert!(
            display_width(&t) <= 6,
            "must never exceed the budget, got {}",
            display_width(&t)
        );
    }

    #[test]
    fn truncation_never_splits_a_wide_character_in_half() {
        // Budget 5 cannot fit 3 wide chars (6 cols); it must drop one, not split.
        let t = truncate_to_width("日本語", 5);
        assert!(display_width(&t) <= 5);
        assert!(!t.contains('\u{FFFD}'), "no replacement chars: {t}");
    }

    #[test]
    fn zero_width_budget_yields_an_empty_string() {
        assert_eq!(truncate_to_width("anything", 0), "");
    }

    #[test]
    fn width_of_one_leaves_room_only_for_the_ellipsis() {
        assert_eq!(truncate_to_width("hello", 1), "…");
    }

    #[test]
    fn pad_fills_to_the_column_width() {
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(display_width(&pad_to_width("日本", 6)), 6);
    }
}
