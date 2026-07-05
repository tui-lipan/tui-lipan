fn view() {
    let el = ui! {
        Frame::new().title("Root") => {
            VStack::new() => {
                Text::new("a"),
                Button::new("b"),
            },
        }
    };
}
