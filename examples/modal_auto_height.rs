/// Reproducer for Modal + List with height = Auto and max/min constraints.
///
/// Three modals to compare:
///   1. Frame + long List with max_height - reference, should cap at 10 rows
///   2. Modal + long List with height Auto, no cap - should grow to full list
///   3. Modal + long List with max_height(10) - should be capped same as frame
use tui_lipan::prelude::*;

const ITEMS: &[&str] = &[
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango",
];

#[derive(Clone, Debug)]
enum Msg {
    Open(usize),
    Close,
}

struct State {
    open: Option<usize>,
}

struct Demo;

impl Component for Demo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> Self::State {
        State { open: None }
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        ctx.state.open = match msg {
            Msg::Open(n) => Some(n),
            Msg::Close => None,
        };
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let items: Vec<ListItem> = ITEMS.iter().map(|s| ListItem::new(*s)).collect();
        let on_close = ctx.link().callback(|_| Msg::Close);

        // Reference: Frame with a long list capped via max_height on the element
        let frame_capped = Frame::new()
            .title("Frame  max_height=10")
            .border(true)
            .padding(1)
            .height(Length::Auto)
            .child(List::new().items(items.clone()).height(Length::Auto))
            .max_height(Length::Px(10));

        // Modal 1: height Auto, no cap - should expand to full list height
        let modal_uncapped = Modal::new()
            .title("Modal  height=Auto  no cap")
            .height(Length::Auto)
            .child(List::new().items(items.clone()).height(Length::Auto))
            .on_close(on_close.clone());

        // Modal 2: height Auto, capped with element-level max_height
        let modal_capped = Modal::new()
            .title("Modal  height=Auto  max_height=10")
            .height(Length::Auto)
            .child(List::new().items(items.clone()).height(Length::Auto))
            .on_close(on_close.clone())
            .max_height(Length::Px(10));

        let mut root = VStack::new()
            .gap(1)
            .child(Text::new("Frame reference (always visible):"))
            .child(frame_capped)
            .child(Text::new(
                "--- content below frame (should be right after it) ---",
            ))
            .child(
                HStack::new()
                    .gap(2)
                    .child(
                        Button::filled("1: Modal uncapped")
                            .on_click(ctx.link().callback(|_| Msg::Open(1))),
                    )
                    .child(
                        Button::filled("2: Modal max_height=10")
                            .on_click(ctx.link().callback(|_| Msg::Open(2))),
                    ),
            );

        match ctx.state.open {
            Some(1) => root = root.child(modal_uncapped),
            Some(2) => root = root.child(modal_capped),
            _ => {}
        }

        root.into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Modal Auto Height + max_height Repro")
        .mount(Demo)
        .run()
}
