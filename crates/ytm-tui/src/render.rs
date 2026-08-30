//! The top-level frame layout: sidebar | main, with the now-playing bar pinned
//! to the bottom. Every widget guards on a zero-sized `Rect` — layout math on a
//! tiny terminal is how a TUI panics and loses the user's session.

use crate::{
    app::AppState,
    theme::Theme,
    util::text::truncate_to_width,
    widgets::{nowplaying, sidebar},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Spec §6: sidebar ~22 columns, now-playing bar 3 rows.
const SIDEBAR_WIDTH: u16 = 22;
const NOWPLAYING_HEIGHT: u16 = 3;

pub fn render(f: &mut Frame, s: &AppState, t: &Theme) {
    let area = f.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(NOWPLAYING_HEIGHT)])
        .split(area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(SIDEBAR_WIDTH),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(rows[0]);

    sidebar::draw(f, cols[0], s, t);
    draw_rule(f, cols[1], t);
    draw_main(f, cols[2], s, t);

    // Modals and toasts overlay everything, so they go last (Tasks 27, 29).
    nowplaying::draw(f, rows[1], s, t);
}

/// A single dim vertical rule between sidebar and main. No heavy boxes.
fn draw_rule(f: &mut Frame, area: Rect, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(t.fg_dim))))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

/// The main pane. Task 21 draws only the heading; the list widgets that fill it
/// arrive in Tasks 23-26.
fn draw_main(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let w = area.width as usize;
    let heading = truncate_to_width(&pane_title(s), w);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            heading,
            Style::default()
                .fg(t.fg_bright)
                .add_modifier(Modifier::BOLD),
        ))),
        area,
    );
}

fn pane_title(s: &AppState) -> String {
    use crate::app::Pane;
    match s.pane {
        Pane::Playlists => match &s.open_playlist {
            Some(id) => s
                .playlists
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Playlist".to_owned()),
            None => "Playlists".to_owned(),
        },
        Pane::Songs => "Songs".to_owned(),
        Pane::Albums => "Albums".to_owned(),
        Pane::Artists => "Artists".to_owned(),
        Pane::Search => "Search".to_owned(),
        Pane::Queue => "Queue".to_owned(),
    }
}
