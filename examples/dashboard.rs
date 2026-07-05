use tui_lipan::prelude::*;

struct DashboardDemo;

impl Component for DashboardDemo {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let cpu_data = vec![
            10, 20, 30, 45, 60, 55, 40, 30, 25, 20, 15, 10, 5, 10, 25, 40, 60, 80, 70, 50,
        ];
        let mem_data = vec![
            50, 52, 51, 53, 55, 58, 60, 62, 65, 64, 63, 62, 60, 58, 55, 53, 51, 50, 49, 48,
        ];

        let status_bar = StatusBar::new()
            .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
            .padding((0, 1))
            .gap(2)
            .left(
                Text::new(" NORMAL ").style(Style::new().bg(Color::Green).fg(Color::Black).bold()),
            )
            .left(Text::new(" master ").style(Style::new().fg(Color::Blue)))
            .center(Text::new("Dashboard Demo"))
            .right(Text::new("Ln 10, Col 5"))
            .right(Text::new("UTF-8"));

        let grid = Grid::new()
            .uniform_columns(2)
            .rows([Length::Flex(1), Length::Flex(1)])
            .row_gap(1)
            .column_gap(2)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .child(
                Frame::new().title("CPU Usage").border(true).child(
                    VStack::new()
                        .child(
                            Badge::new("ON")
                                .child(Text::new("Live"))
                                .style(Style::new().bg(Color::Green))
                                .text_style(Style::new().fg(Color::Black).bold())
                                .position(BadgePosition::TopStart)
                                .offset((1, 0)),
                        )
                        .child(
                            Sparkline::new(cpu_data)
                                .min(0)
                                .max(100)
                                .bars("▁▂▃▄▅▆▇█".chars())
                                .style(Style::new().fg(Color::Red)),
                        ),
                ),
            )
            .child(
                Frame::new().title("Memory Usage").border(true).child(
                    VStack::new()
                        .child(
                            Badge::new("OK")
                                .child(Text::new("Stable"))
                                .style(Style::new().bg(Color::Blue))
                                .text_style(Style::new().fg(Color::White).bold())
                                .position(BadgePosition::TopEnd)
                                .offset((1, 0)),
                        )
                        .child(
                            Sparkline::new(mem_data)
                                .min(0)
                                .max(100)
                                .bars("▁▂▃▄▅▆▇█".chars())
                                .style(Style::new().fg(Color::Cyan)),
                        ),
                ),
            )
            .child(
                Frame::new()
                    .title("Network")
                    .border(true)
                    .child(Text::new("Receiving data...")),
            )
            .child(
                Frame::new()
                    .title("Disk")
                    .border(true)
                    .child(Text::new("I/O Idle")),
            );

        VStack::new().child(grid).child(status_bar).into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Dashboard Demo")
        .mount(DashboardDemo)
        .run()
}
