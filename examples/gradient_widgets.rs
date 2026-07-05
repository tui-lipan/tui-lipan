use std::sync::Arc;

use tui_lipan::prelude::*;

struct GradientWidgets;

#[derive(Default)]
struct State {
    progress: f64,
    throughput: f64,
    slider_value: f64,
    table_selected: usize,
    show_palette: bool,
    last_action: Option<Arc<str>>,
}

#[derive(Clone, Debug)]
enum Msg {
    ProgressChanged(ProgressEvent),
    ThroughputChanged(ProgressEvent),
    SliderChanged(f64),
    TableSelected(TableEvent),
    TogglePalette(bool),
    PaletteActivated(SearchEvent<Arc<str>>),
}

impl Component for GradientWidgets {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            progress: 0.62,
            throughput: 0.41,
            slider_value: 58.0,
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ProgressChanged(event) => {
                ctx.state.progress = event.progress;
                Update::full()
            }
            Msg::ThroughputChanged(event) => {
                ctx.state.throughput = event.progress;
                Update::full()
            }
            Msg::SliderChanged(value) => {
                ctx.state.slider_value = value;
                Update::full()
            }
            Msg::TableSelected(event) => {
                ctx.state.table_selected = event.index;
                Update::full()
            }
            Msg::TogglePalette(show) => {
                ctx.state.show_palette = show;
                Update::full()
            }
            Msg::PaletteActivated(event) => {
                ctx.state.last_action = Some(event.item.value.clone());
                ctx.state.show_palette = false;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('/') => {
                ctx.state.show_palette = true;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Esc => {
                if ctx.state.show_palette {
                    ctx.state.show_palette = false;
                    KeyUpdate::handled(Update::full())
                } else {
                    ctx.quit();
                    KeyUpdate::handled(Update::full())
                }
            }
            KeyCode::Char('q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let utilization_gradient =
            ColorGradient::new(Color::Rgb(60, 179, 113), Color::Rgb(226, 82, 87))
                .with_center(Color::Rgb(239, 196, 92));
        let throughput_gradient =
            ColorGradient::new(Color::Rgb(77, 166, 255), Color::Rgb(97, 224, 169));
        let score_gradient =
            ColorGradient::new(Color::Rgb(120, 120, 235), Color::Rgb(115, 221, 146))
                .with_center(Color::Rgb(244, 197, 93));

        let selected_action = ctx
            .state
            .last_action
            .as_deref()
            .unwrap_or("(none)")
            .to_string();

        let small = matches!(ctx.breakpoint(110, 170), Breakpoint::Small);

        let top_left = VStack::new()
            .gap(1)
            .width(Length::Flex(1))
            .child(self.progress_and_slider_panel(ctx, utilization_gradient, throughput_gradient))
            .child(self.table_heatmap_panel(ctx));

        let top_right = VStack::new()
            .gap(1)
            .width(Length::Flex(1))
            .child(self.tree_indent_panel())
            .child(self.spinner_panel());

        let body: Element = if small {
            VStack::new().gap(1).child(top_left).child(top_right).into()
        } else {
            HStack::new().gap(1).child(top_left).child(top_right).into()
        };

        let content = VStack::new()
            .gap(1)
            .padding(1)
            .child(
                Text::new("Gradient Widgets Showcase")
                    .style(Style::new().bold().fg(Color::Rgb(126, 190, 255))),
            )
            .child(
                Text::new(
                    "Drag bars/sliders, move table selection, and press '/' to open gradient-scored search.",
                )
                .style(Style::new().dim()),
            )
            .child(body)
            .child(
                StatusBar::new()
                    .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
                    .left(Text::new(format!("Last search action: {}", selected_action)))
                    .right(Text::new("/: search | q: quit")),
            );

        rsx! {
            ZStack {
                content,
                if ctx.state.show_palette {
                    {
                        Modal::new()
                            .title("Action Search")
                            .child(
                                SearchPalette::<Arc<str>>::new()
                                    .items(search_items())
                                    .show_scores(true)
                                    .score_gradient(score_gradient)
                                    .score_range(0, 100)
                                    .on_activate(ctx.link().callback(Msg::PaletteActivated)),
                            )
                            .padding(0)
                            .on_close(ctx.link().callback(|_| Msg::TogglePalette(false)))
                            .key("gradient-search")
                    },
                },
            }
        }
    }
}

impl GradientWidgets {
    fn progress_and_slider_panel(
        &self,
        ctx: &Context<Self>,
        utilization_gradient: ColorGradient,
        throughput_gradient: ColorGradient,
    ) -> Element {
        Frame::new()
            .title("Progress + Slider Gradients")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("ProgressBar.filled_gradient").style(Style::new().bold()))
                    .child(
                        ProgressBar::new(ctx.state.progress)
                            .progress_style(ProgressStyle::Block)
                            .filled_style(Style::new().bold())
                            .filled_gradient(utilization_gradient)
                            .empty_style(Style::new().fg(Color::indexed(238)))
                            .show_percentage(true)
                            .label("CPU")
                            .draggable(true)
                            .on_change(ctx.link().callback(Msg::ProgressChanged)),
                    )
                    .child(
                        ProgressBar::new(ctx.state.throughput)
                            .progress_style(ProgressStyle::Line)
                            .filled_style(Style::new().bold())
                            .filled_gradient(throughput_gradient)
                            .empty_style(Style::new().fg(Color::indexed(238)))
                            .show_percentage(true)
                            .label("Throughput")
                            .draggable(true)
                            .on_change(ctx.link().callback(Msg::ThroughputChanged)),
                    )
                    .child(Text::new("Slider gradients").style(Style::new().bold()))
                    .child(
                        Slider::new(ctx.state.slider_value)
                            .min(0.0)
                            .max(100.0)
                            .step(1.0)
                            .label("Target")
                            .show_value(true)
                            .filled_track_style(Style::new().bold())
                            .filled_track_gradient(utilization_gradient)
                            .thumb_style(Style::new().bold())
                            .thumb_gradient(utilization_gradient)
                            .focus_thumb_style(Style::new().bold().underline())
                            .on_change(ctx.link().callback(Msg::SliderChanged)),
                    ),
            )
            .into()
    }

    fn table_heatmap_panel(&self, ctx: &Context<Self>) -> Element {
        let load_gradient = ColorGradient::new(Color::Rgb(71, 186, 131), Color::Rgb(224, 86, 93))
            .with_center(Color::Rgb(236, 198, 92));
        let latency_gradient =
            ColorGradient::new(Color::Rgb(93, 156, 255), Color::Rgb(244, 143, 82));
        let error_gradient = ColorGradient::new(Color::Rgb(84, 203, 136), Color::Rgb(233, 90, 98));

        let rows = [
            ("api-gateway", 81_u64, 124_u64, 42_u64),
            ("auth", 53_u64, 71_u64, 11_u64),
            ("billing", 67_u64, 156_u64, 65_u64),
            ("search", 44_u64, 98_u64, 24_u64),
            ("notifications", 31_u64, 63_u64, 8_u64),
        ]
        .into_iter()
        .map(|(service, cpu, p95_ms, error_bps)| {
            TableRow::new(vec![
                TableCell::new(service),
                TableCell::new(format!("{:>3}%", cpu))
                    .style(Style::new().fg(Color::Black).bold())
                    .heat_bg(cpu, load_gradient, GradientRange::new(0, 100)),
                TableCell::new(format!("{:>3}ms", p95_ms)).heat_fg(
                    p95_ms,
                    latency_gradient,
                    GradientRange::new(40, 220),
                ),
                TableCell::new(format!("{:>4.2}%", error_bps as f64 / 100.0)).heat_fg(
                    error_bps,
                    error_gradient,
                    GradientRange::new(0, 80),
                ),
            ])
        })
        .collect::<Vec<_>>();

        Frame::new()
            .title("TableCell Heatmap Helpers")
            .border(true)
            .padding(1)
            .child(
                Table::new()
                    .header(
                        TableRow::new(["Service", "CPU", "P95", "Error"]).style(
                            Style::new()
                                .bold()
                                .bg(Color::indexed(238))
                                .fg(Color::indexed(252)),
                        ),
                    )
                    .rows(rows)
                    .widths([
                        ColumnWidth::Fill(2),
                        ColumnWidth::Fixed(6),
                        ColumnWidth::Fixed(7),
                        ColumnWidth::Fixed(8),
                    ])
                    .selected(ctx.state.table_selected)
                    .selection_symbol(Some("> "))
                    .selection_style(Style::new().bg(Color::indexed(24)).fg(Color::White))
                    .on_select(ctx.link().callback(Msg::TableSelected))
                    .height(Length::Px(8)),
            )
            .into()
    }

    fn tree_indent_panel(&self) -> Element {
        let tree = Tree::new(sample_tree())
            .indent_style(IndentStyle::Long)
            .indent_guide_style(Style::new().fg(Color::indexed(240)))
            .indent_gradient(
                ColorGradient::new(Color::Rgb(86, 92, 108), Color::Rgb(255, 170, 84))
                    .with_center(Color::Rgb(104, 213, 255)),
            )
            .show_icons(true)
            .expanded_icon("v")
            .collapsed_icon(">")
            .selection_style(Style::new().reverse())
            .scrollbar(true)
            .height(Length::Auto);

        Frame::new()
            .title("Tree Indent Gradient")
            .border(true)
            .padding(1)
            .child(tree)
            .into()
    }

    fn spinner_panel(&self) -> Element {
        Frame::new()
            .title("Spinner Gradient Tinting")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "Dots is flat color; OpenCode/Lightsaber pulse through tint gradients",
                        )
                        .style(Style::new().dim()),
                    )
                    .child(
                        Spinner::new()
                            .spinner_style(SpinnerStyle::Dots)
                            .label("Dots (flat)")
                            .style(Style::new().fg(Color::Rgb(0, 194, 255)))
                            .speed(SpinnerSpeed::Fast)
                            .width(Length::Auto),
                    )
                    .child(
                        Spinner::new()
                            .spinner_style(SpinnerStyle::OpenCode)
                            .label("OpenCode")
                            .style(Style::new().fg(Color::Rgb(88, 200, 255)))
                            .speed(SpinnerSpeed::Fast)
                            .width(Length::Auto),
                    )
                    .child(
                        Spinner::new()
                            .spinner_style(SpinnerStyle::Lightsaber)
                            .label("Lightsaber (cyan)")
                            .style(Style::new().fg(Color::Rgb(0, 194, 255)))
                            .speed(SpinnerSpeed::Fast)
                            .width(Length::Auto),
                    )
                    .child(
                        Spinner::new()
                            .spinner_style(SpinnerStyle::Lightsaber)
                            .label("Lightsaber (amber)")
                            .style(Style::new().fg(Color::Rgb(255, 189, 56)))
                            .speed(SpinnerSpeed::Fast)
                            .width(Length::Auto),
                    ),
            )
            .into()
    }
}

fn sample_tree() -> TreeNode {
    TreeNode::new("cluster")
        .expanded(true)
        .child(
            TreeNode::new("region/us-east")
                .expanded(true)
                .child(
                    TreeNode::new("zone-a")
                        .expanded(true)
                        .child(TreeNode::new("api-gateway"))
                        .child(TreeNode::new("auth")),
                )
                .child(
                    TreeNode::new("zone-b")
                        .expanded(true)
                        .child(TreeNode::new("billing"))
                        .child(TreeNode::new("search")),
                ),
        )
        .child(
            TreeNode::new("region/eu-central")
                .expanded(true)
                .child(
                    TreeNode::new("zone-a")
                        .expanded(true)
                        .child(
                            TreeNode::new("worker-1")
                                .expanded(true)
                                .child(TreeNode::new("queue/critical"))
                                .child(TreeNode::new("queue/default")),
                        )
                        .child(TreeNode::new("worker-2")),
                )
                .child(
                    TreeNode::new("zone-b")
                        .expanded(true)
                        .child(TreeNode::new("cache")),
                ),
        )
}

fn search_items() -> Vec<SearchItem<Arc<str>>> {
    vec![
        SearchItem::new("Restart API Gateway", Arc::from("Restart API Gateway"))
            .description("Deploy latest edge routing config"),
        SearchItem::new("Scale Billing Workers", Arc::from("Scale Billing Workers"))
            .description("Increase replicas for invoice processing"),
        SearchItem::new("Flush Search Cache", Arc::from("Flush Search Cache"))
            .description("Reset stale query result shards"),
        SearchItem::new("Run Data Backfill", Arc::from("Run Data Backfill"))
            .description("Replay events into analytics warehouse"),
        SearchItem::new(
            "Toggle Maintenance Mode",
            Arc::from("Toggle Maintenance Mode"),
        )
        .description("Switch public traffic to read-only mode"),
    ]
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Gradient Widget Showcase")
        .mount(GradientWidgets)
        .run()
}
