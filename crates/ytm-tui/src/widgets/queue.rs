//! The play queue: one row per entry, with the current one marked.
//!
//! Columns match `tracklist` so switching panes does not shift the grid, but the
//! left column means something different here: it is the play position (FR-Q1),
//! not the multi-select bullet. Nothing in the queue is multi-selected.

use crate::{
    app::AppState, theme::Theme, util::text::pad_to_width, widgets::tracklist::visible_window,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

/// Right-hand duration column, wide enough for `1:02:03`.
const DURATION_WIDTH: usize = 7;
/// The current-entry marker plus its trailing space.
const MARK_WIDTH: usize = 2;
/// Title takes six tenths of what is left; the artist gets the rest.
const TITLE_SHARE: usize = 6;

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // The filtered view, not `s.queue`: the reducer counts and indexes this, so
    // drawing the raw queue made `/` look broken here and put the cursor on a
    // different row than `x` or Enter acted on.
    let rows = s.visible_tracks();

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                if s.is_filtering() {
                    "No matches"
                } else {
                    "Queue is empty"
                },
                Style::default().fg(t.fg_dim),
            )),
            area,
        );
        return;
    }

    let h = area.height as usize;
    let (start, end) = visible_window(s.selected, s.scroll_offset, h, rows.len());
    let w = area.width as usize;

    let text_w = w.saturating_sub(DURATION_WIDTH + MARK_WIDTH);
    let title_w = (text_w * TITLE_SHARE) / 10;
    let artist_w = text_w.saturating_sub(title_w);

    let items: Vec<ListItem> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let idx = start + i;
            let is_sel = idx == s.selected;
            // Unfiltered, the visible index *is* the queue index, and that is the
            // only way to tell two copies of the same song apart — the queue can
            // legitimately hold a video twice. Under a filter the two indices
            // differ, so fall back to the id and accept that duplicates both
            // highlight; showing the wrong row as playing would be worse.
            let is_current = if s.is_filtering() {
                s.queue_current
                    .and_then(|c| s.queue.get(c))
                    .is_some_and(|c| c.video_id == track.video_id)
            } else {
                s.queue_current == Some(idx)
            };

            let base = if is_current {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.fg)
            };
            let style = if is_sel { base.bg(t.bg_sel) } else { base };
            let dim = if is_sel {
                style
            } else {
                Style::default().fg(t.fg_dim)
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    if is_current { "\u{25b6} " } else { "  " },
                    Style::default().fg(t.accent),
                ),
                Span::styled(pad_to_width(&track.title, title_w), style),
                Span::styled(pad_to_width(&track.artist_display(), artist_w), dim),
                Span::styled(
                    format!(
                        "{:>width$}",
                        track.duration.to_string(),
                        width = DURATION_WIDTH
                    ),
                    dim,
                ),
            ]))
            .style(if is_sel {
                Style::default().bg(t.bg_sel)
            } else {
                Style::default()
            })
        })
        .collect();

    f.render_widget(List::new(items), area);
}

#[cfg(test)]
mod tests {
    use crate::{
        app::{AppState, Pane},
        theme::Theme,
    };
    use ratatui::{Terminal, backend::TestBackend};
    use ytm_core::Track;

    /// Render through the real entry point, so these cover what the user sees.
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

    fn two_entries() -> AppState {
        AppState {
            pane: Pane::Queue,
            queue: vec![Track::stub("v1", "First"), Track::stub("v2", "Second")],
            ..Default::default()
        }
    }

    #[test]
    fn queue_lists_its_tracks_in_order() {
        let text = text_of(&two_entries());
        assert!(text.contains("First"));
        assert!(text.contains("Second"));
        assert!(
            text.find("First") < text.find("Second"),
            "order must be preserved"
        );
    }

    #[test]
    fn the_current_queue_entry_is_marked() {
        let s = AppState {
            queue_current: Some(1),
            ..two_entries()
        };
        // FR-Q1: the user must be able to tell where they are.
        assert!(
            text_of(&s).contains("\u{25b6}"),
            "current entry needs a marker"
        );
    }

    #[test]
    fn an_entry_that_is_not_current_gets_no_marker() {
        // Without this, printing the marker on every row would pass the test
        // above while telling the user nothing.
        let s = AppState {
            queue_current: None,
            ..two_entries()
        };
        assert!(!text_of(&s).contains("\u{25b6}"));
    }

    #[test]
    fn an_empty_queue_explains_itself() {
        let s = AppState {
            pane: Pane::Queue,
            ..Default::default()
        };
        assert!(text_of(&s).contains("Queue is empty"));
    }
}
