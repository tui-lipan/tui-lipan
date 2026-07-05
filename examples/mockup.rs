use tui_lipan::prelude::*;

fn main() -> Result<()> {
    mockup!("Mockup Demo – Layout Prototyping", {
        VStack::new()
            .child(
                HStack::new().gap(1).child(sidebar()).child(
                    VStack::new()
                        .gap(1)
                        .child(metrics_row())
                        .child(main_content()),
                ),
            )
            .child(status_bar())
    })
}

fn sidebar() -> Element {
    Frame::new()
        .title("Navigation")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .width(Length::Px(28))
        .style(Style::new().bg(Color::indexed(235)))
        .title_style(Style::new().fg(Color::rgb(168, 130, 255)).bold())
        .child(
            List::new()
                .items([
                    ListItem::new("  Dashboard"),
                    ListItem::new("  Packages"),
                    ListItem::new("  Settings"),
                    ListItem::new("  Logs"),
                    ListItem::new("  Deployments"),
                ])
                .selected(0)
                .style(Style::new().fg(Color::indexed(252)))
                .selection_style(Style::new().bg(Color::indexed(24)).fg(Color::White))
                .selection_symbol(Some("▸ "))
                .selection_symbol_style(Style::new().fg(Color::rgb(168, 130, 255)).bold()),
        )
        .into()
}

fn metrics_row() -> Element {
    HStack::new()
        .gap(1)
        .height(Length::Px(8))
        .child(metric_card(
            "CPU",
            vec![10, 20, 35, 50, 65, 45, 30, 25, 15, 10, 30, 65, 80, 70, 55],
            Color::Green,
        ))
        .child(metric_card(
            "Memory",
            vec![55, 58, 60, 62, 61, 63, 64, 62, 60, 59, 57, 56, 55, 54, 53],
            Color::Cyan,
        ))
        .child(metric_card(
            "Network",
            vec![5, 12, 8, 15, 25, 40, 35, 28, 22, 18, 14, 10, 8, 6, 5],
            Color::rgb(249, 115, 22),
        ))
        .into()
}

fn metric_card(title: impl Into<String>, data: Vec<u64>, color: Color) -> Element {
    Frame::new()
        .title(title.into())
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().bg(Color::indexed(235)))
        .title_style(Style::new().fg(color).bold())
        .child(
            Sparkline::new(data)
                .style(Style::new().fg(color))
                .min(0)
                .max(100),
        )
        .into()
}

fn main_content() -> Element {
    Frame::new()
        .title("Overview")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .style(Style::new().bg(Color::indexed(235)).fg(Color::indexed(252)))
        .title_style(Style::new().fg(Color::rgb(88, 166, 255)).bold())
        .child(
            VStack::new()
                .gap(1)
                .child(
                    Text::new("mockup! - Fast TUI Prototyping")
                        .style(Style::new().fg(Color::rgb(88, 166, 255)).bold()),
                )
                .child(Text::new(
                    "Use mockup! to preview layouts without writing any component logic.",
                ))
                .child(Text::new(
                    "No Message, no State, no update() - just the view.",
                ))
                .child(Text::new(""))
                .child(
                    Frame::new()
                        .title("Usage")
                        .border(true)
                        .border_style(BorderStyle::Rounded)
                        .padding(1)
                        .title_style(Style::new().fg(Color::rgb(16, 185, 129)).bold())
                        .child(
                            VStack::new()
                                .child(Text::new("mockup!(\"Title\", {").style(
                                    Style::new().fg(Color::rgb(16, 185, 129)),
                                ))
                                .child(Text::new("    Frame::new()").style(
                                    Style::new().fg(Color::rgb(16, 185, 129)),
                                ))
                                .child(Text::new("        .title(\"Hello\")").style(
                                    Style::new().fg(Color::rgb(16, 185, 129)),
                                ))
                                .child(Text::new("        .child(Text::new(\"World\"))").style(
                                    Style::new().fg(Color::rgb(16, 185, 129)),
                                ))
                                .child(Text::new("})").style(
                                    Style::new().fg(Color::rgb(16, 185, 129)),
                                )),
                        ),
                )
                .child(Text::new(""))
                .child(
                    Text::new("Interactive widgets (tabs, lists, inputs) still respond to focus and mouse.")
                        .style(Style::new().fg(Color::indexed(244)).italic()),
                )
                .child(
                    Text::new("Press Esc or q to quit.")
                        .style(Style::new().fg(Color::indexed(244)).italic()),
                ),
        )
        .into()
}

fn status_bar() -> Element {
    StatusBar::new()
        .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
        .padding((0, 1))
        .left(
            Text::new(" MOCKUP ").style(
                Style::new()
                    .bg(Color::rgb(168, 130, 255))
                    .fg(Color::Black)
                    .bold(),
            ),
        )
        .center(Text::new("Layout Preview"))
        .right(Text::new(" Esc to quit ").style(Style::new().fg(Color::indexed(244))))
        .into()
}
