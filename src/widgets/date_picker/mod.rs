//! Date picker widget.

mod types;
mod utils;

pub use types::*;
pub(crate) use utils::*;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, IntoElement};
use crate::core::event::{KeyCode, KeyEvent, MouseEvent};
use crate::style::{BorderStyle, Length, Padding, Style, StyleSlot};
use crate::widgets::{BorderLabels, Button, Center, Frame, FrameLabel, HStack, Text, VStack};
use std::sync::Arc;

/// A simple calendar-based date selection widget.
///
/// Keyboard model (WAI-ARIA grid / roving tabindex):
/// - The selected day is the sole tab stop and focus entry.
/// - Other in-month days stay mouse-activatable but are not focusable, so Tab
///   cannot walk cell-by-cell.
/// - Left/Right move ±1 day, Up/Down ±7 days (emitting [`DateEvent`] via
///   `on_select`, including across month boundaries).
/// - PageUp / PageDown move to the previous / next month with the day clamped
///   to that month's length (via `on_select` when set, otherwise the month
///   callbacks).
/// - Home / End go to the first / last day of the **month** (a picker-oriented
///   variant of the ARIA grid pattern, which uses week bounds).
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
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) focus_key: Option<Arc<str>>,
    pub(crate) on_focus: Option<Callback<DateEvent>>,
    pub(crate) on_blur: Option<Callback<DateEvent>>,
    pub(crate) on_select: Option<Callback<DateEvent>>,
    pub(crate) on_prev_month: Option<Callback<()>>,
    pub(crate) on_next_month: Option<Callback<()>>,
    pub(crate) on_key: Option<KeyHandler>,
}

fn derived_focus_key(title: Option<&str>) -> Arc<str> {
    match title {
        Some(title) if !title.is_empty() => Arc::from(format!("__tui_lipan_datepicker:{title}")),
        _ => Arc::from("__tui_lipan_datepicker"),
    }
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
            disabled: false,
            disabled_style: Style::default(),
            focusable: true,
            tab_stop: true,
            focus_key: None,
            on_focus: None,
            on_blur: None,
            on_select: None,
            on_prev_month: None,
            on_next_month: None,
            on_key: None,
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

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Control whether the selected day cell is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the selected day participates in tab traversal.
    ///
    /// Non-selected days are never tab stops (roving focus).
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Key applied to the selected day so focus follows arrow-driven selection.
    ///
    /// When omitted, the key is derived from `title` when set (so distinct titled
    /// pickers do not collide), otherwise a shared default. The key must stay
    /// stable across month changes — do not encode year/month. Override when
    /// two untitled pickers share a tree (or to give the picker an app-owned key).
    pub fn focus_key(mut self, key: impl Into<Arc<str>>) -> Self {
        self.focus_key = Some(key.into());
        self
    }

    /// Set the callback fired when the selected day cell gains focus.
    pub fn on_focus(mut self, cb: Callback<DateEvent>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the selected day cell loses focus.
    pub fn on_blur(mut self, cb: Callback<DateEvent>) -> Self {
        self.on_blur = Some(cb);
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

    /// Set focused key handler on the selected day (runs before built-in arrows).
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
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
        let focus_key = picker
            .focus_key
            .clone()
            .unwrap_or_else(|| derived_focus_key(picker.title.as_deref()));

        let header_label = format!(
            "{} {}",
            months[(month.saturating_sub(1) % 12) as usize],
            year
        );

        let mut prev_button = Button::filled("◀")
            .padding(0)
            .style(picker.nav_style)
            .hover_style_slot(picker.nav_hover_style)
            .width(Length::Px(2))
            .focusable(false)
            .tab_stop(false);
        let mut next_button = Button::filled("▶")
            .padding(0)
            .style(picker.nav_style)
            .hover_style_slot(picker.nav_hover_style)
            .width(Length::Px(2))
            .focusable(false)
            .tab_stop(false);

        if picker.disabled {
            prev_button = prev_button
                .disabled(true)
                .disabled_style(picker.nav_disabled_style);
            next_button = next_button
                .disabled(true)
                .disabled_style(picker.nav_disabled_style);
        } else if let Some(cb) = picker.on_prev_month.clone() {
            prev_button = prev_button.on_click(Callback::new(move |_: MouseEvent| cb.emit(())));
        } else {
            prev_button = prev_button
                .disabled(true)
                .disabled_style(picker.nav_disabled_style);
        }

        if !picker.disabled {
            if let Some(cb) = picker.on_next_month.clone() {
                next_button = next_button.on_click(Callback::new(move |_: MouseEvent| cb.emit(())));
            } else {
                next_button = next_button
                    .disabled(true)
                    .disabled_style(picker.nav_disabled_style);
            }
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
                    let is_selected = day_counter == day;
                    let label = format!("{:>2}", day_counter);
                    let cell_event = DateEvent {
                        year,
                        month,
                        day: day_counter,
                    };
                    let mut button = Button::filled(label)
                        .padding(0)
                        .width(Length::Px(2))
                        .style(if is_selected {
                            picker.selected_style
                        } else {
                            picker.day_style
                        })
                        .hover_style_slot(picker.day_hover_style)
                        // Only the selected day is focusable so arrow-driven
                        // selection can reclaim focus via `focus_key`.
                        .focusable(picker.focusable && is_selected && !picker.disabled)
                        .tab_stop(picker.tab_stop && is_selected && !picker.disabled);

                    if is_selected {
                        if let Some(cb) = picker.on_focus.clone() {
                            button = button.on_focus(Callback::new(move |_| cb.emit(cell_event)));
                        }
                        if let Some(cb) = picker.on_blur.clone() {
                            button = button.on_blur(Callback::new(move |_| cb.emit(cell_event)));
                        }
                    }

                    if picker.disabled {
                        button = button.disabled(true).disabled_style(picker.disabled_style);
                    } else if let Some(cb) = picker.on_select.clone() {
                        let event = cell_event;
                        button =
                            button.on_click(Callback::new(move |_: MouseEvent| cb.emit(event)));
                    } else {
                        button = button.disabled(true).disabled_style(picker.day_style);
                    }

                    if is_selected && !picker.disabled {
                        let on_select = picker.on_select.clone();
                        let on_prev = picker.on_prev_month.clone();
                        let on_next = picker.on_next_month.clone();
                        let caller_on_key = picker.on_key.clone();
                        button = button.on_key(KeyHandler::new(move |key: KeyEvent| {
                            if caller_on_key
                                .as_ref()
                                .is_some_and(|handler| handler.handle(key))
                            {
                                return true;
                            }
                            handle_datepicker_key(
                                key, year, month, day, &on_select, &on_prev, &on_next,
                            )
                        }));
                    }

                    let cell: Element = if is_selected {
                        button.key(focus_key.clone())
                    } else {
                        button.into()
                    };
                    row = row.child(cell);
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
            frame = frame
                .header(BorderLabels::new().left(FrameLabel::new(title).style(picker.title_style)));
        }

        frame.into()
    }
}

fn handle_datepicker_key(
    key: KeyEvent,
    year: i32,
    month: u32,
    day: u32,
    on_select: &Option<Callback<DateEvent>>,
    on_prev: &Option<Callback<()>>,
    on_next: &Option<Callback<()>>,
) -> bool {
    if key.mods.ctrl || key.mods.alt || key.mods.super_key {
        return false;
    }

    match key.code {
        KeyCode::Left => {
            emit_day_delta(year, month, day, -1, on_select);
            true
        }
        KeyCode::Right => {
            emit_day_delta(year, month, day, 1, on_select);
            true
        }
        KeyCode::Up => {
            emit_day_delta(year, month, day, -7, on_select);
            true
        }
        KeyCode::Down => {
            emit_day_delta(year, month, day, 7, on_select);
            true
        }
        KeyCode::PageUp => {
            let (y, m) = prev_month(year, month);
            let d = day.min(days_in_month(y, m));
            if let Some(cb) = on_select {
                cb.emit(DateEvent {
                    year: y,
                    month: m,
                    day: d,
                });
            } else if let Some(cb) = on_prev {
                cb.emit(());
            }
            true
        }
        KeyCode::PageDown => {
            let (y, m) = next_month(year, month);
            let d = day.min(days_in_month(y, m));
            if let Some(cb) = on_select {
                cb.emit(DateEvent {
                    year: y,
                    month: m,
                    day: d,
                });
            } else if let Some(cb) = on_next {
                cb.emit(());
            }
            true
        }
        KeyCode::Home => {
            if let Some(cb) = on_select {
                cb.emit(DateEvent {
                    year,
                    month,
                    day: 1,
                });
            }
            true
        }
        KeyCode::End => {
            if let Some(cb) = on_select {
                cb.emit(DateEvent {
                    year,
                    month,
                    day: days_in_month(year, month),
                });
            }
            true
        }
        _ => false,
    }
}

fn emit_day_delta(
    year: i32,
    month: u32,
    day: u32,
    delta: i32,
    on_select: &Option<Callback<DateEvent>>,
) {
    let Some(cb) = on_select else {
        return;
    };
    let (y, m, d) = shift_day(year, month, day, delta);
    if y == year && m == month && d == day {
        return;
    }
    cb.emit(DateEvent {
        year: y,
        month: m,
        day: d,
    });
}
