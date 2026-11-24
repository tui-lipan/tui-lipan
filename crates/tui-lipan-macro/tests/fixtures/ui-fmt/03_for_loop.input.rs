fn view(items: Vec<String>) {
    let el = ui! {
        VStack::new() => {
            for item in items {
                Text::new(item),
            },
        }
    };
}
