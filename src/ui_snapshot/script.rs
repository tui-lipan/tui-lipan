//! Action scripts for driving a UI without a human.
//!
//! A key script can only type. An action script can also click, hover, focus,
//! scroll, drag, and wait, which is what it takes to reach a modal behind a
//! button or a row behind a scroll.
//!
//! # Targeting
//!
//! Widgets are addressed by **reconciliation key** (`#submit`), not by pixel.
//! Coordinates rot: a layout change silently turns `click:42,7` into a click on
//! empty space and the script still "succeeds". A key either resolves or fails
//! loudly, and it survives the widget moving. Raw coordinates remain available
//! for the cases keys cannot express.
//!
//! # Syntax
//!
//! Actions are separated by `;` or newlines. `#name` targets a key; `col,row`
//! targets a cell.
//!
//! ```text
//! key:tab              send one key, in keybinding syntax (ctrl+n, esc, f12)
//! type:hello world     type literal text, one key event per character
//! click:#submit        left click the centre of the widget keyed `submit`
//! click:12,7           left click a cell
//! rclick:#row          right click
//! mclick:#tab          middle click
//! hover:#sidebar       move the pointer over a widget
//! focus:#email         focus a widget directly
//! focus:next           advance focus (also `focus:prev`)
//! scroll:#list,down    scroll over a widget (`up` or `down`)
//! scroll:down          scroll wherever the pointer is
//! drag:#a>#b           press on one widget, move to another, release
//! wait:500             advance the clock 500ms, ticking animations
//! sleep:500            wait 500ms of real time, pumping messages, for async work
//! ```

use std::time::Duration;

use crate::Result;
use crate::core::element::Key;
use crate::core::event::{KeyEvent, MouseButton};

/// Default milliseconds for a bare `wait:` with no value.
const DEFAULT_WAIT_MS: u64 = 250;

/// What a mouse action points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The centre of the widget carrying this reconciliation key.
    Key(Key),
    /// An exact cell in viewport coordinates.
    Cell {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
    },
}

/// Focus movement that does not name a widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusStep {
    /// Move to the next focusable widget.
    Next,
    /// Move to the previous focusable widget.
    Prev,
}

/// Scroll direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    /// Scroll up.
    Up,
    /// Scroll down.
    Down,
}

/// One step of an action script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Send a single key event.
    Key(KeyEvent),
    /// Type literal text, one key event per character.
    Type(String),
    /// Press and release a mouse button over a target.
    Click {
        /// Where to click.
        target: Target,
        /// Which button.
        button: MouseButton,
    },
    /// Move the pointer over a target without pressing.
    Hover(Target),
    /// Focus the widget carrying a key.
    FocusKey(Key),
    /// Move focus without naming a widget.
    Focus(FocusStep),
    /// Scroll over a target, or wherever the pointer is.
    Scroll {
        /// Where to scroll, or `None` for the current pointer position.
        target: Option<Target>,
        /// Direction.
        direction: ScrollDirection,
    },
    /// Press on one target, move to another, release.
    Drag {
        /// Press location.
        from: Target,
        /// Release location.
        to: Target,
    },
    /// Advance the clock, ticking animations.
    Wait(Duration),
    /// Wait in *real* time, pumping messages, so asynchronous work can land.
    ///
    /// The counterpart to [`Wait`](Self::Wait), which moves a virtual clock and therefore cannot make
    /// a subprocess answer or a socket deliver.
    Sleep(Duration),
}

/// Parse an action script into ordered steps.
///
/// Returns an error naming the offending step rather than skipping it: a
/// silently dropped action produces a capture of the wrong state, which is worse
/// than a failed run.
pub(crate) fn parse_script(raw: &str) -> Result<Vec<Action>> {
    let mut actions = Vec::new();
    for step in raw.split([';', '\n']) {
        let step = step.trim();
        if step.is_empty() {
            continue;
        }
        actions.push(parse_action(step)?);
    }
    Ok(actions)
}

/// Parse one `verb:argument` step.
fn parse_action(step: &str) -> Result<Action> {
    let (verb, arg) = match step.split_once(':') {
        Some((verb, arg)) => (verb.trim(), arg.trim()),
        None => (step, ""),
    };

    match verb {
        // `type` keeps its argument verbatim: leading spaces are typed text.
        "type" => Ok(Action::Type(
            step.split_once(':')
                .map(|(_, arg)| arg)
                .unwrap_or("")
                .into(),
        )),
        "key" => Ok(Action::Key(single_key(arg, step)?)),
        "click" => Ok(Action::Click {
            target: parse_target(arg, step)?,
            button: MouseButton::Left,
        }),
        "rclick" => Ok(Action::Click {
            target: parse_target(arg, step)?,
            button: MouseButton::Right,
        }),
        "mclick" => Ok(Action::Click {
            target: parse_target(arg, step)?,
            button: MouseButton::Middle,
        }),
        "hover" => Ok(Action::Hover(parse_target(arg, step)?)),
        "focus" => match arg {
            "next" => Ok(Action::Focus(FocusStep::Next)),
            "prev" => Ok(Action::Focus(FocusStep::Prev)),
            other => Ok(Action::FocusKey(parse_key_ref(other, step)?)),
        },
        "scroll" => parse_scroll(arg, step),
        "drag" => {
            let (from, to) = arg
                .split_once('>')
                .ok_or_else(|| invalid(step, "expected `drag:<from>><to>`"))?;
            Ok(Action::Drag {
                from: parse_target(from.trim(), step)?,
                to: parse_target(to.trim(), step)?,
            })
        }
        "wait" => {
            let ms = if arg.is_empty() {
                DEFAULT_WAIT_MS
            } else {
                arg.trim_end_matches("ms")
                    .trim()
                    .parse()
                    .map_err(|_| invalid(step, "expected milliseconds"))?
            };
            Ok(Action::Wait(Duration::from_millis(ms)))
        }
        "sleep" => {
            let ms = if arg.is_empty() {
                DEFAULT_WAIT_MS
            } else {
                arg.trim_end_matches("ms")
                    .trim()
                    .parse()
                    .map_err(|_| invalid(step, "expected milliseconds"))?
            };
            Ok(Action::Sleep(Duration::from_millis(ms)))
        }
        other => Err(invalid(
            step,
            &format!(
                "unknown action `{other}`; expected one of key, type, click, rclick, mclick, \
                 hover, focus, scroll, drag, wait, sleep"
            ),
        )),
    }
}

/// Parse `#key,direction` / `direction` for a scroll step.
fn parse_scroll(arg: &str, step: &str) -> Result<Action> {
    let (target, direction) = match arg.rsplit_once(',') {
        Some((target, direction)) => (Some(parse_target(target.trim(), step)?), direction.trim()),
        None => (None, arg),
    };
    let direction = match direction {
        "up" => ScrollDirection::Up,
        "down" => ScrollDirection::Down,
        other => {
            return Err(invalid(
                step,
                &format!("unknown scroll direction `{other}`; expected up or down"),
            ));
        }
    };
    Ok(Action::Scroll { target, direction })
}

/// Parse `#key` or `col,row`.
fn parse_target(arg: &str, step: &str) -> Result<Target> {
    if let Some(key) = arg.strip_prefix('#') {
        return Ok(Target::Key(parse_key_ref(key, step)?));
    }
    let (x, y) = arg
        .split_once(',')
        .ok_or_else(|| invalid(step, "expected `#key` or `col,row`"))?;
    Ok(Target::Cell {
        x: x.trim()
            .parse()
            .map_err(|_| invalid(step, "column must be a number"))?,
        y: y.trim()
            .parse()
            .map_err(|_| invalid(step, "row must be a number"))?,
    })
}

/// Parse a bare or `#`-prefixed key reference.
fn parse_key_ref(raw: &str, step: &str) -> Result<Key> {
    let name = raw.trim().trim_start_matches('#');
    if name.is_empty() {
        return Err(invalid(step, "expected a widget key"));
    }
    Ok(Key::from(name.to_owned()))
}

/// Parse exactly one key event from keybinding syntax.
fn single_key(arg: &str, step: &str) -> Result<KeyEvent> {
    let mut events = super::keys::parse_key_script(arg)?;
    if events.len() != 1 {
        return Err(invalid(
            step,
            "expected exactly one key; use several `key:` steps for a sequence",
        ));
    }
    Ok(events.remove(0))
}

/// Build a parse error naming the offending step.
fn invalid(step: &str, reason: &str) -> crate::Error {
    std::io::Error::other(format!("invalid action `{step}`: {reason}")).into()
}

/// Everything an action needs from whatever is hosting the UI.
///
/// Implemented for both the headless [`TestBackend`](crate::TestBackend) and the
/// live [`AppRunner`](crate::AppRunner), so a script means the same thing whether
/// it drives a recording, a snapshot, or an attached agent session.
pub(crate) trait ActionHost {
    /// Rect of the widget carrying `key`, if it is in the current tree.
    fn rect_of_key(&self, key: &Key) -> Option<crate::style::Rect>;
    /// Deliver one key event.
    fn perform_key(&mut self, key: KeyEvent) -> Result<()>;
    /// Deliver one mouse event.
    fn perform_mouse(&mut self, event: crate::core::event::MouseEvent) -> Result<()>;
    /// Focus the widget carrying `key`; returns whether focus moved.
    fn perform_focus_key(&mut self, key: &Key) -> Result<bool>;
    /// Move focus one step.
    fn perform_focus_step(&mut self, step: FocusStep) -> Result<()>;
    /// Advance the clock, ticking animations.
    fn perform_wait(&mut self, dt: Duration) -> Result<()>;
    /// Wait in real time, pumping messages, so asynchronous work can land.
    fn perform_sleep(&mut self, dt: Duration) -> Result<()>;
}

/// Resolve a target to a cell, erroring when a key is not on screen.
///
/// A missing key is a hard error: continuing would click whatever happens to be
/// at a stale coordinate, which is the failure mode key targeting exists to
/// prevent.
fn resolve(host: &impl ActionHost, target: &Target) -> Result<(u16, u16)> {
    match target {
        Target::Cell { x, y } => Ok((*x, *y)),
        Target::Key(key) => {
            let rect = host.rect_of_key(key).ok_or_else(|| {
                std::io::Error::other(format!(
                    "no widget with key `{}` is currently rendered",
                    key.as_ref()
                ))
            })?;
            if rect.w == 0 || rect.h == 0 {
                return Err(std::io::Error::other(format!(
                    "widget `{}` has zero area and cannot be pointed at",
                    key.as_ref()
                ))
                .into());
            }
            // Centre, so a click lands inside borders and padding. Rect origins
            // are signed because a scrolled-away widget sits off-screen; a
            // negative centre cannot be pointed at.
            let cx = i32::from(rect.x) + i32::from(rect.w / 2);
            let cy = i32::from(rect.y) + i32::from(rect.h / 2);
            if cx < 0 || cy < 0 {
                return Err(std::io::Error::other(format!(
                    "widget `{}` is off-screen at ({cx}, {cy}) and cannot be pointed at",
                    key.as_ref()
                ))
                .into());
            }
            Ok((cx as u16, cy as u16))
        }
    }
}

/// Build a mouse event at `(x, y)`.
fn mouse_at(x: u16, y: u16, kind: crate::core::event::MouseKind) -> crate::core::event::MouseEvent {
    crate::core::event::MouseEvent {
        x,
        y,
        kind,
        mods: crate::core::event::KeyMods::NONE,
    }
}

/// Run one action against `host`.
pub(crate) fn execute(host: &mut impl ActionHost, action: &Action) -> Result<()> {
    use crate::core::event::MouseKind;

    match action {
        Action::Key(key) => host.perform_key(*key),
        Action::Type(text) => {
            for ch in text.chars() {
                host.perform_key(KeyEvent {
                    code: crate::core::event::KeyCode::Char(ch),
                    mods: crate::core::event::KeyMods::NONE,
                })?;
            }
            Ok(())
        }
        Action::Click { target, button } => {
            let (x, y) = resolve(host, target)?;
            // Move first: widgets that track hover expect the pointer to arrive
            // before the press, exactly as a real terminal reports it.
            host.perform_mouse(mouse_at(x, y, MouseKind::Moved))?;
            host.perform_mouse(mouse_at(x, y, MouseKind::Down(*button)))?;
            host.perform_mouse(mouse_at(x, y, MouseKind::Up(*button)))
        }
        Action::Hover(target) => {
            let (x, y) = resolve(host, target)?;
            host.perform_mouse(mouse_at(x, y, MouseKind::Moved))
        }
        Action::FocusKey(key) => {
            if !host.perform_focus_key(key)? {
                return Err(std::io::Error::other(format!(
                    "widget `{}` is not focusable or is not rendered",
                    key.as_ref()
                ))
                .into());
            }
            Ok(())
        }
        Action::Focus(step) => host.perform_focus_step(*step),
        Action::Scroll { target, direction } => {
            let kind = match direction {
                ScrollDirection::Up => MouseKind::ScrollUp,
                ScrollDirection::Down => MouseKind::ScrollDown,
            };
            match target {
                Some(target) => {
                    let (x, y) = resolve(host, target)?;
                    host.perform_mouse(mouse_at(x, y, MouseKind::Moved))?;
                    host.perform_mouse(mouse_at(x, y, kind))
                }
                // No target: scroll wherever the pointer already is.
                None => host.perform_mouse(mouse_at(0, 0, kind)),
            }
        }
        Action::Drag { from, to } => {
            let (fx, fy) = resolve(host, from)?;
            let (tx, ty) = resolve(host, to)?;
            host.perform_mouse(mouse_at(fx, fy, MouseKind::Moved))?;
            host.perform_mouse(mouse_at(fx, fy, MouseKind::Down(MouseButton::Left)))?;
            // An intermediate drag step: threshold-based drags need motion
            // between press and release before they engage.
            host.perform_mouse(mouse_at(
                fx.midpoint(tx),
                fy.midpoint(ty),
                MouseKind::Drag(MouseButton::Left),
            ))?;
            host.perform_mouse(mouse_at(tx, ty, MouseKind::Drag(MouseButton::Left)))?;
            host.perform_mouse(mouse_at(tx, ty, MouseKind::Up(MouseButton::Left)))
        }
        Action::Wait(dt) => host.perform_wait(*dt),
        Action::Sleep(dt) => host.perform_sleep(*dt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::KeyCode;

    fn parse_one(step: &str) -> Action {
        let mut actions = parse_script(step).expect("parses");
        assert_eq!(actions.len(), 1);
        actions.remove(0)
    }

    #[test]
    fn steps_split_on_semicolons_and_newlines() {
        let actions = parse_script("key:tab; wait:10\nfocus:next").expect("parses");
        assert_eq!(actions.len(), 3);
        assert!(matches!(actions[2], Action::Focus(FocusStep::Next)));
    }

    #[test]
    fn blank_steps_are_ignored() {
        assert!(parse_script("  ;\n\n ; ").expect("parses").is_empty());
    }

    #[test]
    fn clicks_target_keys_or_cells() {
        assert_eq!(
            parse_one("click:#submit"),
            Action::Click {
                target: Target::Key(Key::from("submit")),
                button: MouseButton::Left,
            }
        );
        assert_eq!(
            parse_one("click:12,7"),
            Action::Click {
                target: Target::Cell { x: 12, y: 7 },
                button: MouseButton::Left,
            }
        );
    }

    #[test]
    fn button_variants_are_distinct() {
        for (step, expected) in [
            ("click:#a", MouseButton::Left),
            ("rclick:#a", MouseButton::Right),
            ("mclick:#a", MouseButton::Middle),
        ] {
            match parse_one(step) {
                Action::Click { button, .. } => assert_eq!(button, expected, "{step}"),
                other => panic!("expected a click, got {other:?}"),
            }
        }
    }

    #[test]
    fn type_preserves_spaces_and_punctuation() {
        assert_eq!(
            parse_one("type:hello, world!"),
            Action::Type("hello, world!".to_owned())
        );
    }

    #[test]
    fn focus_accepts_keys_and_steps() {
        assert_eq!(parse_one("focus:next"), Action::Focus(FocusStep::Next));
        assert_eq!(parse_one("focus:prev"), Action::Focus(FocusStep::Prev));
        assert_eq!(
            parse_one("focus:#email"),
            Action::FocusKey(Key::from("email"))
        );
        // A `#` prefix is accepted for symmetry with click/hover targets.
        assert_eq!(
            parse_one("focus:#email"),
            parse_one("focus:email"),
            "`#` should be optional for focus"
        );
    }

    #[test]
    fn scroll_parses_with_and_without_a_target() {
        assert_eq!(
            parse_one("scroll:#list,down"),
            Action::Scroll {
                target: Some(Target::Key(Key::from("list"))),
                direction: ScrollDirection::Down,
            }
        );
        assert_eq!(
            parse_one("scroll:up"),
            Action::Scroll {
                target: None,
                direction: ScrollDirection::Up,
            }
        );
    }

    #[test]
    fn scroll_over_a_cell_keeps_the_coordinate_pair_together() {
        // `scroll:4,2,down` must split on the *last* comma, not the first.
        assert_eq!(
            parse_one("scroll:4,2,down"),
            Action::Scroll {
                target: Some(Target::Cell { x: 4, y: 2 }),
                direction: ScrollDirection::Down,
            }
        );
    }

    #[test]
    fn drag_takes_two_targets() {
        assert_eq!(
            parse_one("drag:#card>#column"),
            Action::Drag {
                from: Target::Key(Key::from("card")),
                to: Target::Key(Key::from("column")),
            }
        );
    }

    #[test]
    fn wait_accepts_bare_and_suffixed_milliseconds() {
        assert_eq!(
            parse_one("wait:500"),
            Action::Wait(Duration::from_millis(500))
        );
        assert_eq!(
            parse_one("wait:500ms"),
            Action::Wait(Duration::from_millis(500))
        );
        assert_eq!(
            parse_one("wait"),
            Action::Wait(Duration::from_millis(DEFAULT_WAIT_MS))
        );
    }

    #[test]
    fn keys_use_ordinary_keybinding_syntax() {
        match parse_one("key:ctrl+n") {
            Action::Key(event) => {
                assert_eq!(event.code, KeyCode::Char('n'));
                assert!(event.mods.ctrl);
            }
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn a_multi_key_argument_is_rejected() {
        // `key:` is deliberately one event; a sequence would hide timing.
        let err = parse_script("key:tab,enter").expect_err("must reject");
        assert!(err.to_string().contains("exactly one key"), "{err}");
    }

    #[test]
    fn unknown_verbs_name_the_offending_step() {
        let err = parse_script("key:tab; frobnicate:#x").expect_err("must reject");
        let message = err.to_string();
        assert!(message.contains("frobnicate"), "{message}");
        assert!(message.contains("expected one of"), "{message}");
    }

    #[test]
    fn malformed_targets_are_rejected() {
        assert!(parse_script("click:notacoord").is_err());
        assert!(parse_script("click:1,x").is_err());
        assert!(parse_script("drag:#a").is_err());
        assert!(parse_script("scroll:#list,sideways").is_err());
        assert!(parse_script("wait:soon").is_err());
    }
}
