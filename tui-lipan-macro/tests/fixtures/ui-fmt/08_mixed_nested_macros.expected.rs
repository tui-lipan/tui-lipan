fn view(kind: Kind) {
    let nested = ui! {
        VStack::new() => {
            if matches!(kind, Kind::A) {
                Text::new(format!("{}", rsx! { Text { content : "x" } })),
            } else {
                Text::new(format!("{}", ui! { Text::new("inner") })),
            },
        }
    };
    let rsx_outer = rsx! {
        Frame {
            content: ui! {
                VStack::new() => { Text::new("r") }
            },
        }
    };
}
