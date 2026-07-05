//! Flow widget demo for chip/badge wrapping.
//!
//! Usage:
//!   cargo run --example flow_badges
//!
//! Drag the splitter handle to resize the preview pane and validate
//! that Flow re-wraps chips across rows as width changes.
//! Press 'q' to quit.

use tui_lipan::prelude::*;

struct FlowBadgesDemo;

impl Component for FlowBadgesDemo {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let chips = [
            ("rust", Color::Indexed(45)),
            ("layout", Color::Indexed(81)),
            ("flow", Color::Indexed(117)),
            ("wrapping", Color::Indexed(153)),
            ("terminal-ui", Color::Indexed(118)),
            ("chip", Color::Indexed(154)),
            ("badge", Color::Indexed(190)),
            ("component", Color::Indexed(226)),
            ("state", Color::Indexed(220)),
            ("update", Color::Indexed(214)),
            ("context", Color::Indexed(208)),
            ("render", Color::Indexed(202)),
            ("builder", Color::Indexed(196)),
            ("view", Color::Indexed(199)),
            ("focus", Color::Indexed(170)),
            ("theme", Color::Indexed(141)),
            ("palette", Color::Indexed(99)),
            ("style", Color::Indexed(63)),
            ("frame", Color::Indexed(69)),
            ("splitter", Color::Indexed(75)),
            ("scroll", Color::Indexed(44)),
            ("grid", Color::Indexed(43)),
            ("hstack", Color::Indexed(42)),
            ("vstack", Color::Indexed(41)),
            ("zstack", Color::Indexed(40)),
            ("center", Color::Indexed(39)),
            ("centerpin", Color::Indexed(38)),
            ("mouse", Color::Indexed(37)),
            ("keyboard", Color::Indexed(36)),
            ("events", Color::Indexed(35)),
            ("callbacks", Color::Indexed(34)),
            ("commands", Color::Indexed(33)),
            ("async", Color::Indexed(32)),
            ("docs", Color::Indexed(31)),
            ("examples", Color::Indexed(30)),
            ("qa", Color::Indexed(29)),
        ];

        let chip_elements = chips
            .into_iter()
            .map(|(label, color)| {
                Text::new(format!(" {label} "))
                    .style(Style::new().bg(color).fg(Color::Black).bold())
                    .into()
            })
            .collect::<Vec<Element>>();

        let instructions = Frame::new()
            .title("Controls")
            .border(true)
            .padding(1)
            .child(Text::new(
                "• Drag splitter handle to resize the Flow pane\n\
                 • Observe chip wrapping across rows\n\
                 • Press q to quit",
            ));

        let preview = Frame::new()
            .title("Flow badge wrap preview")
            .border(true)
            .padding(1)
            .child(
                Flow::new()
                    .gap(1)
                    .align(Align::Start)
                    .children(chip_elements),
            );

        Frame::new()
            .title("Flow Demo")
            .border(true)
            .padding(1)
            .child(
                Splitter::vertical()
                    .weights(vec![0.28, 0.72])
                    .min_size(24)
                    .child(instructions)
                    .child(preview),
            )
            .into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Flow Badges Demo")
        .mount(FlowBadgesDemo)
        .run()
}
