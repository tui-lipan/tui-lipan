use tui_lipan::prelude::*;

struct ProgressZonesDemo;

#[derive(Default)]
struct State {
    cpu: f64,
    memory: f64,
    disk: f64,
    network: f64,
    queue: f64,
    custom: f64,
}

#[derive(Clone, Debug)]
enum Msg {
    Cpu(ProgressEvent),
    Memory(ProgressEvent),
    Disk(ProgressEvent),
    Network(ProgressEvent),
    Queue(ProgressEvent),
    Custom(ProgressEvent),
}

impl Component for ProgressZonesDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            cpu: 0.72,
            memory: 0.48,
            disk: 0.91,
            network: 0.37,
            queue: 0.58,
            custom: 0.43,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let cpu = ProgressBar::new(ctx.state.cpu)
            .progress_style(ProgressStyle::Line)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Right)
            .label("CPU")
            .target(0.85)
            .target_symbol('◆')
            .target_style(Style::new().fg(Color::LightBlue).bold())
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Cpu))
            .zones([
                ProgressZone::new(0.60).style(Style::new().fg(Color::LightGreen)),
                ProgressZone::new(0.80).style(Style::new().fg(Color::Yellow)),
                ProgressZone::new(1.00).style(Style::new().fg(Color::LightRed)),
            ]);

        let memory = ProgressBar::new(ctx.state.memory)
            .progress_style(ProgressStyle::Block)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Middle)
            .label("Memory")
            .target(0.70)
            .target_symbol('◆')
            .block_empty_bg_dim(0.72)
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Memory))
            .zones([
                ProgressZone::new(0.20)
                    .style(Style::new().fg(Color::Cyan))
                    .symbol('▓'),
                ProgressZone::new(0.40)
                    .style(Style::new().fg(Color::LightBlue))
                    .symbol('▓'),
                ProgressZone::new(1.00)
                    .style(Style::new().fg(Color::LightMagenta))
                    .symbol('▓'),
            ]);

        let disk = ProgressBar::new(ctx.state.disk)
            .progress_style(ProgressStyle::Rect)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Left)
            .label("Disk")
            .target(0.80)
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Disk))
            .zones([
                ProgressZone::new(0.60).style(Style::new().fg(Color::Green)),
                ProgressZone::new(0.85).style(Style::new().fg(Color::Yellow)),
                ProgressZone::new(1.00).style(Style::new().fg(Color::Red)),
            ]);

        let network = ProgressBar::new(ctx.state.network)
            .progress_style(ProgressStyle::Block)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Above)
            .label("Network")
            .target(0.55)
            .target_symbol('▲')
            .target_style(Style::new().fg(Color::White))
            .block_empty_bg_dim(0.78)
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Network))
            .zones([
                ProgressZone::new(0.40).style(Style::new().fg(Color::LightBlue)),
                ProgressZone::new(0.75).style(Style::new().fg(Color::LightGreen)),
                ProgressZone::new(1.00).style(Style::new().fg(Color::LightYellow)),
            ]);

        let queue = ProgressBar::new(ctx.state.queue)
            .progress_style(ProgressStyle::LineDotted)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Below)
            .label("Queue")
            .target(0.65)
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Queue))
            .zones([
                ProgressZone::new(0.50).style(Style::new().fg(Color::LightCyan)),
                ProgressZone::new(0.80).style(Style::new().fg(Color::Yellow)),
                ProgressZone::new(1.00).style(Style::new().fg(Color::LightRed)),
            ]);

        let custom = ProgressBar::new(ctx.state.custom)
            .progress_style(ProgressStyle::Custom {
                filled: '■',
                empty: '·',
            })
            .show_percentage(true)
            .label("Custom")
            .target(0.50)
            .target_symbol('◆')
            .draggable(true)
            .on_change(ctx.link().callback(Msg::Custom))
            .zones([
                ProgressZone::new(0.45)
                    .style(Style::new().fg(Color::LightMagenta))
                    .symbol('■'),
                ProgressZone::new(1.00)
                    .style(Style::new().fg(Color::LightBlue))
                    .symbol('■'),
            ]);

        Frame::new()
            .title("Progress Zones")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "Drag bars with mouse to test zones, target, and percentage positions",
                        )
                        .style(Style::new().dim()),
                    )
                    .child(cpu)
                    .child(memory)
                    .child(disk)
                    .child(network)
                    .child(queue)
                    .child(custom),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Cpu(event) => ctx.state.cpu = event.progress,
            Msg::Memory(event) => ctx.state.memory = event.progress,
            Msg::Disk(event) => ctx.state.disk = event.progress,
            Msg::Network(event) => ctx.state.network = event.progress,
            Msg::Queue(event) => ctx.state.queue = event.progress,
            Msg::Custom(event) => ctx.state.custom = event.progress,
        }
        Update::full()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Progress Zones")
        .mount(ProgressZonesDemo)
        .run()
}
