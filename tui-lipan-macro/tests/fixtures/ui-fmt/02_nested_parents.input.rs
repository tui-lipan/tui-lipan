fn view() {
    let el = ui! {
        Frame::new().header_left("Root") => {
            VStack::new() => {
                Text::new("a"),
                Button::new("b"),
            },
        }
    };
}
