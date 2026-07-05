use tui_lipan::prelude::*;

struct MultiSelectDemo;

#[derive(Default)]
struct State {
    active_index: usize,
    selected_indices: Vec<usize>,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    HighlightChanged(usize),
    Toggled(MultiSelectToggleEvent),
    Changed(MultiSelectChangeEvent),
    Commit(MultiSelectCommitEvent),
}

const ITEMS: &[&str] = &[
    "auth", "api", "db", "cache", "search", "worker", "metrics", "alerts", "docs", "release",
];

impl Component for MultiSelectDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            status: "Use Space to toggle, Enter to commit".to_string(),
            ..State::default()
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let selected_labels = ctx
            .state
            .selected_indices
            .iter()
            .filter_map(|index| ITEMS.get(*index))
            .copied()
            .collect::<Vec<_>>()
            .join(", ");

        Frame::new()
            .title("MultiSelect Example")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        MultiSelect::new()
                            .items(ITEMS.iter().copied())
                            .active_index(ctx.state.active_index)
                            .selected_indices(ctx.state.selected_indices.clone())
                            .max_selected(4)
                            .height(Length::Px(10))
                            .on_active_index_change(ctx.link().callback(Msg::HighlightChanged))
                            .on_toggle(ctx.link().callback(Msg::Toggled))
                            .on_change(ctx.link().callback(Msg::Changed))
                            .on_commit(ctx.link().callback(Msg::Commit)),
                    )
                    .child(Text::new(format!("Selected: [{}]", selected_labels)))
                    .child(Text::new(format!("Status: {}", ctx.state.status))),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::HighlightChanged(index) => {
                ctx.state.active_index = index;
                Update::full()
            }
            Msg::Toggled(event) => {
                ctx.state.status = format!(
                    "Toggled index {} -> {}",
                    event.index,
                    if event.selected {
                        "selected"
                    } else {
                        "unselected"
                    }
                );
                Update::full()
            }
            Msg::Changed(event) => {
                ctx.state.selected_indices = event.selected_indices;
                Update::full()
            }
            Msg::Commit(event) => {
                ctx.state.status = format!("Committed {} items", event.selected_indices.len());
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
        .title("tui-lipan - MultiSelect")
        .mount(MultiSelectDemo)
        .run()
}
