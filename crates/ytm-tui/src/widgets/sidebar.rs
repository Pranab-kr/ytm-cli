//! The source list down the left edge. Labels are asserted on by tests and by
//! the user's muscle memory — do not rename them casually.

use crate::{
    app::{AppState, Focus, Pane},
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

/// Display order of the sources, top to bottom.
pub const SOURCES: [(Pane, &str); 6] = [
    (Pane::Playlists, "Playlists"),
    (Pane::Songs, "Songs"),
    (Pane::Albums, "Albums"),
    (Pane::Artists, "Artists"),
    (Pane::Search, "Search"),
    (Pane::Queue, "Queue"),
];

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let w = area.width as usize;
    let lines: Vec<Line> = SOURCES
        .iter()
        .enumerate()
        .take(area.height as usize)
        .map(|(i, (pane, label))| {
            let selected = i == s.sidebar_selected;
            let active = *pane == s.pane;

            let mut style = Style::default().fg(if active { t.fg_bright } else { t.fg });
            if active {
                style = style.add_modifier(Modifier::BOLD);
            }
            // Reversed background for selection, not a '>' marker (spec §6).
            if selected && s.focus == Focus::Sidebar {
                style = style.bg(t.bg_sel);
            }
            Line::from(Span::styled(truncate_to_width(label, w), style))
        })
        .collect();

    f.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_keys_match_the_order_the_sidebar_renders() {
        // `goto_source` indexes PANE_ORDER; this widget renders SOURCES. If the
        // two ever disagree, pressing 3 highlights one row and opens another.
        use crate::app::PANE_ORDER;
        let rendered: Vec<_> = SOURCES.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            rendered,
            PANE_ORDER.to_vec(),
            "sidebar order and PANE_ORDER must stay identical"
        );
    }
}
