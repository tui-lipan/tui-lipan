use ratatui::style::Color as RColor;

use crate::style::Color;

pub(crate) fn to_ratatui_color(color: Color) -> RColor {
    match color {
        Color::Reset => RColor::Reset,
        // Prefer omitting fg/bg in `to_ratatui_style` for true inherit behavior.
        Color::Backdrop => RColor::Reset,
        Color::Transparent => RColor::Reset,
        Color::Black => RColor::Black,
        Color::Red => RColor::Red,
        Color::Green => RColor::Green,
        Color::Yellow => RColor::Yellow,
        Color::Blue => RColor::Blue,
        Color::Magenta => RColor::Magenta,
        Color::Cyan => RColor::Cyan,
        Color::Gray => RColor::Gray,
        Color::DarkGray => RColor::DarkGray,
        Color::LightRed => RColor::LightRed,
        Color::LightGreen => RColor::LightGreen,
        Color::LightYellow => RColor::LightYellow,
        Color::LightBlue => RColor::LightBlue,
        Color::LightMagenta => RColor::LightMagenta,
        Color::LightCyan => RColor::LightCyan,
        Color::White => RColor::White,
        Color::Indexed(i) => RColor::Indexed(i),
        Color::Rgb(r, g, b) => RColor::Rgb(r, g, b),
    }
}

pub(crate) fn from_ratatui_color(color: RColor) -> Color {
    match color {
        RColor::Reset => Color::Reset,
        RColor::Black => Color::Black,
        RColor::Red => Color::Red,
        RColor::Green => Color::Green,
        RColor::Yellow => Color::Yellow,
        RColor::Blue => Color::Blue,
        RColor::Magenta => Color::Magenta,
        RColor::Cyan => Color::Cyan,
        RColor::Gray => Color::Gray,
        RColor::DarkGray => Color::DarkGray,
        RColor::LightRed => Color::LightRed,
        RColor::LightGreen => Color::LightGreen,
        RColor::LightYellow => Color::LightYellow,
        RColor::LightBlue => Color::LightBlue,
        RColor::LightMagenta => Color::LightMagenta,
        RColor::LightCyan => Color::LightCyan,
        RColor::White => Color::White,
        RColor::Indexed(i) => Color::Indexed(i),
        RColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
