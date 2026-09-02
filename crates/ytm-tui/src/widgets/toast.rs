//! Overlays: toasts bottom-right, modals centered.

use crate::{
    app::{AppState, ToastKind},
    theme::Theme,
    util::text::{display_width, truncate_to_width},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// At most this many toasts on screen; a burst of failures must not bury the
/// pane it is describing.
const MAX_VISIBLE: usize = 3;
/// Leave a margin so a toast never sits flush against the frame edge.
const MARGIN: u16 = 1;

/// A rect of `pct_x`% x `pct_y`% centered in `area`, never larger than it.
pub fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let h = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(h[1])[1]
}

/// Stack the newest toasts upward from the bottom-right, one row each. The newest
/// are kept rather than the oldest: the last thing that happened is what the user is
/// trying to understand.
pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 || s.toasts.is_empty() {
        return;
    }

    let max_w = area.width.saturating_sub(MARGIN * 2) as usize;
    if max_w == 0 {
        return;
    }

    // Newest first, so index 0 is the bottom row.
    let visible: Vec<_> = s.toasts.iter().rev().take(MAX_VISIBLE).collect();

    for (i, toast) in visible.iter().enumerate() {
        let row = area.bottom().saturating_sub(MARGIN + 1 + i as u16);
        // Off the top of a short frame: stop rather than wrapping around.
        if row < area.top() {
            break;
        }

        let text = truncate_to_width(&toast.text, max_w);
        let w = display_width(&text) as u16;
        let rect = Rect {
            x: area.right().saturating_sub(MARGIN + w),
            y: row,
            width: w,
            height: 1,
        };

        let color = match toast.kind {
            ToastKind::Error => t.error,
            ToastKind::Success => t.success,
            ToastKind::Info => t.fg,
        };
        // Clear first: a toast sits over the list, and leftover glyphs
        // underneath would read as part of the message.
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color)))),
            rect,
        );
    }
}

/// The activity indicator, top-right of the pane it belongs to (FR-U4). The frame
/// index comes from `elapsed_ms` rather than a stored `ThrobberState`, so rendering
/// stays a pure function of `AppState` and is reproducible in a test.
pub fn draw_spinner(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if !s.loading || area.width == 0 || area.height == 0 {
        return;
    }
    let throbber =
        throbber_widgets_tui::Throbber::default().throbber_style(Style::default().fg(t.accent));
    let mut state = throbber_widgets_tui::ThrobberState::default();
    state.calc_step((s.elapsed_ms / 100) as i8);

    let span = throbber.to_symbol_span(&state);
    let w = display_width(&span.content) as u16;
    let rect = Rect {
        x: area.right().saturating_sub(w),
        y: area.top(),
        width: w.min(area.width),
        height: 1,
    };
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(Line::from(span)), rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{AppState, ToastKind},
        theme::Theme,
        util::text::{display_width, truncate_to_width},
    };
    use ratatui::layout::Rect;

    fn text_of(s: &AppState) -> String {
        use ratatui::{Terminal, backend::TestBackend};
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
    fn centered_rect_is_centered_and_correctly_sized() {
        let area = Rect::new(0, 0, 100, 100);
        let r = centered_rect(50, 40, area);
        assert_eq!(r.width, 50);
        assert_eq!(r.height, 40);
        assert_eq!(r.x, 25);
        assert_eq!(r.y, 30);
    }

    #[test]
    fn centered_rect_never_exceeds_a_tiny_area() {
        let area = Rect::new(0, 0, 4, 3);
        let r = centered_rect(90, 90, area);
        assert!(r.width <= 4 && r.height <= 3, "got {r:?}");
    }

    #[test]
    fn error_toasts_render_their_text() {
        let mut s = AppState::default();
        s.push_toast(ToastKind::Error, "rate limited", 0);
        assert!(text_of(&s).contains("rate limited"));
    }

    #[test]
    fn at_most_three_toasts_are_visible_at_once() {
        // A burst of failures must not paper over the whole pane.
        let mut s = AppState::default();
        for i in 0..6 {
            s.push_toast(ToastKind::Error, &format!("problem {i}"), 0);
        }
        let text = text_of(&s);
        let shown = (0..6)
            .filter(|i| text.contains(&format!("problem {i}")))
            .count();
        assert_eq!(shown, 3, "expected 3 of 6 toasts, saw {shown}");
    }

    #[test]
    fn the_newest_toast_is_the_one_kept() {
        let mut s = AppState::default();
        for i in 0..5 {
            s.push_toast(ToastKind::Error, &format!("problem {i}"), 0);
        }
        let text = text_of(&s);
        assert!(text.contains("problem 4"), "the newest must survive");
        assert!(!text.contains("problem 0"), "the oldest must be dropped");
    }

    #[test]
    fn a_long_toast_is_truncated_rather_than_wrapping_off_screen() {
        let long = "x".repeat(200);
        let fitted = truncate_to_width(&long, 40);
        assert!(display_width(&fitted) <= 40);
    }

    #[test]
    fn a_long_toast_does_not_push_the_frame_wider_than_the_terminal() {
        let mut s = AppState::default();
        s.push_toast(ToastKind::Error, &"x".repeat(300), 0);
        // 80x20 = 1600 cells. Overflowing layout math would panic or clip.
        assert_eq!(text_of(&s).chars().count(), 1600);
    }

    #[test]
    fn no_toasts_means_nothing_is_drawn_over_the_list() {
        let s = AppState {
            pane: crate::app::Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        assert!(text_of(&s).contains("Roygbiv"));
    }

    #[test]
    fn the_spinner_shows_only_while_loading() {
        // FR-U4: a fetch in flight must be visible.
        let mut s = AppState {
            pane: crate::app::Pane::Songs,
            loading: true,
            ..Default::default()
        };
        let busy = text_of(&s);
        s.loading = false;
        let idle = text_of(&s);
        assert_ne!(busy, idle, "the spinner must change the frame");
    }
}
