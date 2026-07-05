//! Date picker widget.

mod types;
mod utils;

pub use types::*;
pub(crate) use utils::*;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::event::MouseEvent;
use crate::style::{BorderStyle, Length, Padding, Style, StyleSlot};
use crate::widgets::{Button, Center, Frame, HStack, Text, VStack};
use std::sync::Arc;

/// A simple calendar-based date selection widget.
#[derive(Clone)]
pub struct DatePicker {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) title: Option<Arc<str>>,
    pub(crate) title_style: Style,
    pub(crate) style: Style,
    pub(crate) header_style: Style,
    pub(crate) weekday_style: Style,
    pub(crate) day_style: Style,
    pub(crate) day_hover_style: StyleSlot,
    pub(crate) selected_style: Style,
    pub(crate) outside_month_style: Style,
    pub(crate) nav_style: Style,
    pub(crate) nav_hover_style: StyleSlot,
    pub(crate) nav_disabled_style: Style,
    pub(crate) show_outside_days: bool,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) on_select: Option<Callback<DateEvent>>,
    pub(crate) on_prev_month: Option<Callback<()>>,
    pub(crate) on_next_month: Option<Callback<()>>,
}

impl DatePicker {
    /// Create a new date picker.
    pub fn new() -> Self {
        Self {
            year: 2024,
            month: 1,
            day: 1,
            title: Some("Select Date".into()),
            title_style: Style::default(),
            style: Style::default(),
            header_style: Style::default(),
            weekday_style: Style::default(),
            day_style: Style::default(),
            day_hover_style: StyleSlot::Inherit,
            selected_style: Style::default(),
            outside_month_style: Style::default(),
            nav_style: Style::default(),
            nav_hover_style: StyleSlot::Inherit,
            nav_disabled_style: Style::default(),
            show_outside_days: false,
            border: true,
            border_style: BorderStyle::Rounded,
            padding: Padding::default(),
            width: Length::Auto,
            height: Length::Auto,
            on_select: None,
            on_prev_month: None,
            on_next_month: None,
        }
    }

    /// Set the year.
    pub fn year(mut self, year: i32) -> Self {
        self.year = year;
        self
    }

    /// Set the month.
    pub fn month(mut self, month: u32) -> Self {
        self.month = month;
        self
    }

    /// Set the day.
    pub fn day(mut self, day: u32) -> Self {
        self.day = day;
        self
    }

    /// Set the title (None disables the title).
    pub fn title(mut self, title: Option<impl Into<Arc<str>>>) -> Self {
        self.title = title.map(Into::into);
        self
    }

    /// Set title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set header style.
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Set weekday label style.
    pub fn weekday_style(mut self, style: Style) -> Self {
        self.weekday_style = style;
        self
    }

    /// Set day style.
    pub fn day_style(mut self, style: Style) -> Self {
        self.day_style = style;
        self
    }

    /// Set day hover style.
    pub fn day_hover_style(mut self, style: Style) -> Self {
        self.day_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed day hover style.
    pub fn extend_day_hover_style(mut self, style: Style) -> Self {
        self.day_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed day hover style.
    pub fn inherit_day_hover_style(mut self) -> Self {
        self.day_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set day hover style slot directly for composite forwarding.
    pub fn day_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.day_hover_style = slot;
        self
    }

    /// Set selected day style.
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Set outside-month day style.
    pub fn outside_month_style(mut self, style: Style) -> Self {
        self.outside_month_style = style;
        self
    }

    /// Set navigation button style.
    pub fn nav_style(mut self, style: Style) -> Self {
        self.nav_style = style;
        self
    }

    /// Set navigation button hover style.
    pub fn nav_hover_style(mut self, style: Style) -> Self {
        self.nav_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed navigation button hover style.
    pub fn extend_nav_hover_style(mut self, style: Style) -> Self {
        self.nav_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed navigation button hover style.
    pub fn inherit_nav_hover_style(mut self) -> Self {
        self.nav_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set navigation button hover style slot directly for composite forwarding.
    pub fn nav_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.nav_hover_style = slot;
        self
    }

    /// Set navigation button disabled style.
    pub fn nav_disabled_style(mut self, style: Style) -> Self {
        self.nav_disabled_style = style;
        self
    }

    /// Toggle rendering days from adjacent months.
    pub fn show_outside_days(mut self, show: bool) -> Self {
        self.show_outside_days = show;
        self
    }

    /// Draw a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set day selection callback.
    pub fn on_select(mut self, cb: Callback<DateEvent>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set previous-month callback.
    pub fn on_prev_month(mut self, cb: Callback<()>) -> Self {
        self.on_prev_month = Some(cb);
        self
    }

    /// Set next-month callback.
    pub fn on_next_month(mut self, cb: Callback<()>) -> Self {
        self.on_next_month = Some(cb);
        self
    }
}

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl From<DatePicker> for Element {
    fn from(picker: DatePicker) -> Self {
        let months = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];

        let year = picker.year;
        let month = picker.month.clamp(1, 12);
        let day = picker.day.clamp(1, days_in_month(year, month));

        let header_label = format!(
            "{} {}",
            months[(month.saturating_sub(1) % 12) as usize],
            year
        );

        let mut prev_button = Button::filled("◀")
            .padding(0)
            .style(picker.nav_style)
            .hover_style_slot(picker.nav_hover_style)
            .width(Length::Px(2));
        let mut next_button = Button::filled("▶")
            .padding(0)
            .style(picker.nav_style)
            .hover_style_slot(picker.nav_hover_style)
            .width(Length::Px(2));

        if let Some(cb) = picker.on_prev_month.clone() {
            prev_button = prev_button.on_click(Callback::new(move |_: MouseEvent| cb.emit(())));
        } else {
            prev_button = prev_button
                .disabled(true)
                .disabled_style(picker.nav_disabled_style);
        }

        if let Some(cb) = picker.on_next_month.clone() {
            next_button = next_button.on_click(Callback::new(move |_: MouseEvent| cb.emit(())));
        } else {
            next_button = next_button
                .disabled(true)
                .disabled_style(picker.nav_disabled_style);
        }

        let header = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(prev_button)
            .child(Center::new().child(Text::new(header_label).style(picker.header_style)))
            .child(next_button);

        let days_header = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(
                Text::new("Su")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("Mo")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("Tu")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("We")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("Th")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("Fr")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            )
            .child(
                Text::new("Sa")
                    .style(picker.weekday_style)
                    .width(Length::Px(2)),
            );

        let first_weekday = weekday(year, month, 1) as usize;
        let days_in_current_month = days_in_month(year, month);
        let (prev_year, prev_month_val) = prev_month(year, month);
        let days_in_prev = days_in_month(prev_year, prev_month_val);

        let mut calendar = VStack::new().gap(0).height(Length::Auto);
        let mut day_counter = 1u32;
        let mut next_day = 1u32;

        for week in 0..6 {
            let mut row = HStack::new().gap(1).height(Length::Px(1));
            for weekday_idx in 0..7 {
                let cell_index = week * 7 + weekday_idx;

                if cell_index < first_weekday {
                    if picker.show_outside_days {
                        let day_val = days_in_prev - (first_weekday as u32 - cell_index as u32) + 1;
                        let label = format!("{:>2}", day_val);
                        let cell = Text::new(label)
                            .style(picker.outside_month_style)
                            .width(Length::Px(2));
                        row = row.child(cell);
                    } else {
                        row = row.child(Text::new("  ").width(Length::Px(2)));
                    }
                    continue;
                }

                if day_counter <= days_in_current_month {
                    let label = format!("{:>2}", day_counter);
                    let mut button = Button::filled(label)
                        .padding(0)
                        .width(Length::Px(2))
                        .style(picker.day_style)
                        .hover_style_slot(picker.day_hover_style);

                    if day_counter == day {
                        button = button.style(picker.selected_style);
                    }

                    if let Some(cb) = picker.on_select.clone() {
                        let event = DateEvent {
                            year,
                            month,
                            day: day_counter,
                        };
                        button =
                            button.on_click(Callback::new(move |_: MouseEvent| cb.emit(event)));
                    } else {
                        button = button.disabled(true).disabled_style(picker.day_style);
                    }

                    row = row.child(button);
                    day_counter = day_counter.saturating_add(1);
                } else if picker.show_outside_days {
                    let label = format!("{:>2}", next_day);
                    let cell = Text::new(label)
                        .style(picker.outside_month_style)
                        .width(Length::Px(2));
                    row = row.child(cell);
                    next_day = next_day.saturating_add(1);
                } else {
                    row = row.child(Text::new("  ").width(Length::Px(2)));
                }
            }
            calendar = calendar.child(row);
        }

        let content = VStack::new()
            .gap(1)
            .child(header)
            .child(days_header)
            .child(calendar);

        let mut frame = Frame::new()
            .border(picker.border)
            .border_style(picker.border_style)
            .padding(picker.padding)
            .style(picker.style)
            .child(content)
            .width(picker.width)
            .height(picker.height);

        if let Some(title) = picker.title.clone() {
            frame = frame.title(title).title_style(picker.title_style);
        }

        frame.into()
    }
}
