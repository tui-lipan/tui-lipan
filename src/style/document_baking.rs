//! Internal document-oriented theme baking.

#[cfg(feature = "diff-view")]
use std::rc::Rc;

use crate::core::element::{Element, ElementKind};
use crate::style::Theme;
use crate::widgets::DocumentStyles;

/// Apply the remaining render-time theme carve-out to document-oriented content.
///
/// Regular widget styles are resolved by renderers. This pass intentionally only
/// reaches document/syntax/diff/content-formatter state that still needs an app
/// theme before rendering, recursing through element containers and preserving
/// nested [`crate::widgets::ThemeProvider`] scoping.
pub(crate) fn apply_document_theme_carve_out(theme: &Theme, mut el: Element) -> Element {
    apply_document_theme_carve_out_in_place(theme, &mut el);
    el
}

fn apply_document_theme_carve_out_in_place(theme: &Theme, el: &mut Element) {
    match &mut el.kind {
        ElementKind::DocumentView(dv) => {
            apply_document_styles(&mut dv.doc_styles, theme);
            apply_document_content_formatter_theme(dv, theme);
            #[cfg(feature = "syntax-syntect")]
            apply_document_syntax_theme(dv, theme);
            #[cfg(feature = "diff-view")]
            apply_diff_document_theme(dv, theme);
        }
        ElementKind::TextArea(_text_area) => {
            #[cfg(feature = "syntax-syntect")]
            apply_syntect_text_area_theme(_text_area, theme);
            #[cfg(feature = "diff-view")]
            apply_diff_text_area_theme(_text_area, theme);
        }
        ElementKind::ThemeProvider(tp) => {
            apply_document_theme_carve_out_in_place(&tp.theme, &mut tp.child);
        }
        other => {
            for child in other.children_mut() {
                apply_document_theme_carve_out_in_place(theme, child);
            }
        }
    }
}

fn apply_document_styles(styles: &mut DocumentStyles, theme: &Theme) {
    if *styles == DocumentStyles::default() {
        *styles = DocumentStyles::from_theme(theme);
    }
}

fn apply_document_content_formatter_theme(dv: &mut crate::widgets::DocumentView, theme: &Theme) {
    let Some(formatter_rc) = dv.formatter.as_mut() else {
        return;
    };
    if std::rc::Rc::get_mut(formatter_rc).is_none() {
        *formatter_rc = std::rc::Rc::from(formatter_rc.clone_box());
    }
    let Some(fmt) = std::rc::Rc::get_mut(formatter_rc) else {
        return;
    };
    fmt.set_app_theme_if_absent(theme);
}

#[cfg(feature = "syntax-syntect")]
fn apply_syntect_text_area_theme(text_area: &mut crate::widgets::TextArea, theme: &Theme) {
    if let Some(strategy) = text_area.color_strategy.as_mut() {
        crate::widgets::apply_syntect_strategy_app_theme(strategy, theme);
    }
}

#[cfg(feature = "syntax-syntect")]
fn apply_document_syntax_theme(dv: &mut crate::widgets::DocumentView, theme: &Theme) {
    if let Some(strategy) = dv.code_syntax_strategy.as_mut() {
        crate::widgets::apply_syntect_strategy_app_theme(strategy, theme);
    }
}

/// Merge theme diff palette into a widget's diff palette using `Style::patch()`.
#[cfg(feature = "diff-view")]
fn apply_diff_palette_theme(palette: &mut crate::style::DiffPalette, theme: &Theme) {
    let t = &theme.diff;
    palette.context = t.context.patch(palette.context);
    palette.added = t.added.patch(palette.added);
    palette.removed = t.removed.patch(palette.removed);
    palette.empty = t.empty.patch(palette.empty);
    palette.added_word = t.added_word.patch(palette.added_word);
    palette.removed_word = t.removed_word.patch(palette.removed_word);
    palette.added_marker = t.added_marker.patch(palette.added_marker);
    palette.removed_marker = t.removed_marker.patch(palette.removed_marker);
    palette.context_line_number = t.context_line_number.patch(palette.context_line_number);
    palette.added_line_number = t.added_line_number.patch(palette.added_line_number);
    palette.removed_line_number = t.removed_line_number.patch(palette.removed_line_number);
    palette.context_separator_style = t
        .context_separator_style
        .patch(palette.context_separator_style);
}

#[cfg(feature = "diff-view")]
fn apply_diff_text_area_theme(text_area: &mut crate::widgets::TextArea, theme: &Theme) {
    let Some(strategy_rc) = text_area.color_strategy.as_mut() else {
        return;
    };

    // Clone+replace when the Rc is shared (e.g. Element was cloned into a
    // cache before theme application) so that Rc::get_mut succeeds.
    if Rc::get_mut(strategy_rc).is_none() {
        let Some(existing) = strategy_rc
            .as_ref()
            .as_any()
            .downcast_ref::<crate::widgets::DiffColorStrategy>()
        else {
            return;
        };
        *strategy_rc = Rc::new(existing.clone());
    }

    let Some(strategy) = Rc::get_mut(strategy_rc) else {
        return;
    };
    let Some(diff) = strategy
        .as_any_mut()
        .downcast_mut::<crate::widgets::DiffColorStrategy>()
    else {
        return;
    };

    let old_style = diff.style;
    apply_diff_palette_theme(&mut diff.style, theme);
    #[cfg(feature = "syntax-syntect")]
    if let Some(base) = diff.base.as_mut() {
        crate::widgets::apply_syntect_strategy_app_theme(base, theme);
    }
    diff.recompute_strategy_cache_key();
    let themed_padding_gutter_style = diff.style.empty.patch(diff.style.context_line_number);
    if text_area.split_wrap_padding_gutter_style.is_some() {
        text_area.split_wrap_padding_gutter_style = Some(themed_padding_gutter_style);
    }
    if text_area.split_wrap_padding_style.is_some() {
        text_area.split_wrap_padding_style = Some(diff.style.empty);
    }
    if diff.style != old_style && text_area.gutter_lines.is_some() {
        text_area.gutter_lines = Some(crate::widgets::rebuild_diff_gutter_spans(
            diff.lines.as_ref(),
            diff.style,
        ));
    }
}

#[cfg(feature = "diff-view")]
fn apply_diff_document_theme(dv: &mut crate::widgets::DocumentView, theme: &Theme) {
    let Some(formatter_rc) = dv.formatter.as_mut() else {
        return;
    };

    // Try in-place mutation first; fall back to clone+replace when the Rc is
    // shared (e.g. Element was cloned into a cache before theme application).
    if Rc::get_mut(formatter_rc).is_none() {
        let Some(existing) = formatter_rc
            .as_ref()
            .as_any()
            .downcast_ref::<crate::widgets::DiffDocumentFormatter>()
        else {
            return;
        };
        *formatter_rc = Rc::from(crate::widgets::document_view::ContentFormatter::clone_box(
            existing,
        ));
    }

    let Some(formatter) = Rc::get_mut(formatter_rc) else {
        return;
    };
    let Some(diff) = formatter
        .as_any_mut()
        .downcast_mut::<crate::widgets::DiffDocumentFormatter>()
    else {
        return;
    };

    let old_style = diff.strategy().style;
    apply_diff_palette_theme(&mut diff.strategy_mut().style, theme);
    #[cfg(feature = "syntax-syntect")]
    if let Some(base) = diff.strategy_mut().base.as_mut() {
        crate::widgets::apply_syntect_strategy_app_theme(base, theme);
    }
    diff.strategy_mut().recompute_strategy_cache_key();
    diff.refresh_formatter_cache_key();
    let themed_padding_gutter_style = diff
        .strategy()
        .style
        .empty
        .patch(diff.strategy().style.context_line_number);
    if dv.split_wrap_padding_gutter_style.is_some() {
        dv.split_wrap_padding_gutter_style = Some(themed_padding_gutter_style);
    }
    if dv.split_wrap_padding_style.is_some() {
        dv.split_wrap_padding_style = Some(diff.strategy().style.empty);
    }
    if diff.strategy().style != old_style && dv.gutter_lines.is_some() {
        dv.gutter_lines = Some(crate::widgets::rebuild_diff_gutter_spans(
            diff.strategy().lines.as_ref(),
            diff.strategy().style,
        ));
    }
}
