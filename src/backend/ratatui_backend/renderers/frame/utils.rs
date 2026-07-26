use crate::backend::ratatui_backend::common::{border_tabs_title_line, truncate_spans};
use crate::style::Style;
use crate::widgets::internal::FrameProps;
use ratatui::text::Line;

pub(crate) fn build_tabs_line<'a>(
    props: &'a FrameProps,
    block_style: Style,
    active: bool,
    width: u16,
) -> Option<Line<'a>> {
    if props.has_header || props.tab_titles.is_empty() {
        return None;
    }

    let mut active_tab_style = block_style.patch(props.active_tab_style);
    let mut inactive_tab_style = block_style.patch(props.inactive_tab_style);
    if active {
        if let Some(style) = props.focus_active_tab_style() {
            active_tab_style = active_tab_style.patch(style);
        }
        if let Some(style) = props.focus_inactive_tab_style() {
            inactive_tab_style = inactive_tab_style.patch(style);
        }
    }
    let mut title_style = block_style.patch(props.header.style);
    if active && let Some(focused_style) = props.header.focused_style {
        title_style = title_style.patch(focused_style);
    }
    let line = border_tabs_title_line(
        &props.tab_titles,
        props.active_tab,
        active_tab_style,
        inactive_tab_style,
        props.tab_variant,
        block_style,
        title_style,
    );
    Some(Line::from(truncate_spans(line.spans, width)))
}
