/// Side-by-side TextArea + DocumentView with height = Length::Auto.
///
/// Both widgets receive the same text and should shrink to their visual line
/// count (i.e. wrap-aware).  The bug under investigation: DocumentView uses
/// raw source-line count for Auto height while TextArea uses actual visual
/// lines, so DocumentView scrolls even when there is plenty of vertical space.
use std::sync::Arc;

use tui_lipan::prelude::*;

const INITIAL: &str = "\
This is the first line, intentionally written long so it wraps inside a narrow column.
Short line.
Another longer line that should also wrap when the terminal is not super wide.
Line four.
Line five.";

#[derive(Default)]
struct Demo;

#[derive(Clone)]
struct State {
    text: Arc<str>,
    cursor: usize,
    anchor: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        let text: Arc<str> = Arc::from(INITIAL);
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
        }
    }
}

#[derive(Clone)]
enum Msg {
    Edited(TextAreaEvent),
}

impl Component for Demo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        let Msg::Edited(ev) = msg;
        ctx.state.text = ev.value;
        ctx.state.cursor = ev.cursor;
        ctx.state.anchor = ev.anchor;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Text::new(
                "height = Length::Auto  |  wrap = true  |  \
                 Left: TextArea (sizes to visual lines)  |  \
                 Right: DocumentView (bug: sizes to source lines, may scroll)",
            ))
            .child(
                HStack::new()
                    .child(
                        TextArea::new(ctx.state.text.clone())
                            .height(Length::Auto)
                            .wrap(true)
                            .border(true)
                            .cursor(ctx.state.cursor)
                            .anchor(ctx.state.anchor)
                            .on_change(ctx.link().callback(Msg::Edited)),
                    )
                    .child(
                        DocumentView::new(ctx.state.text.clone())
                            .height(Length::Auto)
                            .wrap(true)
                            .border(true)
                            .scrollbar(false),
                    ),
            )
            .child(Text::new(
                "──── below widgets (should appear just after them) ────",
            ))
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Auto Height Test - TextArea vs DocumentView")
        .mount(Demo)
        .run()
}
