use tui_lipan::prelude::*;
use tui_lipan::{HexArea, HexAreaChangeEvent, HexAreaCursorEvent, HexAreaEditEvent};

struct HexAreaDemo;

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    cursor: usize,
    anchor: Option<usize>,
    scroll_offset: usize,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    Cursor(HexAreaCursorEvent),
    Changed(HexAreaChangeEvent),
    Edited(HexAreaEditEvent),
    Scrolled(ScrollEvent),
}

impl Component for HexAreaDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let bytes = (0u8..=255).collect::<Vec<_>>();
        State {
            bytes,
            cursor: 0,
            anchor: None,
            scroll_offset: 0,
            status: "Arrows move, hex keys edit, Insert/Delete modify bytes".to_string(),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let selected_count = ctx
            .state
            .anchor
            .map(|anchor| {
                let start = anchor.min(ctx.state.cursor);
                let end = anchor.max(ctx.state.cursor);
                end.saturating_sub(start).saturating_add(1)
            })
            .unwrap_or(0);

        Frame::new()
            .title("HexArea Binary Inspector")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        HexArea::new(ctx.state.bytes.clone())
                            .cursor(ctx.state.cursor)
                            .anchor(ctx.state.anchor)
                            .scroll_offset(Some(ctx.state.scroll_offset))
                            .bytes_per_row(16)
                            .read_only(false)
                            .show_ascii(true)
                            .show_offsets(true)
                            .uppercase_hex(true)
                            .height(Length::Flex(1))
                            .on_cursor_change(ctx.link().callback(Msg::Cursor))
                            .on_change(ctx.link().callback(Msg::Changed))
                            .on_edit(ctx.link().callback(Msg::Edited))
                            .on_scroll(ctx.link().callback(Msg::Scrolled)),
                    )
                    .child(Text::new(format!(
                        "Cursor: {}  Anchor: {:?}  Selected: {}  Bytes: {}",
                        ctx.state.cursor,
                        ctx.state.anchor,
                        selected_count,
                        ctx.state.bytes.len()
                    )))
                    .child(Text::new(format!("Status: {}", ctx.state.status))),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Cursor(event) => {
                ctx.state.cursor = event.cursor;
                ctx.state.anchor = event.anchor;
                Update::full()
            }
            Msg::Changed(event) => {
                ctx.state.bytes = event.bytes.to_vec();
                ctx.state.cursor = event.cursor;
                ctx.state.anchor = event.anchor;
                Update::full()
            }
            Msg::Edited(event) => {
                ctx.state.status = format!(
                    "Edit {:?} at {}: {:?} -> {:?}",
                    event.kind, event.index, event.before, event.after
                );
                Update::full()
            }
            Msg::Scrolled(event) => {
                ctx.state.scroll_offset = event.offset;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if matches!(key.code, KeyCode::Char('q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - HexArea")
        .mount(HexAreaDemo)
        .run()
}
