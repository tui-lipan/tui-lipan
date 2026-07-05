//! Grid widget: column/row tracks, auto-flow, gap, and spanning cells.
//!
//! Run with: `cargo run --example grid_basic`

use tui_lipan::prelude::*;

struct GridBasic;

impl Component for GridBasic {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Grid::new()
            .columns([Length::Px(14), Length::Flex(1), Length::Auto])
            .rows([Length::Auto, Length::Auto, Length::Flex(1)])
            .gap_x(1)
            .gap_y(0)
            .padding(1u16)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .cell(0, 0, Text::new("Name:"))
            .cell(0, 1, Input::new("Ada").key("name"))
            .cell(0, 2, Text::new("ok"))
            .child(Button::new("Auto (1,0)").key("b1"))
            .child(Text::new("flow"))
            .child(Text::new("…"))
            .cell_span(2, 0, 1, 3, {
                Frame::new()
                    .title("Span row")
                    .border(true)
                    .width(Length::Flex(1))
                    .child(Text::new("One cell spanning three columns"))
            })
            .into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - grid_basic")
        .mount(GridBasic)
        .run()
}
