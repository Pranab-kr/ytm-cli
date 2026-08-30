//! One row per track: mark, title, artist, duration. Columns are sized by
//! display width so CJK titles keep the grid intact (spec §6).

use crate::{
    app::{AppState, Pane},
    theme::Theme,
    util::text::pad_to_width,
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
/// The multi-select bullet plus its trailing space.
const MARK_WIDTH: usize = 2;
/// Title takes six tenths of what is left; the artist gets the rest.
const TITLE_SHARE: usize = 6;

/// The slice of rows to draw, keeping `selected` visible.
///
/// Returns a half-open range. `offset` is the previous scroll position, used as
/// a starting guess so the list does not jump when the selection has not left
/// the viewport.
pub fn visible_window(selected: usize, offset: usize, height: usize, len: usize) -> (usize, usize) {
    if len == 0 || height == 0 {
        return (0, 0);
    }
    let mut start = offset.min(len.saturating_sub(1));
    if selected < start {
        start = selected;
    }
    if selected >= start + height {
        start = selected + 1 - height;
    }
    let end = (start + height).min(len);
    (start, end)
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rows: &[ytm_core::Track] = match s.pane {
        Pane::Search => &s.search_results,
        Pane::Queue => &s.queue,
        _ => &s.tracks,
    };

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Nothing here yet",
                Style::default().fg(t.fg_dim),
            )),
            area,
        );
        return;
    }

    let h = area.height as usize;
    let (start, end) = visible_window(s.selected, s.scroll_offset, h, rows.len());
    let w = area.width as usize;

    // Columns in display cells, not bytes: mark + title + artist + duration.
    let text_w = w.saturating_sub(DURATION_WIDTH + MARK_WIDTH);
    let title_w = (text_w * TITLE_SHARE) / 10;
    let artist_w = text_w.saturating_sub(title_w);

    let items: Vec<ListItem> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let idx = start + i;
            let is_sel = idx == s.selected;
            let is_now = s
                .now_playing
                .as_ref()
                .is_some_and(|n| n.video_id == track.video_id);
            let marked = s.marked.contains(&track.video_id);

            let base = if is_now {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.fg)
            };
            // Selection is a reversed background, not a '>' marker (spec §6).
            let style = if is_sel { base.bg(t.bg_sel) } else { base };
            let dim = if is_sel {
                style
            } else {
                Style::default().fg(t.fg_dim)
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    if marked { "\u{2022} " } else { "  " },
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
    use super::*;

    /// Render through the real entry point so the test covers the layout the
    /// user actually sees, not just this widget in isolation.
    fn buffer_text(s: &crate::app::AppState) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = crate::theme::Theme::default();
        t.draw(|f| crate::render::render(f, s, &theme, &crate::keymap::KeyMap::default()))
            .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn window_shows_the_top_when_selection_is_near_the_start() {
        assert_eq!(visible_window(0, 0, 10, 100), (0, 10));
        assert_eq!(visible_window(5, 0, 10, 100), (0, 10));
    }

    #[test]
    fn window_scrolls_when_the_selection_passes_the_bottom() {
        // selection 12 with a 10-row viewport must bring row 12 into view
        let (start, end) = visible_window(12, 0, 10, 100);
        assert!(
            start <= 12 && 12 < end,
            "selection must be visible, got {start}..{end}"
        );
    }

    #[test]
    fn window_never_exceeds_the_item_count() {
        let (start, end) = visible_window(2, 0, 10, 3);
        assert_eq!((start, end), (0, 3));
    }

    #[test]
    fn window_is_empty_for_an_empty_list() {
        assert_eq!(visible_window(0, 0, 10, 0), (0, 0));
    }

    #[test]
    fn window_handles_a_zero_height_viewport() {
        assert_eq!(visible_window(0, 0, 0, 50), (0, 0));
    }

    #[test]
    fn rows_show_title_artist_and_duration() {
        use crate::app::{AppState, Pane};
        use ytm_core::{Track, TrackDuration};

        let s = AppState {
            pane: Pane::Songs,
            tracks: vec![Track {
                title: "Roygbiv".into(),
                artists: vec!["Boards of Canada".into()],
                duration: TrackDuration::from_secs(149),
                ..Track::stub("v1", "Roygbiv")
            }],
            ..Default::default()
        };

        let text = buffer_text(&s);
        assert!(text.contains("Roygbiv"));
        assert!(text.contains("Boards of Canada"));
        assert!(text.contains("2:29"));
    }

    #[test]
    fn an_empty_pane_shows_a_message_not_a_blank_area() {
        use crate::app::{AppState, Pane};
        let s = AppState {
            pane: Pane::Songs,
            ..Default::default()
        };
        assert!(
            buffer_text(&s).contains("Nothing here"),
            "empty states must say something"
        );
    }
}
