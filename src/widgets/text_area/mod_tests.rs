use std::sync::Arc;

use crate::style::{Color, Span, Style};

use super::{
    TextArea, TextAreaLineNumberMode, TextAreaSentinel, TextAreaVimConfig,
    TextAreaVimCurrentLineHighlight,
};

#[test]
fn sentinel_hover_style_builders_store_styles() {
    let sentinel = TextAreaSentinel::new("[More]").hover_style(Style::new().fg(Color::Cyan));
    assert_eq!(sentinel.hover_style, Some(Style::new().fg(Color::Cyan)));

    let text_area =
        TextArea::new("").image_placeholder_hover_style(Style::new().fg(Color::Magenta));
    assert_eq!(
        text_area.image_placeholder_hover_style,
        Style::new().fg(Color::Magenta)
    );
}

#[test]
fn vim_config_builders_store_current_line_highlight() {
    let config = TextAreaVimConfig::new()
        .search_bar_prefix_style(Style::new().fg(Color::Magenta))
        .search_bar_count_style(Style::new().fg(Color::Green))
        .current_search_match_style(Style::new().bg(Color::Yellow))
        .current_line_highlight(TextAreaVimCurrentLineHighlight::Content)
        .current_line_style(Style::new().bg(Color::Blue))
        .current_line_number_style(Style::new().fg(Color::Cyan));
    let text_area = TextArea::new("")
        .vim_config(config.clone())
        .highlight_vim_current_line(true);

    assert_eq!(
        config.search_bar_prefix_style.explicit_style(),
        Some(Style::new().fg(Color::Magenta))
    );
    assert_eq!(
        config.search_bar_count_style.explicit_style(),
        Some(Style::new().fg(Color::Green))
    );
    assert_eq!(
        config.current_search_match_style.explicit_style(),
        Some(Style::new().bg(Color::Yellow))
    );
    assert_eq!(
        config.current_line_highlight,
        TextAreaVimCurrentLineHighlight::Content
    );
    assert_eq!(
        text_area.vim_config.current_line_highlight,
        TextAreaVimCurrentLineHighlight::Full
    );
    assert_eq!(
        config.current_line_number_style.explicit_style(),
        Some(Style::new().fg(Color::Cyan))
    );
}

#[test]
fn line_number_mode_builder_stores_relative_mode() {
    let text_area = TextArea::new("").line_number_mode(TextAreaLineNumberMode::Relative);

    assert_eq!(text_area.line_number_mode, TextAreaLineNumberMode::Relative);
}

#[test]
fn decoration_builders_store_byte_range_decorations() {
    let decoration = super::TextAreaDecoration {
        range: 1..3,
        style: Style::new().fg(Color::Yellow),
        priority: 10,
        kind: super::TextAreaDecorationKind::Range,
    };
    let text_area = TextArea::new("abc").decoration(decoration.clone());

    assert_eq!(text_area.decorations, vec![decoration]);
}

#[test]
fn virtual_text_builders_store_non_editable_text() {
    let inline = super::TextAreaVirtualText::inline(1, vec![Span::new("hint")]).priority(2);
    let eol = super::TextAreaVirtualText::eol(3, vec![Span::new("diagnostic")]);
    let text_area = TextArea::new("abc")
        .virtual_text(inline.clone())
        .virtual_texts([eol.clone()]);

    assert_eq!(text_area.virtual_texts, vec![inline, eol]);
}

#[test]
fn composable_gutter_lowers_line_numbers_and_signs_to_custom_gutter() {
    let gutter = super::TextAreaGutter::new()
        .line_numbers(TextAreaLineNumberMode::Absolute)
        .signs([super::TextAreaGutterSign::new(1, vec![Span::new("!")])]);
    let text_area = TextArea::new("one\ntwo").gutter(gutter);

    assert!(!text_area.line_numbers);
    assert!(
        text_area
            .gutter_lines
            .as_ref()
            .is_some_and(|lines| lines.len() == 2)
    );
    assert!(text_area.gutter_col_width > 0);
}

#[test]
fn custom_gutter_column_lowers_to_existing_fields() {
    let custom = Arc::new(vec![vec![Span::new("A")]]);
    let gutter =
        super::TextAreaGutter::new().column(super::TextAreaGutterColumn::custom(custom.clone(), 1));
    let text_area = TextArea::new("one").gutter(gutter);

    assert_eq!(text_area.gutter_col_width, 1);
    assert_eq!(text_area.gutter_lines.as_ref(), Some(&custom));
}
