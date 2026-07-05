//! ContextProvider example.
//!
//! Run with: cargo run --example context_provider

use tui_lipan::prelude::*;

struct ContextProviderDemo;

#[derive(Clone)]
struct DemoState {
    count: u32,
    compact: bool,
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            count: 3,
            compact: false,
        }
    }
}

enum Msg {
    Increment,
    ToggleCompact,
}

impl Component for ContextProviderDemo {
    type Message = Msg;
    type Properties = ();
    type State = DemoState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        DemoState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Increment => {
                ctx.state.count = ctx.state.count.saturating_add(1);
                Update::full()
            }
            Msg::ToggleCompact => {
                ctx.state.compact = !ctx.state.compact;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let root = VStack::new()
            .gap(1)
            .child(
                HStack::new()
                    .gap(1)
                    .child(
                        Button::new("Increment count")
                            .on_click(ctx.link().callback(|_| Msg::Increment)),
                    )
                    .child(
                        Button::new("Toggle compact")
                            .on_click(ctx.link().callback(|_| Msg::ToggleCompact)),
                    ),
            )
            .child(
                Frame::new()
                    .title("Inherited Context")
                    .border(true)
                    .padding(1)
                    .child(child::<ContextReadout, _>(|| ContextReadout, "root")),
            )
            .child(
                Frame::new()
                    .title("Shadowed Count")
                    .border(true)
                    .padding(1)
                    .child(
                        ContextProvider::new(99u32)
                            .child(child::<ContextReadout, _>(|| ContextReadout, "nested")),
                    ),
            );

        ContextProvider::new("workspace".to_string())
            .child(
                ContextProvider::new(ctx.state.compact)
                    .child(ContextProvider::new(ctx.state.count).child(root)),
            )
            .into()
    }
}

struct ContextReadout;

impl Component for ContextReadout {
    type Message = ();
    type Properties = &'static str;
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let scope = ctx.props;
        let title = ctx
            .use_context::<String>()
            .unwrap_or_else(|| "unset".to_string());
        let count = ctx.use_context::<u32>().unwrap_or_default();
        let compact = ctx.use_context::<bool>().unwrap_or(false);

        Text::new(format!(
            "scope={scope} title={title} count={count} compact={compact}"
        ))
        .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("ContextProvider Example")
        .mount(ContextProviderDemo)
        .run()
}
