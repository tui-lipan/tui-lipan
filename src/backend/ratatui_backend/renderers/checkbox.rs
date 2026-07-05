use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    finalize_style, resolve_interactive_style_raw, style_backdrop, to_ratatui_rect,
    to_ratatui_style,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::NodeId;
use crate::style::resolve::{resolve_base_style, resolve_force_accent_style, resolve_muted_style};
use crate::style::{Padding, Rect, Style, ThemeRole, resolve_slot};
use crate::widgets::{CheckboxState, CheckboxVariant};

pub(crate) struct CheckboxRenderCtx {
    pub gap: u16,
    pub style: Style,
    pub hover_style: Style,
    pub focus_style: Style,
    pub checked_style: Style,
    pub unchecked_style: Style,
    pub indeterminate_style: Style,
    pub label_style: Style,
    pub padding: Padding,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub disabled: bool,
    pub disabled_style: Style,
    pub contrast_policy: ContrastPolicy,
    pub clip_rect: Option<Rect>,
}

pub(crate) fn render_checkbox(
    f: &mut ratatui::Frame<'_>,
    state: CheckboxState,
    label: Option<&str>,
    variant: CheckboxVariant,
    rect: Rect,
    ctx: CheckboxRenderCtx,
) {
    let CheckboxRenderCtx {
        gap,
        style,
        hover_style,
        focus_style,
        checked_style,
        unchecked_style,
        indeterminate_style,
        label_style,
        padding,
        is_focused,
        is_hovered,
        disabled,
        disabled_style,
        contrast_policy,
        clip_rect,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let inner = rect.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let symbol = match state {
        CheckboxState::Checked => variant.checked_str(),
        CheckboxState::Unchecked => variant.unchecked_str(),
        CheckboxState::Indeterminate => variant.indeterminate_str(),
    };
    let symbol_w = UnicodeWidthStr::width(symbol);

    let sym_base = if disabled {
        style
    } else {
        match state {
            CheckboxState::Checked => style.patch(checked_style),
            CheckboxState::Unchecked => style.patch(unchecked_style),
            CheckboxState::Indeterminate => style.patch(indeterminate_style),
        }
    };
    let base_s = finalize_style(
        resolve_interactive_style_raw(
            style,
            focus_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            disabled,
        ),
        None,
        contrast_policy,
    );
    let base_backdrop = style_backdrop(base_s);

    let sym_s = finalize_style(
        resolve_interactive_style_raw(
            sym_base,
            focus_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            disabled,
        ),
        base_backdrop,
        contrast_policy,
    );

    let lbl_s = finalize_style(
        resolve_interactive_style_raw(
            base_s.patch(label_style),
            focus_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            disabled,
        ),
        base_backdrop,
        contrast_policy,
    );

    let mut spans = Vec::new();

    spans.push(ratatui::text::Span::styled(symbol, to_ratatui_style(sym_s)));

    if let Some(lbl) = label
        && !lbl.is_empty()
        && inner.w as usize > symbol_w + gap as usize
    {
        let gap_str = " ".repeat(gap as usize);
        spans.push(ratatui::text::Span::styled(
            gap_str,
            to_ratatui_style(Style::default()),
        ));

        let remaining = (inner.w as usize)
            .saturating_sub(symbol_w)
            .saturating_sub(gap as usize);

        if remaining > 0 {
            let label_w = UnicodeWidthStr::width(lbl);
            let display_label = if label_w <= remaining {
                lbl.to_string()
            } else {
                let mut truncated = String::new();
                let mut w = 0usize;
                for ch in lbl.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if w + cw + 1 > remaining {
                        break;
                    }
                    truncated.push(ch);
                    w += cw;
                }
                truncated.push('…');
                truncated
            };
            spans.push(ratatui::text::Span::styled(
                display_label,
                to_ratatui_style(lbl_s),
            ));
        }
    }

    let line = ratatui::text::Line::from(spans);
    let inner_rrect = to_ratatui_rect(inner);
    let effective_rrect = if let Some(clip) = clip_rect {
        let r_clip = to_ratatui_rect(clip);
        inner_rrect.intersection(r_clip)
    } else {
        inner_rrect
    };

    if !effective_rrect.is_empty() {
        let dx = (effective_rrect.x as i32)
            .saturating_sub(inner.x as i32)
            .max(0) as u16;
        let dy = (effective_rrect.y as i32)
            .saturating_sub(inner.y as i32)
            .max(0) as u16;
        let p = Paragraph::new(line).scroll((dy, dx));
        f.render_widget(p, effective_rrect);
    }
}

pub(crate) fn render_checkbox_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::CheckboxNode,
    rect: Rect,
    _rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !node.disabled;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = state.ctx.tree.node(node_id).active_theme();
    render_checkbox(
        state.f,
        node.state,
        node.label.as_deref(),
        node.variant,
        rect,
        CheckboxRenderCtx {
            gap: node.gap,
            style: resolve_force_accent_style(theme, node.style),
            hover_style: resolve_slot(theme, ThemeRole::Hover, &node.hover_style),
            focus_style: resolve_slot(theme, ThemeRole::Focus, &node.focus_style),
            checked_style: resolve_force_accent_style(theme, node.checked_style),
            unchecked_style: resolve_force_accent_style(theme, node.unchecked_style),
            indeterminate_style: resolve_force_accent_style(theme, node.indeterminate_style),
            label_style: resolve_base_style(theme, node.label_style),
            padding: node.padding,
            is_focused,
            is_hovered,
            disabled: node.disabled,
            disabled_style: resolve_muted_style(theme, node.disabled_style),
            contrast_policy,
            clip_rect: clip_bounds,
        },
    );
}
