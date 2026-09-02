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

/// Byte index of the start of the word before `cursor`, for Ctrl+Left / Ctrl+W.
/// Shell-style: skip separators behind the cursor, then the word itself. Always
/// lands on a char boundary, so slicing with it is safe on multibyte input.
pub fn prev_word_boundary(s: &str, cursor: usize) -> usize {
    let cursor = cursor.min(s.len());
    let head = &s[..cursor];
    let mut it = head.char_indices().rev().peekable();
    let mut at = cursor;
    while let Some(&(i, c)) = it.peek() {
        if is_word_char(c) {
            break;
        }
        at = i;
        it.next();
    }
    while let Some(&(i, c)) = it.peek() {
        if !is_word_char(c) {
            break;
        }
        at = i;
        it.next();
    }
    at
}

/// Byte index of the start of the next word after `cursor`, for Ctrl+Right. Skips
/// the current word then any separators, landing on the next word's first character
/// — or the end of the line when there is none.
pub fn next_word_boundary(s: &str, cursor: usize) -> usize {
    let cursor = cursor.min(s.len());
    let mut it = s[cursor..].char_indices().peekable();
    let mut at = cursor;
    while let Some(&(i, c)) = it.peek() {
        if !is_word_char(c) {
            break;
        }
        at = cursor + i + c.len_utf8();
        it.next();
    }
    while let Some(&(i, c)) = it.peek() {
        if is_word_char(c) {
            break;
        }
        at = cursor + i + c.len_utf8();
        it.next();
    }
    at
}

/// A word is alphanumeric; everything else separates. Deliberately simple —
/// "boards of canada" and "lo-fi beats" both behave the way a shell would.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
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

    #[test]
    fn ctrl_w_from_the_end_deletes_the_last_word() {
        let s = "boards of canada";
        assert_eq!(prev_word_boundary(s, s.len()), "boards of ".len());
    }

    #[test]
    fn ctrl_w_skips_a_trailing_space_before_deleting() {
        // Typing "boards " then Ctrl+W must remove "boards", not just the space.
        let s = "boards of ";
        assert_eq!(prev_word_boundary(s, s.len()), "boards ".len());
    }

    #[test]
    fn ctrl_w_at_the_start_of_the_line_stays_put() {
        assert_eq!(prev_word_boundary("boards", 0), 0);
        assert_eq!(prev_word_boundary("", 0), 0);
    }

    #[test]
    fn ctrl_w_treats_punctuation_as_a_separator() {
        let s = "lo-fi";
        assert_eq!(prev_word_boundary(s, s.len()), "lo-".len());
    }

    #[test]
    fn word_motion_lands_on_char_boundaries_for_multibyte_text() {
        // Slicing a byte index inside a codepoint panics, which would take the
        // terminal down mid-keystroke.
        let s = "日本語 music";
        let back = prev_word_boundary(s, s.len());
        assert!(s.is_char_boundary(back), "index {back} splits a character");
        assert_eq!(&s[back..], "music");
        let fwd = next_word_boundary(s, 0);
        assert!(s.is_char_boundary(fwd));
    }

    #[test]
    fn ctrl_right_moves_to_the_start_of_the_next_word() {
        let s = "boards of canada";
        let a = next_word_boundary(s, 0);
        assert_eq!(&s[a..], "of canada");
        let b = next_word_boundary(s, a);
        assert_eq!(&s[b..], "canada");
    }

    #[test]
    fn ctrl_right_at_the_end_of_the_line_stays_put() {
        let s = "boards";
        assert_eq!(next_word_boundary(s, s.len()), s.len());
    }

    #[test]
    fn ctrl_left_then_ctrl_right_returns_to_the_same_place() {
        let s = "one two three";
        let back = prev_word_boundary(s, s.len());
        assert_eq!(next_word_boundary(s, back), s.len());
    }
}
