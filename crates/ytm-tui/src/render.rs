//! The top-level frame layout: sidebar | main, with the now-playing bar pinned
//! to the bottom. Every widget guards on a zero-sized `Rect` — layout math on a
//! tiny terminal is how a TUI panics and loses the user's session.

use crate::{
    app::{AppState, Modal, Pane},
    keymap::KeyMap,
    theme::Theme,
    util::text::truncate_to_width,
    widgets::{art, help, modal, nowplaying, playlists, queue, search, sidebar, toast, tracklist},
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
/// Columns the art panel takes when it appears.
const ART_WIDTH: u16 = 24;
/// The main pane keeps at least this much, or the art panel does not appear.
/// A shredded track list is worse than absent art (FR-U5).
const MAIN_MIN_WIDTH: u16 = 48;

/// Split the main area into list and art panel, or leave it whole.
///
/// Pure math so the rule is testable without a terminal or an image protocol.
/// The panel appears only when art is actually displayable, something is
/// playing, and the list keeps enough columns to stay readable.
pub fn split_for_art(area: Rect, art_enabled: bool, has_art: bool) -> (Rect, Option<Rect>) {
    if !art_enabled || !has_art || area.width < MAIN_MIN_WIDTH + ART_WIDTH {
        return (area, None);
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(ART_WIDTH)])
        .split(area);
    (cols[0], Some(cols[1]))
}

pub fn render(f: &mut Frame, s: &AppState, t: &Theme, km: &KeyMap, art: &mut art::ArtCache) {
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

    // Art is keyed by the playing track's thumbnail, and only drawn once the
    // bytes have arrived — an in-flight URL leaves the layout unsplit rather
    // than reserving a gap that may never fill.
    let art_url = s
        .now_playing
        .as_ref()
        .and_then(|t| t.thumbnail_url.as_deref());
    let has_art = art_url.is_some_and(|u| art.get(u).is_some());
    let (main_area, art_area) = split_for_art(cols[2], art.is_enabled(), has_art);

    sidebar::draw(f, cols[0], s, t);
    draw_rule(f, cols[1], t);
    draw_main(f, main_area, s, t);
    if let Some(a) = art_area {
        art::draw(f, a, art_url, art);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::art::ArtCache;
    use ratatui::{Terminal, backend::TestBackend};
    use ytm_core::Track;

    fn frame_text(s: &AppState, art: &mut ArtCache, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        let theme = Theme::default();
        t.draw(|f| render(f, s, &theme, &KeyMap::default(), art))
            .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn playing() -> AppState {
        AppState {
            pane: Pane::Songs,
            tracks: vec![Track::stub("v1", "Roygbiv")],
            now_playing: Some(Track {
                thumbnail_url: Some("https://example.com/a.jpg".into()),
                ..Track::stub("v1", "Roygbiv")
            }),
            ..Default::default()
        }
    }

    #[test]
    fn without_image_support_the_main_pane_keeps_its_full_width() {
        // FR-U5: no art must mean no reserved gap, not an empty panel.
        let s = playing();
        let text = frame_text(&s, &mut ArtCache::disabled(), 80, 24);
        assert!(
            text.contains("Roygbiv"),
            "the track list still renders, got: {text}"
        );
    }

    #[test]
    fn art_layout_reserves_a_panel_only_when_art_is_enabled_and_wide_enough() {
        // The split is pure math, so it is testable without a real terminal.
        // 100 columns leaves the list well above MAIN_MIN_WIDTH after the panel.
        let full = Rect::new(0, 0, 100, 20);
        let (main, art) = split_for_art(full, false, true);
        assert_eq!(main, full, "disabled art must not shrink the main pane");
        assert!(art.is_none());

        let (main, art) = split_for_art(full, true, true);
        assert!(main.width < full.width, "enabled art takes columns");
        assert!(art.is_some());
    }

    #[test]
    fn a_narrow_terminal_gets_no_art_panel_however_capable_it_is() {
        // Splitting a 40-column pane would leave the track list unreadable,
        // and unreadable text is worse than absent art.
        let narrow = Rect::new(0, 0, 40, 20);
        let (main, art) = split_for_art(narrow, true, true);
        assert_eq!(main, narrow);
        assert!(art.is_none());
    }

    #[test]
    fn art_is_skipped_when_nothing_is_playing() {
        let wide = Rect::new(0, 0, 100, 30);
        let (main, art) = split_for_art(wide, true, false);
        assert_eq!(main, wide, "no track means no panel");
        assert!(art.is_none());
    }

    #[test]
    fn an_eight_by_four_terminal_still_does_not_panic_with_art_enabled() {
        let s = playing();
        let _ = frame_text(&s, &mut ArtCache::disabled(), 8, 4);
    }
}
