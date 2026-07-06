use tui_lipan::prelude::*;

struct AutoHeightDemo;

struct State {
    items: Vec<String>,
    show_details: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    Add,
    Remove,
    ToggleDetails,
}

impl Component for AutoHeightDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            items: vec!["first task".to_string()],
            show_details: false,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Add => {
                let next = ctx.state.items.len() + 1;
                ctx.state.items.push(format!("task number {next}"));
            }
            Msg::Remove => {
                ctx.state.items.pop();
            }
            Msg::ToggleDetails => {
                ctx.state.show_details = !ctx.state.show_details;
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Char('a') => KeyUpdate::handled(self.update(Msg::Add, ctx)),
            KeyCode::Char('d') => KeyUpdate::handled(self.update(Msg::Remove, ctx)),
            KeyCode::Char('x') => KeyUpdate::handled(self.update(Msg::ToggleDetails, ctx)),
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut stack = VStack::new()
            .child(Text::new(
                "Auto-height inline demo | a adds | d removes | x toggles details | q quits",
            ))
            .child(Text::new(format!("{} item(s):", ctx.state.items.len())));

        for item in &ctx.state.items {
            stack = stack.child(Text::new(format!("  - {item}")));
        }

        if ctx.state.show_details {
            stack = stack.child(
                Text::new(
                    "Details: the viewport grows and shrinks with the content.\n\
                     No fixed height was configured; the framework measures the\n\
                     view each frame and resizes the inline viewport to match.",
                )
                .overflow(Overflow::Wrap),
            );
        }

        stack.into()
    }
}

fn main() -> Result<()> {
    App::new()
        // The viewport follows the content height each frame. Use
        // `InlineHeight::auto_capped(rows)` to add an upper bound.
        .inline_ephemeral(InlineHeight::auto())
        .mount(AutoHeightDemo)
        .run()
}
