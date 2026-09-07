//! Text scripts compiled directly into the shared automation operation model.
//!
//! Actions are separated by `;` or newlines. `#name` targets an app-authored
//! automation ID; `col,row` targets a viewport cell.

use std::time::Duration;

use crate::Result;
#[cfg(test)]
use crate::automation::AutomationStepKind;
use crate::automation::{
    AutomationError, AutomationId, AutomationScrollDirection, AutomationStep, FocusDirection,
    Selector, SemanticRole, WaitCondition,
};
use crate::core::event::{KeyEvent, MouseButton};

const DEFAULT_WAIT_MS: u64 = 250;

/// Compile a text action script into typed automation operations.
pub(crate) fn compile_script(
    raw: &str,
) -> std::result::Result<Vec<AutomationStep>, AutomationError> {
    let mut steps = Vec::new();
    for raw_step in raw.split([';', '\n']) {
        let raw_step = raw_step.trim();
        if !raw_step.is_empty() {
            steps.push(parse_step(raw_step)?);
        }
    }
    Ok(steps)
}

#[cfg(test)]
pub(crate) fn parse_script(raw: &str) -> std::result::Result<Vec<AutomationStep>, AutomationError> {
    compile_script(raw)
}

fn parse_step(step: &str) -> std::result::Result<AutomationStep, AutomationError> {
    let (verb, arg) = match step.split_once(':') {
        Some((verb, arg)) => (verb.trim(), arg.trim()),
        None => (step, ""),
    };

    match verb {
        "type" => Ok(AutomationStep::type_text(
            step.split_once(':').map(|(_, arg)| arg).unwrap_or(""),
        )),
        "key" => Ok(AutomationStep::key(single_key(arg, step)?)),
        "click" => Ok(AutomationStep::click(parse_selector(arg, step)?)),
        "rclick" => Ok(AutomationStep::click_button(
            parse_selector(arg, step)?,
            MouseButton::Right,
        )),
        "mclick" => Ok(AutomationStep::click_button(
            parse_selector(arg, step)?,
            MouseButton::Middle,
        )),
        "hover" => Ok(AutomationStep::hover(parse_selector(arg, step)?)),
        "focus" => match arg {
            "next" => Ok(AutomationStep::focus_step(FocusDirection::Next)),
            "prev" => Ok(AutomationStep::focus_step(FocusDirection::Previous)),
            other if other.starts_with(['#', '@']) || other.starts_with("text~") => {
                Ok(AutomationStep::focus(parse_selector(other, step)?))
            }
            other => Ok(AutomationStep::focus(Selector::id(parse_id_ref(
                other, step,
            )?))),
        },
        "scroll" => parse_scroll(arg, step),
        "drag" => {
            let (from, to) = arg
                .split_once('>')
                .ok_or_else(|| invalid(step, "expected `drag:<from>><to>`"))?;
            Ok(AutomationStep::drag(
                parse_selector(from.trim(), step)?,
                parse_selector(to.trim(), step)?,
            ))
        }
        "wait" => Ok(AutomationStep::advance(parse_duration(arg, step)?)),
        "sleep" => Ok(AutomationStep::sleep(parse_duration(arg, step)?)),
        "resize" => {
            let (width, height) = arg
                .split_once('x')
                .ok_or_else(|| invalid(step, "expected `resize:WIDTHxHEIGHT`"))?;
            Ok(AutomationStep::resize(
                width
                    .parse()
                    .map_err(|_| invalid(step, "width must be an integer"))?,
                height
                    .parse()
                    .map_err(|_| invalid(step, "height must be an integer"))?,
            ))
        }
        "drain" if arg.is_empty() => Ok(AutomationStep::drain_ready()),
        "checkpoint" if !arg.is_empty() => Ok(AutomationStep::checkpoint(arg)),
        "wait-for" => parse_wait_for(arg, step),
        other => Err(invalid(
            step,
            &format!(
                "unknown action `{other}`; expected one of key, type, click, rclick, mclick, \
                 hover, focus, scroll, drag, wait, sleep, resize, drain, checkpoint, wait-for"
            ),
        )),
    }
}

fn parse_scroll(arg: &str, step: &str) -> std::result::Result<AutomationStep, AutomationError> {
    let (selector, direction) = match arg.rsplit_once(',') {
        Some((target, direction)) => (Some(parse_selector(target.trim(), step)?), direction.trim()),
        None => (None, arg),
    };
    let direction = match direction {
        "up" => AutomationScrollDirection::Up,
        "down" => AutomationScrollDirection::Down,
        other => {
            return Err(invalid(
                step,
                &format!("unknown scroll direction `{other}`; expected up or down"),
            ));
        }
    };
    Ok(AutomationStep::scroll(selector, direction))
}

fn parse_selector(arg: &str, step: &str) -> std::result::Result<Selector, AutomationError> {
    if let Some(id) = arg.strip_prefix('#') {
        return Ok(Selector::id(parse_id_ref(id, step)?));
    }
    if let Some(role) = arg.strip_prefix('@') {
        let (role, name) = role
            .split_once('=')
            .map_or((role, None), |(role, name)| (role, Some(name)));
        let selector = Selector::role(parse_role(role, step)?);
        return Ok(match name {
            Some(name) => selector.name(name),
            None => selector,
        });
    }
    if let Some(text) = arg.strip_prefix("text~") {
        return Ok(Selector::text_contains(text));
    }
    let (x, y) = arg
        .split_once(',')
        .ok_or_else(|| invalid(step, "expected `#automation-id` or `col,row`"))?;
    Ok(Selector::point(
        x.trim()
            .parse()
            .map_err(|_| invalid(step, "column must be a number"))?,
        y.trim()
            .parse()
            .map_err(|_| invalid(step, "row must be a number"))?,
    ))
}

/// Parse `wait-for:PREDICATE,SELECTOR,MILLISECONDS`.
///
/// A predicate that needs a value carries it inline as `PREDICATE=VALUE`, keeping the three
/// comma-separated fields fixed so a selector that contains a comma - a point, or a `text~`
/// needle - still parses. A value therefore cannot contain a comma itself.
fn parse_wait_for(arg: &str, step: &str) -> std::result::Result<AutomationStep, AutomationError> {
    const SHAPE: &str = "expected `wait-for:PREDICATE,SELECTOR,MILLISECONDS`";

    let (predicate_and_selector, timeout) =
        arg.rsplit_once(',').ok_or_else(|| invalid(step, SHAPE))?;
    let (predicate, selector) = predicate_and_selector
        .split_once(',')
        .ok_or_else(|| invalid(step, SHAPE))?;
    let selector = parse_selector(selector.trim(), step)?;
    let timeout = parse_duration(timeout.trim(), step)?;
    let predicate = predicate.trim();
    let (predicate, value) = match predicate.split_once('=') {
        Some((predicate, value)) => (predicate.trim(), Some(value.trim())),
        None => (predicate, None),
    };
    let condition = match (predicate, value) {
        ("exists", None) => WaitCondition::exists(selector),
        ("missing", None) => WaitCondition::missing(selector),
        ("in-view", None) => WaitCondition::in_view(selector),
        ("focused", None) => WaitCondition::focused(selector),
        ("enabled", None) => WaitCondition::enabled(selector),
        ("disabled", None) => WaitCondition::disabled(selector),
        ("selected", value) => WaitCondition::selected(selector, parse_wait_bool(value, step)?),
        ("value", Some(value)) => WaitCondition::value_equals(selector, value),
        ("text", Some(text)) => WaitCondition::text_contains(selector, text),
        ("count", Some(count)) => WaitCondition::count(
            selector,
            count
                .parse()
                .map_err(|_| invalid(step, "count must be a whole number"))?,
        ),
        ("value" | "text" | "count", None) => {
            return Err(invalid(
                step,
                &format!("wait predicate `{predicate}` needs a value, as `{predicate}=...`"),
            ));
        }
        (other, _) => {
            return Err(invalid(
                step,
                &format!(
                    "unknown wait predicate `{other}`; expected exists, missing, in-view, \
                     focused, enabled, disabled, selected[=true|false], value=..., text=..., \
                     or count=..."
                ),
            ));
        }
    };
    Ok(AutomationStep::wait_for(condition, timeout))
}

/// A bare `selected` means selected; `selected=false` is how a script waits for the opposite.
fn parse_wait_bool(value: Option<&str>, step: &str) -> std::result::Result<bool, AutomationError> {
    match value {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(other) => Err(invalid(
            step,
            &format!("expected `true` or `false`, not `{other}`"),
        )),
    }
}

fn parse_role(role: &str, step: &str) -> std::result::Result<SemanticRole, AutomationError> {
    let role = match role {
        "application" => SemanticRole::Application,
        "generic" => SemanticRole::Generic,
        "group" => SemanticRole::Group,
        "text" => SemanticRole::Text,
        "heading" => SemanticRole::Heading,
        "button" => SemanticRole::Button,
        "text-box" => SemanticRole::TextBox,
        "text-area" => SemanticRole::TextArea,
        "checkbox" => SemanticRole::Checkbox,
        "radio" => SemanticRole::Radio,
        "radio-group" => SemanticRole::RadioGroup,
        "combo-box" => SemanticRole::ComboBox,
        "list" => SemanticRole::List,
        "list-item" => SemanticRole::ListItem,
        "table" => SemanticRole::Table,
        "row" => SemanticRole::Row,
        "cell" => SemanticRole::Cell,
        "tab-list" => SemanticRole::TabList,
        "tab" => SemanticRole::Tab,
        "tree" => SemanticRole::Tree,
        "tree-item" => SemanticRole::TreeItem,
        "dialog" => SemanticRole::Dialog,
        "alert" => SemanticRole::Alert,
        "progress-bar" => SemanticRole::ProgressBar,
        "slider" => SemanticRole::Slider,
        "scroll-view" => SemanticRole::ScrollView,
        "separator" => SemanticRole::Separator,
        "status" => SemanticRole::Status,
        "link" => SemanticRole::Link,
        "terminal" => SemanticRole::Terminal,
        "image" => SemanticRole::Image,
        "menu" => SemanticRole::Menu,
        "menu-item" => SemanticRole::MenuItem,
        "toolbar" => SemanticRole::Toolbar,
        other => return Err(invalid(step, &format!("unknown semantic role `{other}`"))),
    };
    Ok(role)
}

fn parse_id_ref(raw: &str, step: &str) -> std::result::Result<AutomationId, AutomationError> {
    let name = raw.trim().trim_start_matches('#');
    if name.is_empty() {
        return Err(invalid(step, "expected an automation ID"));
    }
    AutomationId::try_new(name.to_owned()).map_err(|error| invalid(step, &error.to_string()))
}

fn parse_duration(raw: &str, step: &str) -> std::result::Result<Duration, AutomationError> {
    let milliseconds = if raw.is_empty() {
        DEFAULT_WAIT_MS
    } else {
        raw.trim_end_matches("ms")
            .trim()
            .parse()
            .map_err(|_| invalid(step, "expected milliseconds"))?
    };
    Ok(Duration::from_millis(milliseconds))
}

fn single_key(arg: &str, step: &str) -> std::result::Result<KeyEvent, AutomationError> {
    let mut events =
        super::keys::parse_key_script(arg).map_err(|error| invalid(step, &error.to_string()))?;
    if events.len() != 1 {
        return Err(invalid(
            step,
            "expected exactly one key; use several `key:` steps for a sequence",
        ));
    }
    Ok(events.remove(0))
}

fn invalid(step: &str, reason: &str) -> AutomationError {
    AutomationError::Script(format!("invalid action `{step}`: {reason}"))
}

pub(crate) fn execute_step(
    host: &mut impl crate::session::OperationHost,
    step: &AutomationStep,
) -> Result<()> {
    crate::session::execute_operation(host, &step.kind)
        .map(|_| ())
        .map_err(|error| std::io::Error::other(error.to_string()).into())
}

#[cfg(test)]
pub(crate) fn execute(
    host: &mut impl crate::session::OperationHost,
    step: &AutomationStep,
) -> Result<()> {
    execute_step(host, step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::KeyCode;

    fn parse_one(step: &str) -> AutomationStep {
        let mut steps = parse_script(step).expect("parses");
        assert_eq!(steps.len(), 1);
        steps.remove(0)
    }

    #[test]
    fn steps_split_on_semicolons_and_newlines() {
        let steps = parse_script("key:tab; wait:10\nfocus:next").expect("parses");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2], AutomationStep::focus_step(FocusDirection::Next));
    }

    #[test]
    fn persistent_operations_and_semantic_selectors_compile() {
        assert_eq!(parse_one("resize:120x40"), AutomationStep::resize(120, 40));
        assert_eq!(parse_one("drain"), AutomationStep::drain_ready());
        assert_eq!(
            parse_one("checkpoint:ready"),
            AutomationStep::checkpoint("ready")
        );
        assert_eq!(
            parse_one("click:@button=Save"),
            AutomationStep::click(Selector::role(SemanticRole::Button).name("Save"))
        );
        assert_eq!(
            parse_one("wait-for:exists,text~Connected,500"),
            AutomationStep::wait_for(
                WaitCondition::exists(Selector::text_contains("Connected")),
                Duration::from_millis(500),
            )
        );
    }

    #[test]
    fn every_wait_predicate_has_a_script_spelling() {
        assert_eq!(
            parse_one("wait-for:selected,#tab,500"),
            AutomationStep::wait_for(
                WaitCondition::selected(Selector::id("tab"), true),
                Duration::from_millis(500),
            )
        );
        assert_eq!(
            parse_one("wait-for:selected=false,#tab,500"),
            AutomationStep::wait_for(
                WaitCondition::selected(Selector::id("tab"), false),
                Duration::from_millis(500),
            )
        );
        assert_eq!(
            parse_one("wait-for:value=ready,#draft,500"),
            AutomationStep::wait_for(
                WaitCondition::value_equals(Selector::id("draft"), "ready"),
                Duration::from_millis(500),
            )
        );
        assert_eq!(
            parse_one("wait-for:text=Connected,@list,500"),
            AutomationStep::wait_for(
                WaitCondition::text_contains(Selector::role(SemanticRole::List), "Connected"),
                Duration::from_millis(500),
            )
        );
        assert_eq!(
            parse_one("wait-for:count=3,@list-item,500"),
            AutomationStep::wait_for(
                WaitCondition::count(Selector::role(SemanticRole::ListItem), 3),
                Duration::from_millis(500),
            )
        );
    }

    #[test]
    fn a_valued_predicate_keeps_a_comma_bearing_selector_parsable() {
        // The value rides inside the first field, so the selector is still everything between
        // the first and last comma.
        assert_eq!(
            parse_one("wait-for:value=ready,45,4,500"),
            AutomationStep::wait_for(
                WaitCondition::value_equals(Selector::point(45, 4), "ready"),
                Duration::from_millis(500),
            )
        );
    }

    #[test]
    fn a_valued_predicate_without_its_value_is_rejected() {
        let error = parse_script("wait-for:count,@list-item,500").unwrap_err();
        assert!(error.to_string().contains("needs a value"), "{error}");
    }

    #[test]
    fn invalid_transport_ids_return_script_errors() {
        let error = parse_script("click:#two words").unwrap_err();
        assert!(matches!(error, AutomationError::Script(_)));
    }

    #[test]
    fn blank_steps_are_ignored() {
        assert!(parse_script("  ;\n\n ; ").expect("parses").is_empty());
    }

    #[test]
    fn clicks_target_ids_or_cells() {
        assert_eq!(
            parse_one("click:#submit"),
            AutomationStep::click(Selector::id("submit"))
        );
        assert_eq!(
            parse_one("click:12,7"),
            AutomationStep::click(Selector::point(12, 7))
        );
    }

    #[test]
    fn button_variants_are_distinct() {
        for (step, expected) in [
            ("click:#a", MouseButton::Left),
            ("rclick:#a", MouseButton::Right),
            ("mclick:#a", MouseButton::Middle),
        ] {
            match parse_one(step).kind {
                AutomationStepKind::Click { button, .. } => {
                    assert_eq!(button, expected, "{step}");
                }
                other => panic!("expected a click, got {other:?}"),
            }
        }
    }

    #[test]
    fn type_preserves_spaces_and_punctuation() {
        assert_eq!(
            parse_one("type:hello, world!"),
            AutomationStep::type_text("hello, world!")
        );
    }

    #[test]
    fn focus_accepts_ids_and_steps() {
        assert_eq!(
            parse_one("focus:next"),
            AutomationStep::focus_step(FocusDirection::Next)
        );
        assert_eq!(
            parse_one("focus:prev"),
            AutomationStep::focus_step(FocusDirection::Previous)
        );
        assert_eq!(
            parse_one("focus:#email"),
            AutomationStep::focus(Selector::id("email"))
        );
        assert_eq!(parse_one("focus:#email"), parse_one("focus:email"));
    }

    #[test]
    fn scroll_parses_with_and_without_a_target() {
        assert_eq!(
            parse_one("scroll:#list,down"),
            AutomationStep::scroll(Some(Selector::id("list")), AutomationScrollDirection::Down,)
        );
        assert_eq!(
            parse_one("scroll:up"),
            AutomationStep::scroll(None, AutomationScrollDirection::Up)
        );
    }

    #[test]
    fn scroll_over_a_cell_keeps_the_coordinate_pair_together() {
        assert_eq!(
            parse_one("scroll:4,2,down"),
            AutomationStep::scroll(Some(Selector::point(4, 2)), AutomationScrollDirection::Down,)
        );
    }

    #[test]
    fn drag_takes_two_targets() {
        assert_eq!(
            parse_one("drag:#card>#column"),
            AutomationStep::drag(Selector::id("card"), Selector::id("column"))
        );
    }

    #[test]
    fn wait_accepts_bare_and_suffixed_milliseconds() {
        assert_eq!(
            parse_one("wait:500"),
            AutomationStep::advance(Duration::from_millis(500))
        );
        assert_eq!(
            parse_one("wait:500ms"),
            AutomationStep::advance(Duration::from_millis(500))
        );
        assert_eq!(
            parse_one("wait"),
            AutomationStep::advance(Duration::from_millis(DEFAULT_WAIT_MS))
        );
    }

    #[test]
    fn keys_use_ordinary_keybinding_syntax() {
        match parse_one("key:ctrl+n").kind {
            AutomationStepKind::Key(event) => {
                assert_eq!(event.code, KeyCode::Char('n'));
                assert!(event.mods.ctrl);
            }
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn malformed_steps_are_rejected() {
        for script in [
            "key:tab,enter",
            "key:tab; frobnicate:#x",
            "click:notacoord",
            "click:1,x",
            "drag:#a",
            "scroll:#list,sideways",
            "wait:soon",
        ] {
            assert!(parse_script(script).is_err(), "{script}");
        }
    }
}
