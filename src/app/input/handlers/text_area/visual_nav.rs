use super::*;

pub(super) fn perform_visual_vertical_nav(
    editor: &mut TextEditor,
    action: Action,
    lines: &[TextAreaVisualLine],
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
    virtual_texts: &[TextAreaVirtualText],
) -> bool {
    if lines.is_empty() {
        return false;
    }

    let value = editor.text().to_string();
    let cursor = clamp_text_area_cursor(&value, editor.cursor());

    let cur_idx = visual_line_for_cursor(lines, cursor);
    let cur_line = &lines[cur_idx];

    let cur_col = editor.visual_nav_col().unwrap_or_else(|| {
        let safe_end = clamp_text_area_cursor(&value, cursor.min(cur_line.end));
        let safe_start = clamp_text_area_cursor(&value, cur_line.start.min(safe_end));
        let (logical_line_start, logical_line_end) = logical_line_bounds(&value, safe_start);
        let insertions = inline_virtual_insertions_for_line(
            &value,
            virtual_texts,
            logical_line_start,
            logical_line_end,
        );
        visual_col_with_virtual(
            &value[logical_line_start..safe_end],
            0,
            tab_stop,
            sentinel,
            &insertions,
        )
        .saturating_sub(cur_line.visual_start_col)
    });

    let target_idx = match action {
        Action::MoveUp | Action::SelectUp => {
            if cur_idx == 0 {
                return perform_visual_boundary_nav(editor, action, 0);
            }
            cur_idx - 1
        }
        Action::MoveDown | Action::SelectDown => {
            if cur_idx + 1 >= lines.len() {
                return perform_visual_boundary_nav(editor, action, value.len());
            }
            cur_idx + 1
        }
        _ => return false,
    };

    let target = &lines[target_idx];
    let target_str_end = clamp_text_area_cursor(&value, target.end.min(value.len()));
    let target_str_start = clamp_text_area_cursor(&value, target.start.min(target_str_end));
    // A soft-wrapped row shares its end byte with the next continuation row's
    // start. Landing on that boundary would skip past the target row.
    let target_ends_at_soft_wrap = lines.get(target_idx + 1).is_some_and(|next| {
        next.line_num == target.line_num
            && next.continuation
            && clamp_text_area_cursor(&value, next.start) == target_str_end
    });
    let (logical_line_start, logical_line_end) = logical_line_bounds(&value, target_str_start);
    let insertions = inline_virtual_insertions_for_line(
        &value,
        virtual_texts,
        logical_line_start,
        logical_line_end,
    );
    let target_col = target
        .visual_start_col
        .saturating_add(cur_col)
        .min(target.visual_end_col);
    let offset = crate::utils::text::byte_at_col_sentinel_tabs_virtual(
        &value[logical_line_start..logical_line_end],
        target_col,
        sentinel,
        tab_stop,
        &insertions,
    );
    let mut new_cursor =
        clamp_text_area_cursor(&value, (logical_line_start + offset).min(target_str_end));
    // Keep the cursor on the target row: when the projected column reaches the
    // shared soft-wrap boundary, step back one char so it stays on this row
    // instead of falling through to the next continuation row's first column.
    if target_ends_at_soft_wrap && new_cursor >= target_str_end {
        new_cursor = clamp_text_area_cursor(
            &value,
            crate::utils::text::prev_char_boundary(&value, target_str_end).max(target_str_start),
        );
    }

    let select = matches!(action, Action::SelectUp | Action::SelectDown);
    let prev_anchor = editor.anchor();

    if select {
        if editor.anchor().is_none() {
            editor.set_anchor(Some(cursor));
        }
        editor.set_cursor_keep_anchor(new_cursor);
        if editor.anchor() == Some(editor.cursor()) {
            editor.set_anchor(None);
        }
    } else {
        editor.set_cursor(new_cursor);
    }

    editor.set_visual_nav_col(Some(cur_col));

    new_cursor != cursor || editor.anchor() != prev_anchor
}

fn clamp_text_area_cursor(value: &str, cursor: usize) -> usize {
    crate::utils::text::clamp_cursor(value, cursor.min(value.len()))
}

fn logical_line_bounds(value: &str, cursor: usize) -> (usize, usize) {
    let cursor = clamp_text_area_cursor(value, cursor);
    let start = value[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let end = value[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(value.len());
    (start, end)
}

pub(super) fn perform_visual_boundary_nav(
    editor: &mut TextEditor,
    action: Action,
    new_cursor: usize,
) -> bool {
    let new_cursor = clamp_text_area_cursor(editor.text(), new_cursor);
    let cursor = clamp_text_area_cursor(editor.text(), editor.cursor());
    let prev_anchor = editor.anchor();

    if matches!(action, Action::SelectUp | Action::SelectDown) {
        if editor.anchor().is_none() {
            editor.set_anchor(Some(cursor));
        }
        editor.set_cursor_keep_anchor(new_cursor);
        if editor.anchor() == Some(editor.cursor()) {
            editor.set_anchor(None);
        }
    } else {
        editor.set_cursor(new_cursor);
    }

    editor.set_visual_nav_col(None);
    new_cursor != cursor || editor.anchor() != prev_anchor
}

/// Return the visual line index containing the cursor.
pub(super) fn visual_line_for_cursor(lines: &[TextAreaVisualLine], cursor: usize) -> usize {
    text_area_visual_line_for_cursor(lines, cursor)
}
