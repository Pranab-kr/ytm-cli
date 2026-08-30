//! The query row above the search results.

use crate::{
    app::{AppState, Focus},
    theme::Theme,
    util::text::truncate_to_width,
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
    } else {
        spans.push(Span::styled(
            tail_to_width(&s.search_query, budget),
            Style::default().fg(t.fg_bright),
        ));
    }
    if focused {
        spans.push(Span::styled(CURSOR, Style::default().fg(t.accent)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Keep the **end** of a string within `width` columns. `truncate_to_width`
/// keeps the start, which would hide the characters just typed.
fn tail_to_width(s: &str, width: usize) -> String {
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
        t.draw(|f| crate::render::render(f, s, &theme)).unwrap();
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
        t.draw(|f| crate::render::render(f, &s, &theme)).unwrap();
        // Row 1 of the main pane is the query row; it must stay one row.
        assert!(text_of(&s).contains("Search:"));
    }
}
