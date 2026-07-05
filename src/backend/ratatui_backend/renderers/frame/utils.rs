use crate::backend::ratatui_backend::common::{
    border_tabs_title_line, richtext_to_spans, to_ratatui_style_with_terminal_bg, truncate_spans,
};
use crate::style::{Align, Style};
use crate::widgets::internal::FrameProps;
use ratatui::text::{Line, Span};

pub(crate) fn build_header_line<'a>(
    props: &'a FrameProps,
    block_style: Style,
    active: bool,
    width: u16,
    h_char: &str,
    terminal_bg: Option<crate::style::Color>,
) -> Option<Line<'a>> {
    if props.has_header {
        return None;
    }

    let mut title_line: Option<Line> = None;
    let max_title_w = width.saturating_sub(2);

    if !props.tab_titles.is_empty() {
        let mut active_tab_style = block_style.patch(props.active_tab_style);
        let mut title_style = block_style.patch(props.title_style);
        if active && let Some(fts) = props.focus_title_style() {
            title_style = block_style.patch(fts);
        }
        if active && let Some(fts) = props.focus_active_tab_style() {
            active_tab_style = block_style.patch(fts);
        }
        let mut inactive_tab_style = block_style.patch(props.inactive_tab_style);
        if active && let Some(ifts) = props.focus_inactive_tab_style() {
            inactive_tab_style = block_style.patch(ifts);
        }
        title_line = Some(border_tabs_title_line(
            &props.tab_titles,
            props.active_tab,
            active_tab_style,
            inactive_tab_style,
            props.tab_variant,
            block_style,
            title_style,
        ));
    } else if let Some(t) = &props.title {
        let mut title_style = block_style.patch(props.title_style);
        if active && let Some(fts) = props.focus_title_style() {
            title_style = block_style.patch(fts);
        }
        let spans = richtext_to_spans(t, title_style);
        let spans = truncate_spans(spans, max_title_w);
        title_line = Some(Line::from(spans));
    }

    if let Some(prefix) = &props.title_prefix {
        let mut title_style = block_style.patch(props.title_style);
        if active && let Some(fts) = props.focus_title_style() {
            title_style = block_style.patch(fts);
        }
        let prefix_spans = richtext_to_spans(prefix, title_style);

        if let Some(mut line) = title_line {
            let sep_span = Span::styled(
                h_char.to_string(),
                to_ratatui_style_with_terminal_bg(block_style, terminal_bg),
            );

            for span in prefix_spans.into_iter().rev() {
                line.spans.insert(0, span);
            }
            line.spans.insert(prefix.spans.len(), sep_span);
            title_line = Some(line);
        } else {
            title_line = Some(Line::from(prefix_spans));
        }
    }

    if let Some(suffix) = &props.title_suffix {
        let mut title_style = block_style.patch(props.title_style);
        if active && let Some(fts) = props.focus_title_style() {
            title_style = block_style.patch(fts);
        }
        let suffix_spans = richtext_to_spans(suffix, title_style);

        if let Some(mut line) = title_line {
            let sep_span = Span::styled(
                h_char.to_string(),
                to_ratatui_style_with_terminal_bg(block_style, terminal_bg),
            );

            line.spans.push(sep_span);
            line.spans.extend(suffix_spans);
            title_line = Some(line);
        } else {
            title_line = Some(Line::from(suffix_spans));
        }
    }

    if let Some(mut line) = title_line {
        let p = props.header_padding;
        if p.left > 0 || p.right > 0 {
            let left_span = Span::styled(
                h_char.repeat(p.left as usize),
                to_ratatui_style_with_terminal_bg(block_style, terminal_bg),
            );
            let right_span = Span::styled(
                h_char.repeat(p.right as usize),
                to_ratatui_style_with_terminal_bg(block_style, terminal_bg),
            );
            if p.left > 0 {
                line.spans.insert(0, left_span);
            }
            if p.right > 0 {
                line.spans.push(right_span);
            }
        }

        let aligned = match props.title_alignment {
            Align::Start | Align::Stretch => line.left_aligned(),
            Align::Center => line.centered(),
            Align::End => line.right_aligned(),
        };
        Some(aligned)
    } else {
        None
    }
}
