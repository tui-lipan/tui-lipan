//! Key script parsing for scripted captures.
//!
//! A capture that can only render the app as it starts cannot reach a modal, an
//! error state, or anything a few keystrokes deep. A key script such as
//! `"tab,tab,enter"` is dispatched before capturing, so those states are
//! reachable without writing a harness.
//!
//! Scripts reuse the ordinary keybinding syntax (`ctrl+n`, `f12`, `esc`), so
//! there is one spelling of a key across keymaps, docs, and captures.

use std::str::FromStr;

use crate::Result;
use crate::core::event::KeyEvent;
use crate::input::KeyBindings;

/// Parse a comma-separated key script into individual key events.
///
/// Each entry uses keybinding syntax; a chord such as `"ctrl+x ctrl+s"` expands
/// to one event per step, in order.
///
/// Returns an error for unparseable entries rather than skipping them, since a
/// silently dropped keystroke produces a capture of the wrong state.
pub(crate) fn parse_key_script(raw: &str) -> Result<Vec<KeyEvent>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let bindings = KeyBindings::from_str(raw)
        .map_err(|err| std::io::Error::other(format!("invalid key script `{raw}`: {err}")))?;

    let mut events = Vec::new();
    for binding in bindings.iter() {
        let expanded = binding.key_events().map_err(|err| {
            std::io::Error::other(format!("unsupported key in script `{raw}`: {err}"))
        })?;
        events.extend(expanded);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::KeyCode;

    #[test]
    fn empty_script_yields_no_events() {
        assert!(parse_key_script("").expect("empty is valid").is_empty());
        assert!(parse_key_script("   ").expect("blank is valid").is_empty());
    }

    #[test]
    fn comma_separated_keys_expand_in_order() {
        let events = parse_key_script("tab,tab,enter").expect("parses");
        let codes: Vec<KeyCode> = events.iter().map(|event| event.code).collect();
        assert_eq!(
            codes,
            vec![KeyCode::Tab, KeyCode::Tab, KeyCode::Enter],
            "script order must be preserved"
        );
    }

    #[test]
    fn modifiers_are_parsed() {
        let events = parse_key_script("ctrl+n").expect("parses");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, KeyCode::Char('n'));
        assert!(events[0].mods.ctrl, "ctrl modifier should be set");
    }

    #[test]
    fn chords_expand_to_one_event_per_step() {
        let events = parse_key_script("ctrl+x ctrl+s").expect("parses");
        assert_eq!(events.len(), 2, "each chord step is its own event");
    }

    #[test]
    fn whitespace_around_entries_is_tolerated() {
        let events = parse_key_script(" tab , enter ").expect("parses");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn an_unparseable_entry_is_an_error_not_a_silent_skip() {
        assert!(parse_key_script("tab,definitely-not-a-key").is_err());
    }
}
