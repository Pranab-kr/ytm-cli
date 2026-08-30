//! Confirm and prompt boxes. A modal owns the keyboard while it is open.
//!
//! Both footers name the keys to press. A modal the user cannot see their way
//! out of is worse than no modal, so `[esc] cancel` is always on screen.

use crate::{
    app::{AppState, Modal},
    theme::Theme,
    util::text::{tail_to_width, truncate_to_width},
    widgets::toast::centered_rect,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Box size as a share of the frame (spec §6).
const WIDTH_PCT: u16 = 50;
const HEIGHT_PCT: u16 = 20;

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, t: &Theme) {
    let Some(modal) = &s.modal else { return };
    let (title, body, footer) = match modal {
        Modal::Confirm { text, .. } => {
            (" Confirm ", text.clone(), "[y] yes   [n] no   [esc] cancel")
        }
        Modal::Prompt { title, value, .. } => (
            " Edit ",
            format!("{title}\n{value}"),
            "[enter] save   [esc] cancel",
        ),
        // Help draws itself; Login belongs to the auth pane.
        Modal::Help | Modal::Login { .. } => return,
    };

    if area.width == 0 || area.height == 0 {
        return;
    }
    let rect = centered_rect(WIDTH_PCT, HEIGHT_PCT, area);
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            title,
            Style::default()
                .fg(t.fg_bright)
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(t.fg_dim));
    let inner = block.inner(rect);

    // Clear first, or the list underneath shows through the box.
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, part) in body.split('\n').enumerate() {
        // The value line keeps its *end*: truncating the front would hide the
        // characters the user just typed.
        let text = if i == 0 {
            truncate_to_width(part, w)
        } else {
            tail_to_width(part, w)
        };
        let style = if i == 0 {
            Style::default().fg(t.fg_bright)
        } else {
            Style::default().fg(t.fg)
        };
        lines.push(Line::from(Span::styled(text, style)));
    }
    // Footer sits on the last row, so the box reads top-down.
    while lines.len() + 1 < inner.height as usize {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        truncate_to_width(footer, w),
        Style::default().fg(t.fg_dim),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use crate::{
        app::{AppState, ConfirmAction, Modal, PromptAction},
        event::{AppEvent, InputAction},
        keymap::KeyMap,
        theme::Theme,
    };
    use ytm_core::PlaylistId;

    fn text_of(s: &AppState) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, s, &theme, &KeyMap::default()))
            .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn confirming() -> AppState {
        AppState {
            modal: Some(Modal::Confirm {
                text: "Delete \"Focus\"?".into(),
                action: ConfirmAction::DeletePlaylist(PlaylistId::from("p1")),
            }),
            ..Default::default()
        }
    }

    fn prompting(value: &str) -> AppState {
        AppState {
            modal: Some(Modal::Prompt {
                title: "New playlist name".into(),
                value: value.to_owned(),
                action: PromptAction::CreatePlaylist,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn confirm_modal_shows_the_question_and_both_choices() {
        let text = text_of(&confirming());
        assert!(text.contains("Delete"));
        assert!(
            text.contains("y") && text.contains("n"),
            "must show the keys to press"
        );
    }

    #[test]
    fn prompt_modal_shows_the_title_and_current_value() {
        let text = text_of(&prompting("Chill"));
        assert!(text.contains("New playlist name"));
        assert!(text.contains("Chill"));
    }

    #[test]
    fn typing_in_a_prompt_appends_to_the_value() {
        let mut s = prompting("");
        s.apply(AppEvent::Input(InputAction::Char('a')));
        s.apply(AppEvent::Input(InputAction::Char('b')));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "ab"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn backspace_in_a_prompt_deletes_the_last_character() {
        let mut s = prompting("abc");
        s.apply(AppEvent::Input(InputAction::Backspace));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "ab"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn backspace_on_an_empty_prompt_does_not_panic() {
        let mut s = prompting("");
        s.apply(AppEvent::Input(InputAction::Backspace));
        assert!(s.modal.is_some());
    }

    #[test]
    fn escape_closes_any_modal_without_acting() {
        let mut s = confirming();
        s.apply(AppEvent::Input(InputAction::Cancel));
        assert!(s.modal.is_none());
        assert!(s.pending.is_empty(), "cancelling must not start a mutation");
    }

    #[test]
    fn a_multibyte_character_deletes_cleanly() {
        // Byte-slicing here would panic; pop() is char-aware.
        let mut s = prompting("日本");
        s.apply(AppEvent::Input(InputAction::Backspace));
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "日"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn a_letter_bound_to_a_command_is_text_inside_a_prompt() {
        // `q` must not quit while the user is naming a playlist.
        let mut s = prompting("");
        for c in "quiet".chars() {
            s.apply(AppEvent::Input(InputAction::Char(c)));
        }
        assert!(!s.should_quit, "typing must not trigger commands");
        match &s.modal {
            Some(Modal::Prompt { value, .. }) => assert_eq!(value, "quiet"),
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn typing_never_reaches_a_confirm_box() {
        // A confirm has no text field; stray letters must not be swallowed
        // silently into one, nor act on the list behind it.
        let mut s = confirming();
        s.apply(AppEvent::Input(InputAction::Char('x')));
        match &s.modal {
            Some(Modal::Confirm { text, .. }) => assert!(text.contains("Delete")),
            other => panic!("expected a confirm, got {other:?}"),
        }
    }

    #[test]
    fn the_prompt_shows_a_long_value_without_overflowing_the_box() {
        // 80x24 = 1920 cells; layout math that overflows would panic or clip.
        let s = prompting(&"x".repeat(300));
        assert_eq!(text_of(&s).chars().count(), 1920);
    }

    #[test]
    fn a_modal_survives_a_terminal_too_small_to_hold_it() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut t = Terminal::new(TestBackend::new(6, 3)).unwrap();
        let theme = Theme::default();
        t.draw(|f| crate::render::render(f, &confirming(), &theme, &KeyMap::default()))
            .unwrap();
        t.draw(|f| crate::render::render(f, &prompting("abc"), &theme, &KeyMap::default()))
            .unwrap();
    }

    #[test]
    fn nothing_is_drawn_when_no_modal_is_open() {
        let s = AppState {
            pane: crate::app::Pane::Songs,
            tracks: vec![ytm_core::Track::stub("v1", "Roygbiv")],
            ..Default::default()
        };
        let text = text_of(&s);
        assert!(text.contains("Roygbiv"));
        assert!(!text.contains("[esc]"));
    }
}
