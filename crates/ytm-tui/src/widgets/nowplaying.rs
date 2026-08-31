//! The bottom bar: what is playing, where we are in it.

use crate::{
    app::AppState,
    theme::Theme,
    util::text::{display_width, truncate_to_width},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use ytm_player::player::{PlaybackState, RepeatMode};

/// Eighth-block glyphs give 8x the resolution of a plain block per column.
const EIGHTHS: [char; 8] = [
    '\u{258F}', '\u{258E}', '\u{258D}', '\u{258C}', '\u{258B}', '\u{258A}', '\u{2589}', '\u{2588}',
];

pub fn progress_bar(ratio: f64, width: usize) -> String {
    let r = if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    };
    let total_eighths = (r * (width * 8) as f64).round() as usize;
    let full = total_eighths / 8;
    let rem = total_eighths % 8;

    let mut s: String = std::iter::repeat_n('\u{2588}', full.min(width)).collect();
    let partial = full < width && rem > 0;
    if partial {
        s.push(EIGHTHS[rem - 1]);
    }
    // Pad with spaces so the bar always occupies its full width.
    let filled = full.min(width) + usize::from(partial);
    s.extend(std::iter::repeat_n(' ', width.saturating_sub(filled)));
    s
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    let w = area.width as usize;

    // Row 1: title -- artist, plus the state glyph.
    let line = match &s.now_playing {
        None => Line::from(Span::styled(
            "Nothing playing",
            Style::default().fg(t.fg_dim),
        )),
        Some(track) => {
            let glyph = match s.playback {
                PlaybackState::Playing => "\u{25B6}",
                PlaybackState::Paused => "\u{23F8}",
                PlaybackState::Loading => "\u{22EF}",
                PlaybackState::Stopped => "\u{25A0}",
            };
            let title = truncate_to_width(&track.title, w.saturating_sub(24));
            Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(t.accent)),
                Span::styled(
                    title,
                    Style::default()
                        .fg(t.fg_bright)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" \u{2014} ", Style::default().fg(t.fg_dim)),
                Span::styled(track.artist_display(), Style::default().fg(t.fg)),
            ])
        }
    };
    f.render_widget(Paragraph::new(line), rows[0]);

    // Row 2: elapsed, bar, duration, then the mode flags.
    let pos = s.position.as_secs();
    let dur = s.duration.as_secs();
    let ratio = if dur == 0 {
        0.0
    } else {
        pos as f64 / dur as f64
    };

    let times = format!("{}  ", s.position);
    let tail = format!("  {}", s.duration);
    let flags = format!(
        "  {}{}  {:>3}%",
        if s.shuffle { "\u{21C4}" } else { " " },
        match s.repeat {
            RepeatMode::Off => " ",
            RepeatMode::One => "\u{2460}",
            RepeatMode::All => "\u{21BB}",
        },
        if s.muted { 0 } else { s.volume },
    );

    // Columns, not bytes: the flag glyphs are multi-byte but one column wide.
    let chrome = display_width(&times) + display_width(&tail) + display_width(&flags);
    let bar_w = w.saturating_sub(chrome);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(times, Style::default().fg(t.fg_dim)),
            Span::styled(progress_bar(ratio, bar_w), Style::default().fg(t.accent)),
            Span::styled(tail, Style::default().fg(t.fg_dim)),
            Span::styled(flags, Style::default().fg(t.fg_dim)),
        ])),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::AppState, theme::Theme};
    use ratatui::{Terminal, backend::TestBackend};
    use ytm_core::{Track, TrackDuration};
    use ytm_player::player::PlaybackState;

    fn buffer_text(state: &AppState) -> String {
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::default();
        t.draw(|f| {
            crate::render::render(
                f,
                state,
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
    fn progress_bar_is_empty_at_zero_and_full_at_one() {
        assert_eq!(progress_bar(0.0, 10).trim_end(), "");
        assert_eq!(progress_bar(1.0, 10), "██████████");
    }

    #[test]
    fn progress_bar_uses_partial_blocks_for_sub_cell_precision() {
        // Spec §6: eighth-blocks, not '='.
        // The plan wrote this as a `'\u{258F}'..='\u{2588}'` range, which is
        // inverted — 258F > 2588, so the range is empty and the assertion could
        // never hold. Checking the seven *partial* glyphs directly is what it
        // meant, and is stricter: a bar of solid full blocks does not pass.
        let b = progress_bar(0.55, 10);
        assert!(b.chars().any(|c| EIGHTHS[..7].contains(&c)), "got {b:?}");
    }

    #[test]
    fn progress_bar_clamps_out_of_range_ratios() {
        assert_eq!(progress_bar(-1.0, 5).trim_end(), "");
        assert_eq!(progress_bar(2.0, 5), "█████");
    }

    #[test]
    fn now_playing_shows_title_artist_and_times() {
        let s = AppState {
            now_playing: Some(Track::stub("v1", "Roygbiv")),
            playback: PlaybackState::Playing,
            position: TrackDuration::from_secs(65),
            duration: TrackDuration::from_secs(149),
            ..Default::default()
        };
        let text = buffer_text(&s);
        assert!(text.contains("Roygbiv"), "title missing");
        assert!(text.contains("1:05"), "elapsed missing");
        assert!(text.contains("2:29"), "duration missing");
    }

    #[test]
    fn idle_state_shows_a_placeholder_not_an_empty_bar() {
        let s = AppState::default();
        assert!(buffer_text(&s).contains("Nothing playing"));
    }

    #[test]
    fn sidebar_lists_every_source() {
        let text = buffer_text(&AppState::default());
        for label in ["Playlists", "Songs", "Albums", "Artists", "Search", "Queue"] {
            assert!(text.contains(label), "sidebar missing {label}");
        }
    }

    #[test]
    fn a_very_narrow_terminal_does_not_panic() {
        // Users resize to absurd sizes; a panic here loses their session.
        let mut t = Terminal::new(TestBackend::new(8, 4)).unwrap();
        let s = AppState::default();
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
    }
}
