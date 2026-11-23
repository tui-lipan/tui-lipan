use tui_lipan::prelude::*;

struct ChartShowcase;

impl Component for ChartShowcase {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let cpu = [38.0, 42.0, 57.0, 63.0, 52.0, 75.0, 68.0, 71.0, 64.0, 59.0];
        let mem = [55.0, 56.0, 57.0, 58.0, 60.0, 61.0, 63.0, 62.0, 61.0, 60.0];
        let io = [14.0, 18.0, 22.0, 17.0, 25.0, 31.0, 28.0, 24.0, 20.0, 18.0];

        let chart = Chart::new()
            .series([
                ChartSeries::new("CPU", cpu)
                    .style(Style::new().fg(Color::LightRed))
                    .point_char('◆'),
                ChartSeries::new("MEM", mem)
                    .style(Style::new().fg(Color::LightCyan))
                    .line_char('━'),
                ChartSeries::new("IO", io)
                    .mode(ChartSeriesMode::Bars)
                    .style(Style::new().fg(Color::LightGreen))
                    .bar_char('▇'),
            ])
            .thresholds([
                ChartThreshold::new(65.0)
                    .label("warn")
                    .style(Style::new().fg(Color::Yellow)),
                ChartThreshold::new(80.0)
                    .label("critical")
                    .style(Style::new().fg(Color::LightRed)),
            ])
            .x_axis(ChartAxis::new().ticks(6).label("samples"))
            .y_axis(ChartAxis::new().ticks(5).range(0.0, 100.0).label("usage %"))
            .show_grid(true)
            .show_legend(true)
            .legend_separator("   ")
            .border(true)
            .padding((0, 1))
            .height(Length::Flex(1));

        Frame::new()
            .title("Chart Showcase")
            .border(true)
            .padding(1)
            .child(chart)
            .into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Chart Showcase")
        .mount(ChartShowcase)
        .run()
}
