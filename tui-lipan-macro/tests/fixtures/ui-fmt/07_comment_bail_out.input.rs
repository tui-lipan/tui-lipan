fn view() {
    let keep = ui! {
        VStack::new() => {
            // preserve comment and do not reflow
            Text::new("x")
        }
    };
    let clean = ui! {
        VStack::new() => {
            Text::new("y"),
        }
    };
}
