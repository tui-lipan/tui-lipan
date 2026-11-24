fn view() {
    let el = ui! {
        Frame::new()
            .border(false)
            .style(Style::new().bg(input_bg))
            .padding((1, 2, 0, 2))
            .height(Length::Auto)
            .decorations(
                vec![
                    EdgeDecoration::new(Edge::Bottom)
                        .glyph(DecorationGlyph::HalfBlock)
                        .style(Style::new().fg(input_bg).bg(theme.background))
                        .placement(DecorationPlacement::Outside),
                    EdgeDecoration::new(Edge::Left)
                        .glyph(DecorationGlyph::AutoBlock)
                        .style(Style::new().fg(status_accent_color).bg(theme.background))
                        .cap_end(DecorationGlyph::CapBottom),
                ],
            ) => {
            VStack::new().height(Length::Auto).gap(1) => {
                prompt_input_element,
                HStack::new().height(Length::Px(1)) => {
                    status_text,
                },
            },
        }
    };
}
