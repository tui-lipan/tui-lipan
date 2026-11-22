use tui_lipan::prelude::*;

struct BothAxesDemo;

#[derive(Default)]
struct State {
    v_offset: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Scroll(ScrollEvent),
}

impl Component for BothAxesDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Scroll(event) => {
                ctx.state.v_offset = event.offset;
            }
        }
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        // Rows have *varying* widths and use `Length::Auto`, so each row sizes
        // to its natural width. Because the list is virtualized (> 8 rows), the
        // off-screen rows contribute their estimated natural width to the
        // horizontal scroll range instead of collapsing to zero.
        let rows = (0..24).map(|row| {
            let label = format!("Row {row:02}");
            let cell_count = 6 + (row % 12);
            let cells: Vec<Element> = (0..cell_count)
                .map(|col| {
                    Text::new(format!("{label} · col {col:02} · wide cell content"))
                        .width(Length::Auto)
                        .into()
                })
                .collect();
            HStack::new()
                .gap(2)
                .children(cells)
                .width(Length::Auto)
                .height(Length::Px(1))
                .key(format!("row-{row}"))
        });

        ui! {
            VStack::new().gap(1).padding(1) => {
                Frame::new()
                    .title("ScrollView · vertical + horizontal")
                    .border(true)
                    .padding(1)
                    .child(
                        ScrollView::new()
                            .axis(ScrollAxis::Both)
                            .scrollbar(true)
                            .h_scrollbar(true)
                            .scroll_wheel(true)
                            // Wheel steps: 3 rows vertically, 10 columns horizontally
                            // (columns are finer-grained, so horizontal wants a bigger step).
                            .scroll_wheel_multiplier(3)
                            .h_scroll_wheel_multiplier(10)
                            // Focusable so arrow / vim keys route here once focused
                            // (Tab to focus, or click the view).
                            .focusable(true)
                            .on_scroll(ctx.link().callback(Msg::Scroll))
                            .width(Length::Flex(1))
                            .height(Length::Flex(1))
                            .children(rows),
                    ),
                Text::new(
                    format!(
                        "Wheel: scroll vertically · Shift+wheel: pan horizontally · \
                         Tab to focus, then ↑/↓ ←/→ (or k/j h/l) to scroll · v={}",
                        ctx.state.v_offset
                    ),
                ),
            }
        }
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new()
        .title("ScrollView both axes")
        .mount(BothAxesDemo)
        .run()
}
