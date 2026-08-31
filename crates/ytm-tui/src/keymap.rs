//! Key -> InputAction resolution. Focus-sensitive: while typing, letters are
//! letters, not commands.

use crate::{app::Focus, event::InputAction};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

pub struct KeyMap {
    /// Char bindings active in navigation focus.
    chars: HashMap<char, InputAction>,
}

impl Default for KeyMap {
    fn default() -> Self {
        let mut chars = HashMap::new();
        for (c, a) in [
            ('j', InputAction::Down),
            ('k', InputAction::Up),
            ('h', InputAction::Left),
            ('l', InputAction::Right),
            ('g', InputAction::Home),
            ('G', InputAction::End),
            ('q', InputAction::Quit),
            ('?', InputAction::OpenHelp),
            ('/', InputAction::OpenSearch),
            ('u', InputAction::OpenQueue),
            (' ', InputAction::TogglePause),
            ('n', InputAction::NextTrack),
            ('p', InputAction::PrevTrack),
            ('f', InputAction::SeekForward),
            ('b', InputAction::SeekBack),
            ('+', InputAction::VolumeUp),
            ('-', InputAction::VolumeDown),
            ('m', InputAction::ToggleMute),
            ('s', InputAction::ToggleShuffle),
            ('r', InputAction::CycleRepeat),
            ('a', InputAction::AddToQueue),
            ('A', InputAction::AddToPlaylist),
            ('N', InputAction::CreatePlaylist),
            ('R', InputAction::RenamePlaylist),
            ('D', InputAction::DeletePlaylist),
            ('x', InputAction::RemoveFromPlaylist),
            ('v', InputAction::ToggleMark),
            ('e', InputAction::PlayNext),
            ('J', InputAction::MoveEntryDown),
            ('K', InputAction::MoveEntryUp),
            ('C', InputAction::ClearQueue),
            ('L', InputAction::Refresh),
        ] {
            chars.insert(c, a);
        }
        Self { chars }
    }
}

impl KeyMap {
    pub fn resolve(&self, key: KeyEvent, focus: Focus) -> Option<InputAction> {
        // Ctrl-C escapes everything, including a text field.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Some(InputAction::Quit),
                KeyCode::Char('d') => Some(InputAction::PageDown),
                KeyCode::Char('u') => Some(InputAction::PageUp),
                _ => None,
            };
        }

        if focus == Focus::SearchInput {
            return match key.code {
                KeyCode::Char(c) => Some(InputAction::Char(c)),
                KeyCode::Backspace => Some(InputAction::Backspace),
                KeyCode::Esc => Some(InputAction::Cancel),
                KeyCode::Enter => Some(InputAction::Confirm),
                KeyCode::Down => Some(InputAction::Down),
                KeyCode::Up => Some(InputAction::Up),
                _ => None,
            };
        }

        match key.code {
            // 1-6 jump to a source, checked before the char table so a digit
            // cannot be rebound to something else by accident. Only the digits
            // that name a source are bound; 7-9 and 0 fall through to `None`
            // rather than being swallowed.
            KeyCode::Char(c @ '1'..='6') => Some(InputAction::GoTo(c as u8 - b'0')),
            KeyCode::Char(c) => self.chars.get(&c).cloned(),
            KeyCode::Down => Some(InputAction::Down),
            KeyCode::Up => Some(InputAction::Up),
            KeyCode::Left => Some(InputAction::Left),
            KeyCode::Right => Some(InputAction::Right),
            KeyCode::Home => Some(InputAction::Home),
            KeyCode::End => Some(InputAction::End),
            KeyCode::PageDown => Some(InputAction::PageDown),
            KeyCode::PageUp => Some(InputAction::PageUp),
            KeyCode::Enter => Some(InputAction::Confirm),
            KeyCode::Esc => Some(InputAction::Cancel),
            KeyCode::Tab => Some(InputAction::NextPane),
            KeyCode::BackTab => Some(InputAction::PrevPane),
            _ => None,
        }
    }

    /// For the help overlay, sorted for stable display.
    pub fn bindings(&self) -> Vec<(String, InputAction)> {
        let mut v: Vec<_> = self
            .chars
            .iter()
            .map(|(c, a)| (c.to_string(), a.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Override defaults from `[keys]` in config. Key names are snake_case
    /// action names; values are single characters.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        let table: HashMap<String, String> = toml::from_str(s)?;
        let mut m = Self::default();
        for (action_name, ch) in table {
            let Some(c) = ch.chars().next() else { continue };
            if let Some(action) = action_from_name(&action_name) {
                m.chars.retain(|_, a| *a != action); // a binding is exclusive
                m.chars.insert(c, action);
            }
        }
        Ok(m)
    }
}

fn action_from_name(n: &str) -> Option<InputAction> {
    Some(match n {
        "down" => InputAction::Down,
        "up" => InputAction::Up,
        "left" => InputAction::Left,
        "right" => InputAction::Right,
        "quit" => InputAction::Quit,
        "toggle_pause" => InputAction::TogglePause,
        "next_track" => InputAction::NextTrack,
        "prev_track" => InputAction::PrevTrack,
        "toggle_shuffle" => InputAction::ToggleShuffle,
        "cycle_repeat" => InputAction::CycleRepeat,
        "open_search" => InputAction::OpenSearch,
        "open_queue" => InputAction::OpenQueue,
        "open_help" => InputAction::OpenHelp,
        "move_entry_up" => InputAction::MoveEntryUp,
        "move_entry_down" => InputAction::MoveEntryDown,
        "clear_queue" => InputAction::ClearQueue,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Focus;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn vim_navigation_keys_map_to_movement() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('j'), Focus::Main), Some(InputAction::Down));
        assert_eq!(m.resolve(key('k'), Focus::Main), Some(InputAction::Up));
        assert_eq!(m.resolve(key('h'), Focus::Main), Some(InputAction::Left));
        assert_eq!(m.resolve(key('l'), Focus::Main), Some(InputAction::Right));
    }

    #[test]
    fn arrow_keys_work_alongside_vim_keys() {
        let m = KeyMap::default();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(m.resolve(down, Focus::Main), Some(InputAction::Down));
    }

    #[test]
    fn transport_keys_are_bound() {
        let m = KeyMap::default();
        assert_eq!(
            m.resolve(key(' '), Focus::Main),
            Some(InputAction::TogglePause)
        );
        assert_eq!(
            m.resolve(key('n'), Focus::Main),
            Some(InputAction::NextTrack)
        );
        assert_eq!(
            m.resolve(key('p'), Focus::Main),
            Some(InputAction::PrevTrack)
        );
        assert_eq!(
            m.resolve(key('s'), Focus::Main),
            Some(InputAction::ToggleShuffle)
        );
        assert_eq!(
            m.resolve(key('r'), Focus::Main),
            Some(InputAction::CycleRepeat)
        );
    }

    #[test]
    fn typing_in_the_search_field_produces_characters_not_commands() {
        // Critical: 'j' while typing must insert a letter, not scroll the list.
        let m = KeyMap::default();
        assert_eq!(
            m.resolve(key('j'), Focus::SearchInput),
            Some(InputAction::Char('j'))
        );
        assert_eq!(
            m.resolve(key(' '), Focus::SearchInput),
            Some(InputAction::Char(' '))
        );
    }

    #[test]
    fn every_letter_is_text_while_a_prompt_is_open() {
        // Resolved against the focus a prompt reports, so a name containing
        // command letters types normally.
        let m = KeyMap::default();
        for c in "quit and next".chars() {
            assert_eq!(
                m.resolve(key(c), Focus::SearchInput),
                Some(InputAction::Char(c)),
                "{c:?} must be text"
            );
        }
    }

    #[test]
    fn escape_cancels_from_the_search_field() {
        let m = KeyMap::default();
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            m.resolve(esc, Focus::SearchInput),
            Some(InputAction::Cancel)
        );
    }

    #[test]
    fn ctrl_c_always_quits_even_while_typing() {
        let m = KeyMap::default();
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(m.resolve(c, Focus::SearchInput), Some(InputAction::Quit));
        assert_eq!(m.resolve(c, Focus::Main), Some(InputAction::Quit));
    }

    #[test]
    fn the_queue_editing_keys_are_reachable() {
        // The dispatch tests pass actions in directly, so without this nothing
        // proves a user can actually produce them.
        let m = KeyMap::default();
        assert_eq!(
            m.resolve(key('J'), Focus::Main),
            Some(InputAction::MoveEntryDown)
        );
        assert_eq!(
            m.resolve(key('K'), Focus::Main),
            Some(InputAction::MoveEntryUp)
        );
        assert_eq!(
            m.resolve(key('C'), Focus::Main),
            Some(InputAction::ClearQueue)
        );
        // Lowercase must stay navigation, or reordering would hijack j/k.
        assert_eq!(m.resolve(key('j'), Focus::Main), Some(InputAction::Down));
        assert_eq!(m.resolve(key('k'), Focus::Main), Some(InputAction::Up));
    }

    #[test]
    fn unbound_keys_resolve_to_nothing() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('Z'), Focus::Main), None);
    }

    #[test]
    fn a_user_override_replaces_the_default_binding() {
        let m = KeyMap::from_toml_str(r#"down = "e""#).unwrap();
        assert_eq!(m.resolve(key('e'), Focus::Main), Some(InputAction::Down));
    }

    #[test]
    fn bindings_list_is_non_empty_for_the_help_overlay() {
        // FR-U2: '?' must show something real.
        assert!(!KeyMap::default().bindings().is_empty());
    }

    #[test]
    fn number_keys_jump_straight_to_a_source() {
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('1'), Focus::Main), Some(InputAction::GoTo(1)));
        assert_eq!(m.resolve(key('6'), Focus::Main), Some(InputAction::GoTo(6)));
    }

    #[test]
    fn digits_outside_the_source_range_are_not_bound() {
        // 7-9 and 0 name no source; binding them would swallow the key.
        let m = KeyMap::default();
        assert_eq!(m.resolve(key('7'), Focus::Main), None);
        assert_eq!(m.resolve(key('0'), Focus::Main), None);
    }

    #[test]
    fn a_digit_typed_into_the_search_field_is_a_character_not_a_jump() {
        // Searching for "90's" must be possible.
        let m = KeyMap::default();
        assert_eq!(
            m.resolve(key('9'), Focus::SearchInput),
            Some(InputAction::Char('9'))
        );
        assert_eq!(
            m.resolve(key('1'), Focus::SearchInput),
            Some(InputAction::Char('1'))
        );
    }
}
