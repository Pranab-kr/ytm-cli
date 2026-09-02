//! The query row above the search results.

use crate::{
    app::{AppState, Focus},
    theme::Theme,
    util::text::{display_width, tail_to_width, truncate_to_width},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

const PROMPT: &str = "Search: ";
/// Block cursor, shown only while the field has focus.
const CURSOR: &str = "\u{258F}";

/// One row: the prompt, the query, and a cursor. Truncated from the *left* so
/// the caret stays visible while typing a long query.
pub fn draw_input(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let focused = s.focus == Focus::SearchInput;
    let w = area.width as usize;
    let budget = w.saturating_sub(PROMPT.len() + 1);

    let mut spans = vec![Span::styled(
        PROMPT,
        Style::default().fg(t.fg_dim).add_modifier(Modifier::BOLD),
    )];

    if s.search_query.is_empty() {
        spans.push(Span::styled(
            truncate_to_width("type to search", budget),
            Style::default().fg(t.fg_dim),
        ));
        if focused {
            spans.push(Span::styled(CURSOR, Style::default().fg(t.accent)));
        }
    } else if focused {
        // Split the query at the caret and draw it between the halves. Painting
        // it after the whole string instead would leave the word motions moving
        // state that nothing on screen reflects.
        let (before, after) = split_at_cursor(&s.search_query, s.search_cursor);
        // The caret takes a column, and `before` is truncated from the left so
        // the caret stays on screen while typing a long query.
        let left_budget = budget.saturating_sub(1);
        let left = tail_to_width(before, left_budget);
        let right_budget = left_budget.saturating_sub(display_width(&left));
        spans.push(Span::styled(left, Style::default().fg(t.fg_bright)));
        spans.push(Span::styled(CURSOR, Style::default().fg(t.accent)));
        spans.push(Span::styled(
            truncate_to_width(after, right_budget),
            Style::default().fg(t.fg_bright),
        ));
    } else {
        // Unfocused: no caret, so the tail of the query is what matters.
        spans.push(Span::styled(
            tail_to_width(&s.search_query, budget),
            Style::default().fg(t.fg_bright),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Split the query at the caret, clamped and snapped to a char boundary. The offset
/// can be stale — the query is replaced from outside the field — and slicing a stale
/// or mid-codepoint index panics, which in a TUI takes the session down.
fn split_at_cursor(q: &str, cursor: usize) -> (&str, &str) {
    let mut at = cursor.min(q.len());
    while at > 0 && !q.is_char_boundary(at) {
        at -= 1;
    }
    q.split_at(at)
}

/// Shown when a query returned nothing, to distinguish it from "not searched
/// yet" — an empty results list otherwise looks identical to a pane at rest.
pub fn draw_no_matches(f: &mut Frame, area: Rect, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    f.render_widget(
        Paragraph::new(Span::styled("No matches", Style::default().fg(t.fg_dim))),
        area,
    );
}

/// The filter row, shown whenever a filter is being typed or is narrowing rows. It
/// has to be on screen: without it the user sees only rows disappearing, with no way
/// to tell what the filter holds or how to get back ("the filter text not show").
pub fn draw_filter(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    const FILTER_PROMPT: &str = "Filter: ";
    let focused = s.focus == Focus::FilterInput;
    let w = area.width as usize;
    let budget = w.saturating_sub(FILTER_PROMPT.len() + 1);

    let mut spans = vec![Span::styled(
        FILTER_PROMPT,
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::styled(
        tail_to_width(&s.filter, budget),
        Style::default().fg(t.fg_bright),
    ));
    if focused {
        spans.push(Span::styled(CURSOR, Style::default().fg(t.accent)));
    }
    // The way out, spelled out: Esc keeps the filter and leaves the field, and a
    // second Esc clears it. Discoverable beats memorable.
    let hint = if focused {
        "  enter/esc: leave field"
    } else {
        "  esc: clear filter"
    };
    let used = display_width(FILTER_PROMPT)
        + display_width(&tail_to_width(&s.filter, budget))
        + usize::from(focused);
    if used + display_width(hint) <= w {
        spans.push(Span::styled(hint, Style::default().fg(t.fg_dim)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use crate::{
        app::{AppState, Focus, Pane},
        theme::Theme,
    };
    use ratatui::{Terminal, backend::TestBackend};

    fn text_of(s: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| {
            crate::render::render(
                f,
                s,
                &theme,
                &crate::keymap::KeyMap::default(),
                &mut crate::widgets::art::ArtCache::disabled(),
            )
        })
        .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn the_search_pane_shows_a_prompt_and_the_typed_query() {
        let s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "boards of canada".into(),
            ..Default::default()
        };
        let text = text_of(&s);
        assert!(text.contains("Search:"), "no prompt");
        assert!(text.contains("boards of canada"), "query not echoed");
    }

    #[test]
    fn an_empty_query_shows_a_hint_rather_than_a_bare_prompt() {
        let s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            ..Default::default()
        };
        assert!(text_of(&s).contains("type to search"));
    }

    #[test]
    fn results_render_below_the_query_row() {
        let s = AppState {
            pane: Pane::Search,
            search_query: "boards".into(),
            search_results: vec![ytm_core::Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        let text = text_of(&s);
        assert!(text.contains("Search:"), "query row missing");
        assert!(text.contains("Roygbiv"), "results missing");
    }

    #[test]
    fn a_query_with_no_matches_says_so_rather_than_going_blank() {
        let s = AppState {
            pane: Pane::Search,
            search_query: "zzzzz".into(),
            search_results: vec![],
            ..Default::default()
        };
        assert!(text_of(&s).contains("No matches"));
    }

    #[test]
    fn a_long_query_does_not_overflow_a_narrow_pane() {
        // Users paste. A byte-counted row would wrap and shove the results down.
        let s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "日本語".repeat(40),
            ..Default::default()
        };
        let mut t = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let theme = Theme::default();
        t.draw(|f| {
            crate::render::render(
                f,
                &s,
                &theme,
                &crate::keymap::KeyMap::default(),
                &mut crate::widgets::art::ArtCache::disabled(),
            )
        })
        .unwrap();
        // Row 1 of the main pane is the query row; it must stay one row.
        assert!(text_of(&s).contains("Search:"));
    }

    #[test]
    fn the_caret_is_drawn_where_the_cursor_actually_is() {
        // The whole point of the motions is visible feedback. If the caret is
        // always painted after the text, Ctrl+Left moves state nothing can see
        // and the feature looks broken however correct the reducer is.
        use crate::app::{AppState, Focus, Pane};
        let mut s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "boards of canada".into(),
            search_cursor: "boards of canada".len(),
            ..Default::default()
        };
        let at_end = text_of(&s);
        s.search_cursor = "boards".len();
        let mid_line = text_of(&s);
        assert_ne!(
            at_end, mid_line,
            "moving the caret must change what is on screen"
        );
    }

    #[test]
    fn the_caret_sits_between_the_two_halves_of_the_query() {
        use crate::app::{AppState, Focus, Pane};
        let s = AppState {
            pane: Pane::Search,
            focus: Focus::SearchInput,
            search_query: "abc xyz".into(),
            search_cursor: 3, // right after "abc"
            ..Default::default()
        };
        // The buffer is row-major across the whole 80-column frame, so the query
        // row has to be located rather than assumed to be the first one.
        let t = text_of(&s);
        let row = t
            .as_bytes()
            .chunks(80)
            .map(|c| String::from_utf8_lossy(c).to_string())
            .find(|r| r.contains("Search:"))
            .expect("the query row must be on screen");
        let caret = row.find('\u{258F}').expect("a focused field draws a caret");
        let a = row.find("abc").expect("text before the caret");
        let z = row.find("xyz").expect("text after the caret");
        assert!(
            a < caret && caret < z,
            "caret at {caret} must fall between abc at {a} and xyz at {z}: {row:?}"
        );
    }
}
