//! TextArea sentinel demo.
//!
//! Type `@` to open a file-picker popup. Select a file with arrow keys and
//! press Enter to insert it as a styled token. The token renders as a colored
//! label and is deleted atomically (one Backspace removes the whole token).
//!
//! Ctrl+Shift+S stashes a snapshot; Ctrl+Shift+R restores it.
//!
//! Run with:
//!   cargo run --example text_area_sentinels

use tui_lipan::prelude::*;
use tui_lipan::{SentinelEvent, TextAreaSentinel, TextAreaSnapshot, insert_sentinel};

const FILES: &[&str] = &[
    "src/main.rs",
    "src/lib.rs",
    "Cargo.toml",
    "README.md",
    "examples/text_area_sentinels.rs",
    ".gitignore",
    "src/utils/mod.rs",
    "src/widgets/text_area/mod.rs",
];

struct State {
    editor: TextEditor,
    sentinels: Vec<TextAreaSentinel>,
    picker_open: bool,
    picker_index: usize,
    stash: Option<TextAreaSnapshot>,
    last_event_line: String,
}

impl Default for State {
    fn default() -> Self {
        let mut editor = TextEditor::new(
            "Type here. Press @ to mention a file.\n\n\
             Ctrl+Shift+S stash  Ctrl+Shift+R restore  Backspace deletes a token whole.",
        );
        let len = editor.text().len();
        editor.set_cursor(len);
        Self {
            editor,
            sentinels: Vec::new(),
            picker_open: false,
            picker_index: 0,
            stash: None,
            last_event_line: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    EditorChanged(TextAreaEvent),
    SentinelsChanged(Vec<TextAreaSentinel>),
    SentinelEvents(Vec<SentinelEvent>),
    PickerOpen,
    PickerMove(i32),
    PickerSelect,
    PickerClose,
    SnapshotStash,
    SnapshotRestore,
}

struct SentinelDemo;

impl Component for SentinelDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let state = &ctx.state;
        let link = ctx.link();

        let picker_open = state.picker_open;
        let interceptor = {
            let link = link.clone();
            KeyHandler::new(move |key: KeyEvent| {
                if picker_open {
                    match key.code {
                        KeyCode::Up => link.send(Msg::PickerMove(-1)),
                        KeyCode::Down => link.send(Msg::PickerMove(1)),
                        KeyCode::Enter => link.send(Msg::PickerSelect),
                        KeyCode::Esc => link.send(Msg::PickerClose),
                        _ => {
                            link.send(Msg::PickerClose);
                            return false;
                        }
                    }
                    return true;
                }
                if key.code == KeyCode::Char('@') && key.mods == KeyMods::default() {
                    link.send(Msg::PickerOpen);
                    return true;
                }
                if key.code == KeyCode::Char('s')
                    && key.mods.ctrl
                    && key.mods.shift
                    && !key.mods.alt
                    && !key.mods.super_key
                {
                    link.send(Msg::SnapshotStash);
                    return true;
                }
                if key.code == KeyCode::Char('r')
                    && key.mods.ctrl
                    && key.mods.shift
                    && !key.mods.alt
                    && !key.mods.super_key
                {
                    link.send(Msg::SnapshotRestore);
                    return true;
                }
                false
            })
        };

        let token_style = Style::default().bg(Color::Cyan).fg(Color::Black).bold();
        let token_focus = Style::default()
            .bg(Color::LightCyan)
            .fg(Color::Black)
            .bold();
        let styled: Vec<TextAreaSentinel> = state
            .sentinels
            .iter()
            .map(|s| s.clone().style(token_style).focus_style(token_focus))
            .collect();

        let tokens_text = if state.sentinels.is_empty() {
            "(none yet)".to_string()
        } else {
            state
                .sentinels
                .iter()
                .map(|s| {
                    let path = s.get_payload::<String>().map(String::as_str).unwrap_or("?");
                    format!("{}:{:?}", path, s.sentinel_id())
                })
                .collect::<Vec<_>>()
                .join("  ")
        };

        let hint = if state.picker_open {
            "↑↓ navigate  Enter insert  Esc cancel"
        } else {
            "@ mention  Ctrl+Shift+S stash  Ctrl+Shift+R restore  Backspace removes token"
        };

        VStack::new()
            .child({
                let e: Element = if state.picker_open {
                    let picker_items: Vec<ListItem> = FILES
                        .iter()
                        .enumerate()
                        .map(|(i, name)| ListItem::new(*name).active(i == state.picker_index))
                        .collect();
                    Frame::new()
                        .title("Mention a file  (↑↓ Enter Esc)")
                        .border(true)
                        .height(Length::Px((FILES.len() as u16).saturating_add(2)))
                        .width(Length::Px(45))
                        .child(
                            List::new()
                                .items(picker_items)
                                .selected(state.picker_index)
                                .active_style(Style::default().bg(Color::Blue).fg(Color::White)),
                        )
                        .into()
                } else {
                    Spacer::new().height(Length::Px(0)).into()
                };
                e
            })
            .child(
                Frame::new()
                    .title("TextArea Sentinel Demo")
                    .border(true)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(state.editor.text().to_owned())
                            .cursor(state.editor.cursor())
                            .anchor(state.editor.anchor())
                            .border(true)
                            .scroll_wheel(true)
                            .scrollbar(true)
                            .height(Length::Flex(1))
                            .sentinels(styled)
                            .on_change(link.callback(Msg::EditorChanged))
                            .on_sentinels_change(link.callback(Msg::SentinelsChanged))
                            .on_sentinel_event(link.callback(Msg::SentinelEvents))
                            .key_interceptor(interceptor),
                    ),
            )
            .child(Text::new(hint).height(Length::Px(1)))
            .child(
                Frame::new()
                    .title("Tokens (path + id)")
                    .border(true)
                    .height(Length::Px(3))
                    .child(Text::new(format!("  {}", tokens_text))),
            )
            .child(Text::new(format!("  {}", state.last_event_line)).height(Length::Px(1)))
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        let state = &mut ctx.state;
        match msg {
            Msg::EditorChanged(ev) => {
                state.editor.set_text(ev.value.to_string());
                state.editor.set_cursor(ev.cursor);
                state.editor.set_anchor(ev.anchor);
                Update::full()
            }
            Msg::SentinelsChanged(new_sentinels) => {
                state.sentinels = new_sentinels;
                Update::full()
            }
            Msg::SentinelEvents(events) => {
                state.last_event_line = events
                    .into_iter()
                    .map(|e| match e {
                        SentinelEvent::Deleted { id, sentinel } => {
                            let p = sentinel
                                .get_payload::<String>()
                                .cloned()
                                .unwrap_or_default();
                            format!("Deleted id={id:?} path={p}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                Update::full()
            }
            Msg::PickerOpen => {
                state.picker_open = true;
                state.picker_index = 0;
                Update::full()
            }
            Msg::PickerMove(delta) => {
                let len = FILES.len();
                if delta < 0 {
                    state.picker_index = state
                        .picker_index
                        .checked_sub((-delta) as usize)
                        .unwrap_or(len - 1);
                } else {
                    state.picker_index = (state.picker_index + delta as usize).min(len - 1);
                }
                Update::full()
            }
            Msg::PickerSelect => {
                let path = FILES[state.picker_index];
                let label = format!("@{path}");
                let sentinel = TextAreaSentinel::new(label).payload(path.to_string());
                let cursor = state.editor.cursor();
                let (new_value, new_cursor) =
                    insert_sentinel(state.editor.text(), cursor, &mut state.sentinels, sentinel);
                state.editor.set_text(new_value);
                state.editor.set_cursor(new_cursor);
                state.picker_open = false;
                state.picker_index = 0;
                Update::full()
            }
            Msg::PickerClose => {
                state.picker_open = false;
                Update::full()
            }
            Msg::SnapshotStash => {
                let ta = TextArea::new(state.editor.text().to_owned())
                    .cursor(state.editor.cursor())
                    .anchor(state.editor.anchor())
                    .sentinels(state.sentinels.clone());
                state.stash = Some(TextAreaSnapshot::capture(&ta));
                Update::full()
            }
            Msg::SnapshotRestore => {
                if let Some(snap) = state.stash.clone() {
                    state.editor.set_text(snap.value.to_string());
                    state.editor.set_cursor(snap.cursor);
                    state.editor.set_anchor(snap.anchor);
                    state.sentinels = snap.sentinels;
                }
                Update::full()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - TextArea Sentinel Demo")
        .mount(SentinelDemo)
        .run()
}
