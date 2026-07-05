fn view(a: bool, b: bool) {
    let el = ui! {
        VStack::new() => {
            if a {
                Text::new("a"),
            } else if b {
                Text::new("b"),
            } else {
                Text::new("c"),
            },
        }
    };
}
