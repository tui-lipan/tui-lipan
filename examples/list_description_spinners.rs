//! Spinners anchored to a row's own text, in both `List` and `SearchPalette`.
//!
//! Run with: `cargo run --example list_description_spinners`
//!
//! Press `p` to cycle the palette's `DescriptionPlacement` and watch the spinner
//! follow the description text. Press `q` to quit.

use tui_lipan::prelude::*;

struct DescriptionSpinnersDemo;

#[derive(Default)]
struct State {
    placement_idx: usize,
    selected: usize,
}

#[derive(Clone, Copy, Debug)]
enum Msg {
    Select(ListEvent),
}

const PLACEMENTS: [DescriptionPlacement; 4] = [
    DescriptionPlacement::Inline,
    DescriptionPlacement::Right,
    DescriptionPlacement::Above,
    DescriptionPlacement::Below,
];

fn placement_name(placement: DescriptionPlacement) -> &'static str {
    match placement {
        DescriptionPlacement::Inline => "Inline",
        DescriptionPlacement::Right => "Right",
        DescriptionPlacement::Above => "Above",
        DescriptionPlacement::Below => "Below",
    }
}

fn task_items() -> Vec<ListItem> {
    vec![
        // Leading spinner, the default side.
        ListItem::new("Deploy")
            .description("building image")
            .description_spinner(Spinner::new()),
        // Trailing spinner, with a slower style.
        ListItem::new("Index")
            .description("scanning 4.2k files")
            .description_spinner(
                Spinner::new()
                    .spinner_style(SpinnerStyle::Arc)
                    .speed(SpinnerSpeed::Slow),
            )
            .description_spinner_position(ListSymbolPosition::Right),
        // Anchored to the label instead of the description.
        ListItem::new("Sync")
            .description("waiting for lock")
            .label_spinner(Spinner::new().spinner_style(SpinnerStyle::Circle)),
        // A finished row keeps its column budget without a spinner.
        ListItem::new("Test").description("passed"),
        // Extra lines carry their own slot.
        ListItem::new("Publish").line(
            ListItemLine::new("  crates.io")
                .description("uploading")
                .description_spinner(Spinner::new().spinner_style(SpinnerStyle::Braille)),
        ),
    ]
}

fn palette_items() -> Vec<SearchItem<&'static str>> {
    vec![
        SearchItem::new("Rebuild workspace", "rebuild").description(
            ItemDescription::new()
                .left("compiling 128 crates")
                .spinner(Spinner::new()),
        ),
        SearchItem::new("Fetch remotes", "fetch").description(
            ItemDescription::new()
                .left("contacting origin")
                .spinner(Spinner::new().spinner_style(SpinnerStyle::Line))
                .spinner_position(ListSymbolPosition::Right),
        ),
        SearchItem::new("Open settings", "settings").description("ready"),
    ]
}

impl Component for DescriptionSpinnersDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('p') | KeyCode::Char('P') => {
                ctx.state.placement_idx = (ctx.state.placement_idx + 1) % PLACEMENTS.len();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(event) => {
                ctx.state.selected = event.index;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let placement = PLACEMENTS[ctx.state.placement_idx];

        ui! {
            VStack::new()
                .gap(1)
                .padding(1)
                .child(
                    Frame::new()
                        .header_left("List rows")
                        .child(
                            List::new()
                                .items(task_items())
                                .selected(ctx.state.selected)
                                .symbol_column(false)
                                .on_select(ctx.link().callback(Msg::Select)),
                        ),
                )
                .child(
                    Frame::new()
                        .header_left(
                            format!("SearchPalette - placement: {}", placement_name(placement)),
                        )
                        .child(
                            SearchPalette::new()
                                .items(palette_items())
                                .description_placement(placement),
                        ),
                )
                .child(
                    Text::new("p: cycle description placement   q: quit")
                        .style(Style::new().fg(Color::DarkGray)),
                )
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Description Spinners")
        .mount(DescriptionSpinnersDemo)
        .run()
}
