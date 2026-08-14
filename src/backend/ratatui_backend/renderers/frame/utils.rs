use crate::backend::ratatui_backend::common::{border_tabs_title_line, truncate_spans};
use crate::style::Style;
use crate::widgets::internal::FrameProps;
use crate::widgets::{BorderLabels, TabEdge};
use ratatui::text::Line;

/// Build the tab strip for the border line named by `props.tab_edge`.
///
/// `labels` is that line's own label group: tabs share the line with it, so
/// they inherit its style. Returns `None` when this frame draws no tabs, or
/// when a custom header element already owns the top border.
pub(crate) fn build_tabs_line<'a>(
    props: &'a FrameProps,
    labels: &BorderLabels,
    block_style: Style,
    active: bool,
    width: u16,
) -> Option<Line<'a>> {
    // A custom header element occupies the top border, so top tabs have
    // nowhere to go. It says nothing about the bottom one.
    let header_taken = props.has_header && matches!(props.tab_edge, TabEdge::Top);
    if header_taken || props.tab_titles.is_empty() {
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
    let mut title_style = block_style.patch(labels.style);
    if active && let Some(focused_style) = labels.focused_style {
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
