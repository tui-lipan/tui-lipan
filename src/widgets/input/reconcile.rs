use crate::widgets::Input;
use crate::widgets::internal::InputNode;

pub(crate) fn reconcile_input(widget: &Input) -> InputNode {
    let cursor = crate::utils::text::clamp_cursor(&widget.value, widget.cursor);
    let anchor = widget
        .anchor
        .map(|anchor| crate::utils::text::clamp_cursor(&widget.value, anchor));

    InputNode {
        value: widget.value.clone(),
        cursor,
        anchor,
        placeholder: widget.placeholder.clone(),
        prefix: widget.prefix.clone(),
        prefix_style: widget.prefix_style,
        focus_prefix_style: widget.focus_prefix_style,
        suffix: widget.suffix.clone(),
        suffix_style: widget.suffix_style,
        focus_suffix_style: widget.focus_suffix_style,
        truncate_head: widget.truncate_head,
        style: widget.style,
        hover_style: widget.hover_style,
        focus_style: widget.focus_style,
        focus_content_style: widget.focus_content_style,
        hover_border_style: widget.hover_border_style,
        placeholder_style: widget.placeholder_style,
        focus_placeholder_style: widget.focus_placeholder_style,
        caret_shape: widget.caret_shape,
        caret_color: widget.caret_color,
        selection_style: widget.selection_style,
        border: widget.border,
        border_style: widget.border_style,
        padding: widget.padding,
        mask: widget.mask,
        disabled: widget.disabled,
        disabled_style: widget.disabled_style,
        read_only: widget.read_only,
        error: widget.error.clone(),
        error_style: widget.error_style,
        reserve_error_row: widget.reserve_error_row,
        on_change: widget.on_change.clone(),
        on_edit: widget.on_edit.clone(),
        on_click: widget.on_click.clone(),
        on_key: widget.on_key.clone(),
        key_interceptor: widget.key_interceptor.clone(),
        focusable: widget.focusable,
        tab_order: widget.tab_order,
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::Input;

    use super::reconcile_input;

    #[test]
    fn reconcile_clamps_cursor_and_anchor_inside_unicode_characters() {
        let value = "części ewaluacyjnej";
        let cursor_inside_s = 5;
        assert!(!value.is_char_boundary(cursor_inside_s));

        let input = Input::new(value)
            .cursor(cursor_inside_s)
            .anchor(Some(cursor_inside_s));
        let node = reconcile_input(&input);

        assert_eq!(node.cursor, 4);
        assert_eq!(node.anchor, Some(4));
        assert!(value.is_char_boundary(node.cursor));
    }
}
