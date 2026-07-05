use tui_lipan::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};

pub fn mouse_event_from_raw(
    x: i32,
    y: i32,
    button: u8,
    phase: u8,
    is_wheel: bool,
    shift: bool,
    alt: bool,
    ctrl: bool,
) -> Option<MouseEvent> {
    let x = x.clamp(0, i32::from(u16::MAX)) as u16;
    let y = y.clamp(0, i32::from(u16::MAX)) as u16;
    let kind = if is_wheel {
        match button {
            0 => MouseKind::ScrollUp,
            1 => MouseKind::ScrollDown,
            _ => return None,
        }
    } else {
        let button = match button {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => return None,
        };
        match phase {
            0 => MouseKind::Down(button),
            1 => MouseKind::Up(button),
            2 => MouseKind::Drag(button),
            _ => MouseKind::Moved,
        }
    };

    Some(MouseEvent {
        x,
        y,
        kind,
        mods: KeyMods {
            ctrl,
            alt,
            shift,
            super_key: false,
        },
    })
}
