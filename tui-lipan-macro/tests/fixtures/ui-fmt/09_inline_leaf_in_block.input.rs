fn view(animations_enabled: bool, style: Style, width: Length) -> Element {
    if animations_enabled {
        ui! { Spinner::new().spinner_style(SpinnerStyle::Dots).style(style).width(width) }
    } else {
        ui! { Text::new("...").style(style).overflow(Overflow::Wrap).width(width) }
    }
}
