//! Keyboard-driven terminal hints backed by the public hint and decoration APIs.
//!
//! Run with:
//!   cargo run --example terminal_hints --features terminal

use std::time::Duration;

use tui_lipan::prelude::*;
use tui_lipan::utils::hints::{HOME_ROW_HINT_KEYS, HintKind, HintMatch, HintScan, assign_labels};

const TERMINAL_KEY: &str = "hint-terminal";
const IPV4_HINT_ID: u16 = 1;
const COPY_FLASH_MS: u64 = 150;

struct TerminalHints;

struct State {
    snapshot: TerminalRenderSnapshot,
    matches: Vec<HintMatch>,
    labels: Vec<String>,
    filter: String,
    active: bool,
    copy_flash: Option<TerminalSelection>,
    status: String,
}

#[derive(Clone)]
enum Msg {
    Key(KeyEvent),
    Enter,
    Exit,
    Append(char),
    Activate { index: usize, open: bool },
    ClearCopyFlash,
    Quit,
}

impl Component for TerminalHints {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let mut screen = TerminalScreen::new(10, 110, 32);
        screen.process_bytes(
            b"\x1b[1;36m$ tui-lipan demo --hints\x1b[0m\r\n\
              URL: https://github.com/tui-lipan/tui-lipan\r\n\
              Path: ./examples/terminal_hints.rs:42\r\n\
              Commit: 9fceb02d4a1e8f0c7b12abcdeffed1234567890a\r\n\
              Address: 192.168.1.42 (custom regex hint)\r\n\
              Type a label, Enter copies it, uppercase opens URLs.\r\n",
        );
        let snapshot = screen.render_snapshot();
        let matches = HintScan::new()
            .custom(IPV4_HINT_ID, ipv4_ranges)
            .scan(&snapshot.text);
        let labels = assign_labels(matches.len(), HOME_ROW_HINT_KEYS);

        Self::State {
            snapshot,
            matches,
            labels,
            filter: String::new(),
            active: true,
            copy_flash: None,
            status: String::from("Type a label: lowercase copies, uppercase opens when supported."),
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        ctx.request_focus(TERMINAL_KEY);
        None
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let Some(msg) = hint_key_message(key, &ctx.state) else {
            return KeyUpdate::unhandled(Update::none());
        };
        ctx.link().send(msg);
        KeyUpdate::handled(Update::full())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Key(key) => {
                let Some(next) = hint_key_message(key, &ctx.state) else {
                    return Update::none();
                };
                return self.update(next, ctx);
            }
            Msg::Enter => {
                ctx.state.active = true;
                ctx.state.filter.clear();
                ctx.state.copy_flash = None;
                ctx.state.status =
                    "Hint mode active • lowercase copies • uppercase opens".to_string();
            }
            Msg::Exit => {
                ctx.state.active = false;
                ctx.state.filter.clear();
                ctx.state.status = "Hint mode exited • h enters again • q quits".to_string();
            }
            Msg::Append(ch) => ctx.state.filter.push(ch),
            Msg::Activate { index, open } => return activate_hint(index, open, ctx),
            Msg::ClearCopyFlash => ctx.state.copy_flash = None,
            Msg::Quit => {
                ctx.quit();
                return Update::none();
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let visible = if ctx.state.active {
            matching_indices(&ctx.state.labels, &ctx.state.filter)
        } else {
            Vec::new()
        };
        let decorations = hint_decorations(&ctx.state.matches, &ctx.state.labels, &visible);
        let snapshot = ctx.state.snapshot.decorated(&decorations);
        let filter = if !ctx.state.active {
            "(inactive)"
        } else if ctx.state.filter.is_empty() {
            "(type a label)"
        } else {
            ctx.state.filter.as_str()
        };
        let footer = Text::from_spans([
            Span::new("label: ").fg(Color::DarkGray),
            Span::new(filter).fg(Color::Yellow).bold(),
            Span::new(format!(
                "  {}/{} visible  |  lowercase copy  |  uppercase open  |  Esc/q exit  |  h enter",
                visible.len(),
                ctx.state.matches.len()
            ))
            .fg(Color::DarkGray),
        ]);
        let terminal: Element = Terminal::new()
            .snapshot(snapshot)
            .show_cursor(!ctx.state.active)
            .selection(ctx.state.copy_flash.clone())
            .selection_style(Style::new().fg(Color::Black).bg(Color::LightCyan))
            .on_key(ctx.link().key_handler(|key| Some(Msg::Key(key))))
            .into();

        VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .header_left("Terminal hints / scan + label + decorate")
                    .border(true)
                    .padding(0)
                    .child(terminal.key(TERMINAL_KEY)),
            )
            .child(Frame::new().border(true).padding((0, 1)).child(footer))
            .child(Text::new(ctx.state.status.clone()).style(Style::new().fg(Color::DarkGray)))
            .into()
    }
}

fn ipv4_ranges(line: &str, out: &mut Vec<std::ops::Range<usize>>) {
    let mut offset = 0usize;
    for token in line.split_whitespace() {
        let start = line[offset..].find(token).unwrap_or(0) + offset;
        offset = start + token.len();
        let value = token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.');
        if value.matches('.').count() == 3
            && value
                .split('.')
                .all(|part| !part.is_empty() && part.parse::<u8>().is_ok())
        {
            let relative = token.find(value).unwrap_or(0);
            out.push(start + relative..start + relative + value.len());
        }
    }
}

fn hint_decorations(
    matches: &[HintMatch],
    labels: &[String],
    visible: &[usize],
) -> Vec<TerminalDecoration> {
    visible
        .iter()
        .rev()
        .filter_map(|&index| {
            let hint = matches.get(index)?;
            let label = labels.get(index)?;
            let highlight = match hint.kind {
                HintKind::Url => Color::Cyan,
                HintKind::Path => Color::Green,
                HintKind::GitSha => Color::Magenta,
                HintKind::Custom(_) => Color::Yellow,
            };
            Some([
                TerminalDecoration::highlight(
                    hint.row,
                    hint.start_col..hint.end_col,
                    Style::new().fg(highlight).bold().underline(),
                ),
                TerminalDecoration::label(
                    hint.row,
                    hint.end_col,
                    Span::new(label.clone()).style(
                        Style::new()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .bold()
                            .contrast_policy(ContrastPolicy::BlackOrWhite),
                    ),
                ),
            ])
        })
        .flatten()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HintInput {
    Ignored,
    Append(char),
    Activate { index: usize, open: bool },
}

fn hint_key_message(key: KeyEvent, state: &State) -> Option<Msg> {
    if key.mods.ctrl || key.mods.alt || key.mods.super_key {
        return None;
    }
    if !state.active {
        return match key.code {
            KeyCode::Char('h') => Some(Msg::Enter),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q' | 'Q') => Some(Msg::Exit),
        KeyCode::Char(ch) => {
            match resolve_hint_input(&state.labels, &state.matches, &state.filter, ch) {
                HintInput::Append(lower) => Some(Msg::Append(lower)),
                HintInput::Activate { index, open } => Some(Msg::Activate { index, open }),
                HintInput::Ignored => None,
            }
        }
        _ => None,
    }
}

fn resolve_hint_input(
    labels: &[String],
    matches: &[HintMatch],
    input: &str,
    ch: char,
) -> HintInput {
    let lower = ch.to_ascii_lowercase();
    if !HOME_ROW_HINT_KEYS.contains(lower) {
        return HintInput::Ignored;
    }
    let mut next = input.to_string();
    next.push(lower);
    let candidates = labels
        .iter()
        .enumerate()
        .filter(|(_, label)| label.starts_with(&next))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if candidates.len() == 1 && labels[candidates[0]] == next {
        let index = candidates[0];
        let open = ch.is_ascii_uppercase()
            && matches
                .get(index)
                .is_some_and(|matched| hint_can_open(matched.kind));
        HintInput::Activate { index, open }
    } else if candidates.is_empty() {
        HintInput::Ignored
    } else {
        HintInput::Append(lower)
    }
}

fn matching_indices(labels: &[String], input: &str) -> Vec<usize> {
    labels
        .iter()
        .enumerate()
        .filter_map(|(index, label)| label.starts_with(input).then_some(index))
        .collect()
}

fn activate_hint(index: usize, open: bool, ctx: &mut Context<TerminalHints>) -> Update {
    let Some((text, kind, selection)) = ctx.state.matches.get(index).map(|hint| {
        (
            hint.text.clone(),
            hint.kind,
            TerminalSelection {
                anchor: tui_lipan::utils::GridPos {
                    row: hint.row,
                    col: hint.start_col,
                },
                cursor: tui_lipan::utils::GridPos {
                    row: hint.row,
                    col: hint.end_col,
                },
            },
        )
    }) else {
        return Update::none();
    };
    ctx.state.active = false;
    ctx.state.filter.clear();
    if open {
        match tui_lipan::utils::open_url(&text) {
            Ok(()) => {
                ctx.state.status = format!("Opened {}: {text}", hint_kind_name(kind));
            }
            Err(error) => {
                ctx.state.status = format!("Open failed ({error}): {text}");
            }
        }
        return Update::full();
    }
    if !copy_hint(&text, ctx) {
        return Update::full();
    }

    ctx.state.copy_flash = Some(selection);
    ctx.state.status = format!("Copied {}: {text}", hint_kind_name(kind));
    if let Some(node_id) = ctx.focused_node_id() {
        ctx.flash_copy_feedback(node_id);
    }
    Update::with_command(Command::after(
        Duration::from_millis(COPY_FLASH_MS),
        |link: CommandLink<Msg>| link.send(Msg::ClearCopyFlash),
    ))
}

fn copy_hint(text: &str, ctx: &mut Context<TerminalHints>) -> bool {
    match ctx.clipboard().copy(text) {
        Ok(()) => true,
        Err(error) => {
            ctx.state.status = format!("Clipboard error ({error}): {text}");
            false
        }
    }
}

fn hint_kind_name(kind: HintKind) -> &'static str {
    match kind {
        HintKind::Url => "URL",
        HintKind::Path => "path",
        HintKind::GitSha => "Git SHA",
        HintKind::Custom(_) => "custom hint",
    }
}

fn hint_can_open(kind: HintKind) -> bool {
    kind.can_open() || matches!(kind, HintKind::Custom(tag) if custom_hint_opens(tag))
}

/// Example-local custom-hint policy, equivalent to hyprmux's per-pattern `open` setting.
fn custom_hint_opens(tag: u16) -> bool {
    const CUSTOM_HINTS: &[(u16, bool)] = &[(IPV4_HINT_ID, false)];
    CUSTOM_HINTS
        .iter()
        .find_map(|(candidate, open)| (*candidate == tag).then_some(*open))
        .unwrap_or(false)
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Terminal Hints")
        .mount(TerminalHints)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    fn hint(kind: HintKind) -> HintMatch {
        HintMatch {
            row: 0,
            start_col: 0,
            end_col: 4,
            text: "hint".to_string(),
            kind,
        }
    }

    #[test]
    fn final_uppercase_character_opens_a_multi_character_label() {
        let labels = assign_labels(11, HOME_ROW_HINT_KEYS);
        let matches = vec![hint(HintKind::Url); labels.len()];

        assert_eq!(
            resolve_hint_input(&labels, &matches, "", 'a'),
            HintInput::Append('a')
        );
        assert_eq!(
            resolve_hint_input(&labels, &matches, "a", 'A'),
            HintInput::Activate {
                index: 0,
                open: true,
            }
        );
    }

    #[test]
    fn uppercase_non_openable_custom_hint_still_copies() {
        let labels = vec!["a".to_string()];
        let matches = vec![hint(HintKind::Custom(IPV4_HINT_ID))];

        assert_eq!(
            resolve_hint_input(&labels, &matches, "", 'A'),
            HintInput::Activate {
                index: 0,
                open: false,
            }
        );
    }

    #[test]
    fn labels_render_at_the_end_of_each_detected_hint() {
        let mut backend = TestBackend::new(TerminalHints);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 114,
            h: 16,
        });
        backend.render();
        let frame = backend.capture_frame();
        let active = frame.to_fixed_grid();
        assert!(active.contains("https://github.com/tui-lipan/tui-lipana"));
        assert!(!active.contains("[a]"));

        assert!(
            frame
                .cells
                .iter()
                .any(|cell| cell.symbol == "a" && cell.bg == Color::Yellow)
        );
    }
}
