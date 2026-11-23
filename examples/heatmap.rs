use tui_lipan::prelude::*;
use tui_lipan::{Heatmap, HeatmapCellMode, HeatmapLegendWidth};

struct HeatmapExample;

#[derive(Default)]
struct State;

#[derive(Clone, Debug)]
enum Msg {}

impl Component for HeatmapExample {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let heat_gradient = ColorGradient::new(Color::Rgb(60, 179, 113), Color::Rgb(226, 82, 87))
            .with_center(Color::Rgb(239, 196, 92));

        let cool_gradient = ColorGradient::new(Color::Rgb(77, 166, 255), Color::Rgb(255, 100, 100));

        // Weekly activity heatmap.
        let activity_data = vec![
            vec![3.0, 0.0, 5.0, 2.0, 8.0, 1.0, 0.0],
            vec![7.0, 4.0, 2.0, 9.0, 3.0, 6.0, 1.0],
            vec![1.0, 8.0, 6.0, 4.0, 7.0, 2.0, 5.0],
            vec![5.0, 3.0, 9.0, 1.0, 6.0, 8.0, 4.0],
        ];

        let activity_heatmap = Heatmap::new(activity_data)
            .row_labels(["Week 1", "Week 2", "Week 3", "Week 4"])
            .column_labels(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
            .gradient(heat_gradient)
            .range(0.0, 10.0)
            .cell_width(5)
            .legend_spacing(1)
            .legend_width(HeatmapLegendWidth::Full)
            .show_legend(true)
            .show_values(true)
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Auto);

        // Server load heatmap.
        let load_data = vec![
            vec![12.0, 45.0, 78.0, 92.0, 65.0, 30.0],
            vec![25.0, 58.0, 85.0, 70.0, 42.0, 18.0],
            vec![8.0, 35.0, 62.0, 88.0, 55.0, 22.0],
            vec![40.0, 72.0, 95.0, 60.0, 38.0, 15.0],
            vec![18.0, 50.0, 75.0, 82.0, 48.0, 28.0],
        ];

        let load_heatmap = Heatmap::new(load_data)
            .row_labels(["api", "auth", "db", "cache", "worker"])
            .column_labels(["00:00", "04:00", "08:00", "12:00", "16:00", "20:00"])
            .gradient(cool_gradient)
            .range(0.0, 100.0)
            .cell_mode(HeatmapCellMode::GlyphForeground(" ".into()))
            .cell_width(7)
            .gap_x(1)
            .gap_y(1)
            .legend_gap(1)
            .legend_spacing(1)
            .legend_width(HeatmapLegendWidth::Full)
            .show_legend(true)
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Flex(1));

        // Simple minimal heatmap.
        let simple_data = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
        ];

        let simple_heatmap = Heatmap::new(simple_data)
            .gradient(ColorGradient::new(
                Color::Rgb(40, 40, 100),
                Color::Rgb(255, 255, 100),
            ))
            .cell_width(6)
            .border(true)
            .width(Length::Auto);

        let big_data = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
            vec![10.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0],
            vec![1.0; 10],
            vec![10.0; 10],
            vec![5.0; 10],
        ];

        let big_glyphforeground_heatmap = Heatmap::new(big_data)
            .gradient(ColorGradient::new(
                Color::Rgb(100, 40, 40),
                Color::Rgb(255, 100, 100),
            ))
            .cell_mode(HeatmapCellMode::GlyphForeground("".into()))
            .cell_width(2)
            .gap_x(0)
            .gap_y(0)
            .legend_gap(1)
            .legend_spacing(1)
            .legend_width(HeatmapLegendWidth::Full)
            .show_legend(true)
            .border(true)
            .width(Length::Auto);

        rsx! {
            VStack {
                padding: 1,
                spacing: 1,
                Text {
                    content: "Heatmap Widget Examples",
                    style: Style::new().bold().fg(Color::Rgb(126, 190, 255)),
                },
                Text {
                    content: "Press 'q' to quit",
                    style: Style::new().dim(),
                },
                Frame {
                    title: "Weekly Activity",
                    border: true,
                    activity_heatmap,
                },
                HStack {
                    spacing: 1,
                    Frame {
                        title: "Server Load Glyph Mode",
                        border: true,
                        width: Length::Flex(2),
                        load_heatmap,
                    },
                    VStack {
                        Frame {
                            title: "Simple Grid",
                            border: true,
                            width: Length::Flex(1),
                            simple_heatmap,
                        },
                        Frame {
                            title: "Glyph Foreground Grid",
                            border: true,
                            width: Length::Flex(1),
                            big_glyphforeground_heatmap,
                        },
                    },
                },
            }
        }
    }
}

fn main() -> Result<()> {
    App::new().mount(HeatmapExample).run()
}
