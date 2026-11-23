use tui_lipan::prelude::*;

fn main() -> Result<()> {
    mockup!("Length::Percent Demo", {
        VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("Horizontal Percent Widths")
                    .border(true)
                    .height(Length::Percent(35))
                    .child(
                        HStack::new()
                            .gap(1)
                            .child(width_card(
                                "25%",
                                Length::Percent(25),
                                Color::rgb(59, 130, 246),
                            ))
                            .child(width_card(
                                "35%",
                                Length::Percent(35),
                                Color::rgb(16, 185, 129),
                            ))
                            .child(width_card(
                                "Flex(1)",
                                Length::Flex(1),
                                Color::rgb(245, 158, 11),
                            )),
                    ),
            )
            .child(
                Frame::new()
                    .title("Vertical Percent Heights")
                    .border(true)
                    .child(
                        HStack::new()
                            .gap(1)
                            .child(
                                VStack::new()
                                    .gap(1)
                                    .width(Length::Percent(50))
                                    .child(height_card(
                                        "20%",
                                        Length::Percent(20),
                                        Color::rgb(6, 95, 70),
                                    ))
                                    .child(height_card(
                                        "40%",
                                        Length::Percent(40),
                                        Color::rgb(161, 98, 7),
                                    ))
                                    .child(height_card(
                                        "Flex(1)",
                                        Length::Flex(1),
                                        Color::rgb(30, 64, 175),
                                    )),
                            )
                            .child(
                                Frame::new()
                                    .title("Notes")
                                    .border(true)
                                    .width(Length::Flex(1))
                                    .padding(1)
                                    .child(
                                        VStack::new()
                                            .child(Text::new(
                                                "- Percent values are clamped to 0..=100",
                                            ))
                                            .child(Text::new(
                                                "- Percent resolves from parent available space",
                                            ))
                                            .child(Text::new(
                                                "- Flex still consumes the remaining space",
                                            )),
                                    ),
                            ),
                    ),
            )
    })
}

fn width_card(title: &'static str, width: Length, color: Color) -> Element {
    Frame::new()
        .title(title)
        .border(true)
        .width(width)
        .style(Style::new().bg(color).fg(Color::White))
        .child(Text::new(title).style(Style::new().bold()))
        .into()
}

fn height_card(title: &'static str, height: Length, color: Color) -> Element {
    Frame::new()
        .title(title)
        .border(true)
        .height(height)
        .style(Style::new().bg(color).fg(Color::White))
        .child(Center::new().child(Text::new(title).style(Style::new().bold())))
        .into()
}
