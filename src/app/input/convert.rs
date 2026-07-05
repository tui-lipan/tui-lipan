use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};

pub(crate) fn to_key_event(k: crossterm::event::KeyEvent) -> Option<KeyEvent> {
    if !matches!(
        k.kind,
        crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
    ) {
        return None;
    }

    let mods = KeyMods {
        ctrl: k
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL),
        alt: k.modifiers.contains(crossterm::event::KeyModifiers::ALT),
        shift: k.modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
        super_key: k.modifiers.contains(crossterm::event::KeyModifiers::SUPER),
    };

    let code = match k.code {
        crossterm::event::KeyCode::Char(c) => KeyCode::Char(c),
        crossterm::event::KeyCode::F(n) => KeyCode::F(n),
        crossterm::event::KeyCode::Insert => KeyCode::Insert,
        crossterm::event::KeyCode::Enter => KeyCode::Enter,
        crossterm::event::KeyCode::Esc => KeyCode::Esc,
        crossterm::event::KeyCode::BackTab => KeyCode::BackTab,
        crossterm::event::KeyCode::Tab => KeyCode::Tab,
        crossterm::event::KeyCode::Backspace => KeyCode::Backspace,
        crossterm::event::KeyCode::Delete => KeyCode::Delete,
        crossterm::event::KeyCode::Home => KeyCode::Home,
        crossterm::event::KeyCode::End => KeyCode::End,
        crossterm::event::KeyCode::PageUp => KeyCode::PageUp,
        crossterm::event::KeyCode::PageDown => KeyCode::PageDown,
        crossterm::event::KeyCode::Up => KeyCode::Up,
        crossterm::event::KeyCode::Down => KeyCode::Down,
        crossterm::event::KeyCode::Left => KeyCode::Left,
        crossterm::event::KeyCode::Right => KeyCode::Right,
        _ => return None,
    };

    Some(KeyEvent { code, mods })
}

pub(crate) fn to_mouse_event(m: crossterm::event::MouseEvent) -> Option<MouseEvent> {
    let mods = KeyMods {
        ctrl: m
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL),
        alt: m.modifiers.contains(crossterm::event::KeyModifiers::ALT),
        shift: m.modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
        super_key: m.modifiers.contains(crossterm::event::KeyModifiers::SUPER),
    };

    let kind = match m.kind {
        crossterm::event::MouseEventKind::Down(btn) => MouseKind::Down(to_mouse_button(btn)?),
        crossterm::event::MouseEventKind::Up(btn) => MouseKind::Up(to_mouse_button(btn)?),
        crossterm::event::MouseEventKind::Drag(btn) => MouseKind::Drag(to_mouse_button(btn)?),
        crossterm::event::MouseEventKind::Moved => MouseKind::Moved,
        crossterm::event::MouseEventKind::ScrollDown => MouseKind::ScrollDown,
        crossterm::event::MouseEventKind::ScrollUp => MouseKind::ScrollUp,
        crossterm::event::MouseEventKind::ScrollLeft
        | crossterm::event::MouseEventKind::ScrollRight => return None,
    };

    Some(MouseEvent {
        x: m.column,
        y: m.row,
        kind,
        mods,
    })
}

pub(crate) fn translate_mouse_to_viewport(
    event: MouseEvent,
    origin_x: u16,
    origin_y: u16,
    width: u16,
    height: u16,
) -> Option<MouseEvent> {
    if width == 0 || height == 0 {
        return None;
    }

    let max_x = origin_x.saturating_add(width);
    let max_y = origin_y.saturating_add(height);
    if event.x < origin_x || event.x >= max_x || event.y < origin_y || event.y >= max_y {
        return None;
    }

    Some(MouseEvent {
        x: event.x.saturating_sub(origin_x),
        y: event.y.saturating_sub(origin_y),
        ..event
    })
}

fn to_mouse_button(btn: crossterm::event::MouseButton) -> Option<MouseButton> {
    match btn {
        crossterm::event::MouseButton::Left => Some(MouseButton::Left),
        crossterm::event::MouseButton::Right => Some(MouseButton::Right),
        crossterm::event::MouseButton::Middle => Some(MouseButton::Middle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers,
    };

    #[test]
    fn function_key_is_translated() {
        let key = CrosstermKeyEvent::new(CrosstermKeyCode::F(12), KeyModifiers::empty());
        let translated = to_key_event(key).expect("function keys translate");
        assert!(matches!(translated.code, KeyCode::F(12)));
    }

    #[test]
    fn supported_key_is_translated() {
        let key = CrosstermKeyEvent::new(CrosstermKeyCode::Enter, KeyModifiers::empty());
        let translated = to_key_event(key).expect("supported keys translate");
        assert!(matches!(translated.code, KeyCode::Enter));
    }

    #[test]
    fn translates_modifiers() {
        let key = CrosstermKeyEvent::new(
            CrosstermKeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let translated = to_key_event(key).expect("translated");
        assert_eq!(translated.code, KeyCode::Char('c'));
        assert!(translated.mods.ctrl);
        assert!(translated.mods.shift);
        assert!(!translated.mods.alt);
        assert!(!translated.mods.super_key);
    }

    #[test]
    fn translates_all_supported_keys() {
        let cases = [
            (CrosstermKeyCode::Left, KeyCode::Left),
            (CrosstermKeyCode::Right, KeyCode::Right),
            (CrosstermKeyCode::Up, KeyCode::Up),
            (CrosstermKeyCode::Down, KeyCode::Down),
            (CrosstermKeyCode::Home, KeyCode::Home),
            (CrosstermKeyCode::End, KeyCode::End),
            (CrosstermKeyCode::PageUp, KeyCode::PageUp),
            (CrosstermKeyCode::PageDown, KeyCode::PageDown),
            (CrosstermKeyCode::Enter, KeyCode::Enter),
            (CrosstermKeyCode::Esc, KeyCode::Esc),
            (CrosstermKeyCode::Tab, KeyCode::Tab),
            (CrosstermKeyCode::Backspace, KeyCode::Backspace),
            (CrosstermKeyCode::Delete, KeyCode::Delete),
            (CrosstermKeyCode::Insert, KeyCode::Insert),
            (CrosstermKeyCode::F(12), KeyCode::F(12)),
            (CrosstermKeyCode::Char('a'), KeyCode::Char('a')),
        ];

        for (ct_code, expected) in cases {
            let key = CrosstermKeyEvent::new(ct_code, KeyModifiers::empty());
            let translated =
                to_key_event(key).unwrap_or_else(|| panic!("failed to translate {:?}", ct_code));
            assert_eq!(translated.code, expected, "mismatch for {:?}", ct_code);
        }
    }

    #[test]
    fn ignores_unsupported_keys() {
        let unsupported = [
            CrosstermKeyCode::Null,
            CrosstermKeyCode::CapsLock,
            CrosstermKeyCode::ScrollLock,
            CrosstermKeyCode::NumLock,
            CrosstermKeyCode::PrintScreen,
            CrosstermKeyCode::Pause,
            CrosstermKeyCode::Menu,
            CrosstermKeyCode::KeypadBegin,
        ];

        for code in unsupported {
            let key = CrosstermKeyEvent::new(code, KeyModifiers::empty());
            assert!(
                to_key_event(key).is_none(),
                "expected {:?} to be ignored",
                code
            );
        }
    }

    #[test]
    fn mouse_translation_maps_to_viewport_local_coords() {
        let event = MouseEvent {
            x: 8,
            y: 13,
            kind: MouseKind::Moved,
            mods: KeyMods::default(),
        };

        let translated = translate_mouse_to_viewport(event, 2, 10, 20, 5)
            .expect("event inside viewport should translate");

        assert_eq!(translated.x, 6);
        assert_eq!(translated.y, 3);
        assert_eq!(translated.kind, MouseKind::Moved);
    }

    #[test]
    fn mouse_translation_drops_out_of_bounds_events() {
        let event = MouseEvent {
            x: 8,
            y: 16,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        };

        let translated = translate_mouse_to_viewport(event, 2, 10, 20, 5);
        assert!(translated.is_none());
    }
}
