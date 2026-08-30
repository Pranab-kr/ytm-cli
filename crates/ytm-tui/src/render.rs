//! The top-level frame layout: sidebar | main, with the now-playing bar pinned
//! to the bottom. Every widget guards on a zero-sized `Rect` — layout math on a
//! tiny terminal is how a TUI panics and loses the user's session.

use crate::{
    app::{AppState, Modal, Pane},
    keymap::KeyMap,
    theme::Theme,
    util::text::truncate_to_width,
    widgets::{help, modal, nowplaying, playlists, queue, search, sidebar, toast, tracklist},
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

pub fn render(f: &mut Frame, s: &AppState, t: &Theme, km: &KeyMap) {
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
    nowplaying::draw(f, rows[1], s, t);

    // Overlays go last, over everything they describe.
    // The login modal belongs to the auth pane (Task 33).
    if let Some(Modal::Help) = &s.modal {
        help::draw(f, area, km, t);
    } else {
        modal::draw(f, area, s, t);
    }
    toast::draw(f, area, s, t);
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

/// The main pane: a heading row, then the list for whichever pane is active.
///
/// The match is exhaustive on `Pane` rather than ending in a `_` arm, so adding
/// a pane later fails to compile instead of silently rendering nothing.
fn draw_main(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let w = area.width as usize;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let heading = truncate_to_width(&pane_title(s), w);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            heading,
            Style::default()
                .fg(t.fg_bright)
                .add_modifier(Modifier::BOLD),
        ))),
        rows[0],
    );
    // Top-right of the heading row: tied to the pane whose data is loading.
    toast::draw_spinner(f, rows[0], s, t);

    match s.pane {
        // An open playlist shows its tracks; the list of playlists otherwise.
        Pane::Playlists if s.open_playlist.is_some() => tracklist::draw(f, rows[1], s, t),
        Pane::Playlists => playlists::draw_playlists(f, rows[1], s, t),
        Pane::Songs => tracklist::draw(f, rows[1], s, t),
        Pane::Queue => queue::draw(f, rows[1], s, t),
        Pane::Search => draw_search(f, rows[1], s, t),
        Pane::Albums => playlists::draw_albums(f, rows[1], s, t),
        Pane::Artists => playlists::draw_artists(f, rows[1], s, t),
    }
}

/// Search is the one pane with its own input row: the query line, then results.
fn draw_search(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    search::draw_input(f, rows[0], s, t);

    // "Searched and found nothing" must not look like "hasn't searched yet",
    // which is what `tracklist`'s generic empty state would say.
    if s.search_results.is_empty() && !s.search_query.trim().is_empty() {
        search::draw_no_matches(f, rows[1], t);
    } else {
        tracklist::draw(f, rows[1], s, t);
    }
}

fn pane_title(s: &AppState) -> String {
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
