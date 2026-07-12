use super::*;

pub(super) fn clear_text_area_vim_render_feedback(tree: &mut NodeTree, id: NodeId) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.vim_search_feedback = None;
        ta.vim_mode = TextAreaVimMode::Normal;
        ta.vim_visual_line_caret = None;
        ta.vim_yank_feedback_range = None;
    }
}

pub(super) fn sync_text_area_vim_render_feedback(
    tree: &mut NodeTree,
    id: NodeId,
    state: &TextAreaVimState,
    editor: &TextEditor,
) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.vim_search_feedback =
            text_area_vim_search_feedback_for_text(state, editor.text(), editor.cursor());
        ta.vim_mode = state.mode;
        ta.vim_visual_line_caret = state.visual_line_caret;
        ta.vim_yank_feedback_range = state.yank_feedback_range;
    }
}

pub(super) fn remap_vim_marks_for_last_edit(state: &mut TextAreaVimState, editor: &TextEditor) {
    let Some(edit) = editor.core.last_edit.clone() else {
        return;
    };
    let inserted_len = edit.inserted.len();
    let deleted_len = edit.deleted.len();
    remap_vim_marks_range(state, edit.start, deleted_len, inserted_len);
}

pub(super) fn remap_vim_marks_range(
    state: &mut TextAreaVimState,
    start: usize,
    deleted_len: usize,
    inserted_len: usize,
) {
    let end = start.saturating_add(deleted_len);
    let remap = |offset: usize| remap_offset_after_edit(offset, start, end, inserted_len);
    for mark in state.marks.values_mut() {
        *mark = remap(*mark);
    }
    if let Some(previous_jump) = &mut state.previous_jump {
        *previous_jump = remap(*previous_jump);
    }
}

pub(super) fn remap_offset_after_edit(
    offset: usize,
    start: usize,
    end: usize,
    inserted_len: usize,
) -> usize {
    if offset < start {
        return offset;
    }
    if offset <= end {
        return start.saturating_add(inserted_len);
    }
    start
        .saturating_add(inserted_len)
        .saturating_add(offset.saturating_sub(end))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VimKeyOutcome {
    Unhandled,
    PassThrough,
    ConsumedUnchanged,
    ModeChanged(TextAreaVimMode),
    EditorChanged {
        vertical: bool,
        mode_changed: Option<TextAreaVimMode>,
    },
}

pub(super) struct VimLayoutCtx<'a> {
    pub wrap: bool,
    pub visual_lines: Option<&'a [TextAreaVisualLine]>,
    pub sentinel: Option<&'a SentinelInfo>,
    pub tab_stop: usize,
    pub virtual_texts: &'a [TextAreaVirtualText],
}

pub(super) struct VimClipboardCtx<'a> {
    pub params: TextAreaClipboardParams<'a>,
    pub clipboard: &'a crate::clipboard::ClipboardService,
    pub config: &'a crate::clipboard::ClipboardConfig,
}

pub(super) struct VimOperatorTargetArgs {
    pub op: VimOperator,
    pub target: VimRepeatTarget,
    pub repeat_count: usize,
    pub register: Option<char>,
    pub record_repeat: bool,
}

pub(super) fn dispatch_text_area_vim_key(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    match state.mode {
        TextAreaVimMode::Insert => dispatch_text_area_vim_insert_key(editor, state, key, action),
        TextAreaVimMode::Normal => {
            dispatch_text_area_vim_normal_key(editor, state, key, action, layout)
        }
        TextAreaVimMode::Visual => {
            dispatch_text_area_vim_visual_key(editor, state, key, action, layout)
        }
        TextAreaVimMode::VisualLine => dispatch_text_area_vim_visual_line_key(
            editor,
            state,
            key,
            action,
            layout.sentinel,
            layout.tab_stop,
        ),
    }
}

pub(super) fn dispatch_text_area_vim_insert_key(
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
) -> VimKeyOutcome {
    if key.is(KeyCode::Esc) {
        finalize_vim_insert_session(editor, state);
        return if state.set_mode(TextAreaVimMode::Normal) {
            VimKeyOutcome::ModeChanged(TextAreaVimMode::Normal)
        } else {
            VimKeyOutcome::ConsumedUnchanged
        };
    }

    if matches!(action, Action::Undo | Action::Redo) {
        return VimKeyOutcome::ConsumedUnchanged;
    }

    VimKeyOutcome::PassThrough
}

pub(super) fn dispatch_text_area_vim_normal_key(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    if key.is(KeyCode::Esc) {
        if state.search.visible || state.count.is_some() || state.pending.is_some() {
            state.search.visible = false;
            state.clear_count_pending();
            return VimKeyOutcome::ConsumedUnchanged;
        }
        return VimKeyOutcome::Unhandled;
    }

    if is_plainish_char(key, '0') && state.count.is_some() {
        state.push_count_digit(0);
        return VimKeyOutcome::ConsumedUnchanged;
    }
    if let Some(digit) = plainish_digit_1_to_9(key) {
        state.push_count_digit(digit);
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if state.pending == Some(TextAreaVimPending::G) {
        state.pending = None;
        if is_plainish_char(key, 'g') {
            let count = take_vim_count(state);
            let target = count.map_or(0, |line| line_start_by_one_based_count(editor.text(), line));
            return vim_set_cursor(editor, target, false);
        }
        state.count = None;
        return VimKeyOutcome::ConsumedUnchanged;
    }

    match action {
        Action::FocusNext
        | Action::FocusPrev
        | Action::Quit
        | Action::DismissOverlay
        | Action::ToggleDevTools
        | Action::InsertNewline => return VimKeyOutcome::Unhandled,
        Action::MoveLeft => {
            return vim_repeat_motion(editor, state, false, |editor| editor.move_left());
        }
        Action::MoveRight => {
            return vim_repeat_motion(editor, state, false, |editor| editor.move_right());
        }
        Action::MoveUp => {
            return vim_repeat_vertical_motion(editor, state, Action::MoveUp, layout);
        }
        Action::MoveDown => {
            return vim_repeat_vertical_motion(editor, state, Action::MoveDown, layout);
        }
        Action::MoveHome => return vim_line_start(editor, state),
        Action::MoveEnd => return vim_line_end(editor, state, false),
        Action::MoveWordLeft => return vim_word_backward(editor, state),
        Action::MoveWordRight => return vim_word_forward(editor, state),
        _ => {}
    }

    if is_plainish_char(key, 'h') {
        vim_repeat_motion(editor, state, false, |editor| editor.move_left())
    } else if is_plainish_char(key, 'l') {
        vim_repeat_motion(editor, state, false, |editor| editor.move_right())
    } else if is_plainish_char(key, 'j') {
        vim_repeat_vertical_motion(editor, state, Action::MoveDown, layout)
    } else if is_plainish_char(key, 'k') {
        vim_repeat_vertical_motion(editor, state, Action::MoveUp, layout)
    } else if is_plainish_char(key, 'w') {
        vim_word_forward(editor, state)
    } else if is_plainish_char(key, 'b') {
        vim_word_backward(editor, state)
    } else if is_plainish_char(key, 'e') {
        vim_word_end_motion(editor, state)
    } else if is_plainish_char(key, 'W') {
        vim_big_word_forward(editor, state)
    } else if is_plainish_char(key, 'B') {
        vim_big_word_backward(editor, state)
    } else if is_plainish_char(key, 'E') {
        vim_big_word_end_motion(editor, state)
    } else if is_plainish_char(key, '0') {
        vim_line_start(editor, state)
    } else if is_plainish_char(key, '$') {
        vim_line_end(editor, state, true)
    } else if is_plainish_char(key, 'g') {
        state.pending = Some(TextAreaVimPending::G);
        VimKeyOutcome::ConsumedUnchanged
    } else if is_plainish_char(key, 'G') {
        vim_goto_line(editor, state)
    } else if is_plainish_char(key, 'v') {
        vim_enter_visual(editor, state)
    } else if is_plainish_char(key, 'V') {
        vim_enter_visual_line(editor, state, layout.sentinel, layout.tab_stop)
    } else if is_plainish_char(key, 'i') {
        vim_enter_insert(editor, state, VimInsertKind::Insert)
    } else if is_plainish_char(key, 'a') {
        let changed = editor.move_right();
        editor.set_visual_nav_col(None);
        vim_enter_insert_after_cursor_move(editor, state, changed, VimInsertKind::Append)
    } else if is_plainish_char(key, 'I') {
        let bounds = line_bounds_at(editor.text(), editor.cursor());
        let target = first_nonblank_in_line(editor.text(), bounds.start, bounds.end);
        let changed = vim_set_cursor_raw(editor, target);
        editor.set_visual_nav_col(None);
        vim_enter_insert_after_cursor_move(editor, state, changed, VimInsertKind::InsertLineStart)
    } else if is_plainish_char(key, 'A') {
        let bounds = line_bounds_at(editor.text(), editor.cursor());
        let changed = vim_set_cursor_raw(editor, bounds.end);
        editor.set_visual_nav_col(None);
        vim_enter_insert_after_cursor_move(editor, state, changed, VimInsertKind::AppendLineEnd)
    } else if vim_unhandled_key_should_bubble(key, action) {
        VimKeyOutcome::Unhandled
    } else {
        state.clear_count_pending();
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn dispatch_text_area_vim_visual_key(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    if key.is(KeyCode::Esc) || is_plainish_char(key, 'v') || is_plainish_char(key, 'V') {
        return vim_exit_visual(editor, state);
    }

    if is_plainish_char(key, '0') && state.count.is_some() {
        state.push_count_digit(0);
        return VimKeyOutcome::ConsumedUnchanged;
    }
    if let Some(digit) = plainish_digit_1_to_9(key) {
        state.push_count_digit(digit);
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if state.pending == Some(TextAreaVimPending::G) {
        state.pending = None;
        if is_plainish_char(key, 'g') {
            let count = take_vim_count(state);
            let target = count.map_or(0, |line| line_start_by_one_based_count(editor.text(), line));
            return vim_visual_set_cursor(editor, state, target, false);
        }
        state.count = None;
        return VimKeyOutcome::ConsumedUnchanged;
    }

    match action {
        Action::FocusNext
        | Action::FocusPrev
        | Action::Quit
        | Action::DismissOverlay
        | Action::ToggleDevTools
        | Action::InsertNewline => return VimKeyOutcome::Unhandled,
        Action::MoveLeft | Action::SelectLeft => {
            return vim_visual_repeat_motion(editor, state, false, |editor| {
                prev_char_boundary(editor.text(), editor.cursor())
            });
        }
        Action::MoveRight | Action::SelectRight => {
            return vim_visual_repeat_motion(editor, state, false, |editor| {
                next_char_boundary(editor.text(), editor.cursor())
            });
        }
        Action::MoveUp | Action::SelectUp => {
            return vim_visual_repeat_vertical_motion(editor, state, Action::SelectUp, layout);
        }
        Action::MoveDown | Action::SelectDown => {
            return vim_visual_repeat_vertical_motion(editor, state, Action::SelectDown, layout);
        }
        Action::MoveHome | Action::SelectHome => return vim_visual_line_start(editor, state),
        Action::MoveEnd | Action::SelectEnd => {
            return vim_visual_line_end(editor, state, false, layout);
        }
        Action::MoveWordLeft | Action::SelectWordLeft => {
            return vim_visual_word_backward(editor, state);
        }
        Action::MoveWordRight | Action::SelectWordRight => {
            return vim_visual_word_forward(editor, state);
        }
        _ => {}
    }

    if is_plainish_char(key, 'h') {
        vim_visual_repeat_motion(editor, state, false, |editor| {
            prev_char_boundary(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'l') {
        vim_visual_repeat_motion(editor, state, false, |editor| {
            next_char_boundary(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'j') {
        vim_visual_repeat_vertical_motion(editor, state, Action::SelectDown, layout)
    } else if is_plainish_char(key, 'k') {
        vim_visual_repeat_vertical_motion(editor, state, Action::SelectUp, layout)
    } else if is_plainish_char(key, 'w') {
        vim_visual_word_forward(editor, state)
    } else if is_plainish_char(key, 'b') {
        vim_visual_word_backward(editor, state)
    } else if is_plainish_char(key, 'e') {
        vim_visual_word_end_motion(editor, state)
    } else if is_plainish_char(key, 'W') {
        vim_visual_big_word_forward(editor, state)
    } else if is_plainish_char(key, 'B') {
        vim_visual_big_word_backward(editor, state)
    } else if is_plainish_char(key, 'E') {
        vim_visual_big_word_end_motion(editor, state)
    } else if is_plainish_char(key, '0') {
        vim_visual_line_start(editor, state)
    } else if is_plainish_char(key, '$') {
        vim_visual_line_end(editor, state, true, layout)
    } else if is_plainish_char(key, 'g') {
        state.pending = Some(TextAreaVimPending::G);
        VimKeyOutcome::ConsumedUnchanged
    } else if is_plainish_char(key, 'G') {
        vim_visual_goto_line(editor, state)
    } else if vim_unhandled_key_should_bubble(key, action) {
        VimKeyOutcome::Unhandled
    } else {
        state.clear_count_pending();
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn dispatch_text_area_vim_visual_line_key(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> VimKeyOutcome {
    if key.is(KeyCode::Esc) || is_plainish_char(key, 'v') || is_plainish_char(key, 'V') {
        return vim_exit_visual(editor, state);
    }

    if is_plainish_char(key, '0') && state.count.is_some() {
        state.push_count_digit(0);
        return VimKeyOutcome::ConsumedUnchanged;
    }
    if let Some(digit) = plainish_digit_1_to_9(key) {
        state.push_count_digit(digit);
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if state.pending == Some(TextAreaVimPending::G) {
        state.pending = None;
        if is_plainish_char(key, 'g') {
            let target = take_vim_count(state)
                .map_or(0, |line| line_start_by_one_based_count(editor.text(), line));
            return vim_visual_line_set_head(editor, state, target, true, sentinel, tab_stop);
        }
        state.count = None;
        return VimKeyOutcome::ConsumedUnchanged;
    }

    match action {
        Action::FocusNext
        | Action::FocusPrev
        | Action::Quit
        | Action::DismissOverlay
        | Action::ToggleDevTools
        | Action::InsertNewline => return VimKeyOutcome::Unhandled,
        Action::MoveLeft | Action::SelectLeft => {
            return vim_visual_line_repeat_target(
                editor,
                state,
                false,
                sentinel,
                tab_stop,
                |editor| prev_char_boundary(editor.text(), editor.cursor()),
            );
        }
        Action::MoveRight | Action::SelectRight => {
            return vim_visual_line_repeat_target(
                editor,
                state,
                false,
                sentinel,
                tab_stop,
                |editor| next_char_boundary(editor.text(), editor.cursor()),
            );
        }
        Action::MoveUp | Action::SelectUp => {
            return vim_visual_line_repeat_logical_lines(editor, state, false, sentinel, tab_stop);
        }
        Action::MoveDown | Action::SelectDown => {
            return vim_visual_line_repeat_logical_lines(editor, state, true, sentinel, tab_stop);
        }
        Action::MoveHome | Action::SelectHome => {
            take_vim_count(state);
            let target = line_start_at(editor.text(), editor.cursor());
            return vim_visual_line_set_head(editor, state, target, false, sentinel, tab_stop);
        }
        Action::MoveEnd | Action::SelectEnd => {
            take_vim_count(state);
            let target = line_end_including_newline(editor.text(), editor.cursor());
            return vim_visual_line_set_head(editor, state, target, false, sentinel, tab_stop);
        }
        Action::MoveWordLeft | Action::SelectWordLeft => {
            return vim_visual_line_repeat_target(
                editor,
                state,
                false,
                sentinel,
                tab_stop,
                |editor| vim_word_backward_start(editor.text(), editor.cursor()),
            );
        }
        Action::MoveWordRight | Action::SelectWordRight => {
            return vim_visual_line_repeat_target(
                editor,
                state,
                false,
                sentinel,
                tab_stop,
                |editor| vim_word_forward_start(editor.text(), editor.cursor()),
            );
        }
        _ => {}
    }

    if is_plainish_char(key, 'h') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            prev_char_boundary(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'l') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            next_char_boundary(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'j') {
        vim_visual_line_repeat_logical_lines(editor, state, true, sentinel, tab_stop)
    } else if is_plainish_char(key, 'k') {
        vim_visual_line_repeat_logical_lines(editor, state, false, sentinel, tab_stop)
    } else if is_plainish_char(key, 'w') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_word_forward_start(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'b') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_word_backward_start(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'e') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_word_end(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'W') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_big_word_forward_start(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'B') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_big_word_backward_start(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, 'E') {
        vim_visual_line_repeat_target(editor, state, false, sentinel, tab_stop, |editor| {
            vim_big_word_end(editor.text(), editor.cursor())
        })
    } else if is_plainish_char(key, '0') {
        take_vim_count(state);
        let target = line_start_at(editor.text(), editor.cursor());
        vim_visual_line_set_head(editor, state, target, false, sentinel, tab_stop)
    } else if is_plainish_char(key, '$') {
        let count = vim_repeat_count(state);
        if count > 1 {
            let _ = vim_visual_line_move_logical_lines(
                editor,
                state,
                true,
                count - 1,
                sentinel,
                tab_stop,
            );
        }
        let target = line_end_including_newline(editor.text(), editor.cursor());
        vim_visual_line_set_head(editor, state, target, false, sentinel, tab_stop)
    } else if is_plainish_char(key, 'g') {
        state.pending = Some(TextAreaVimPending::G);
        VimKeyOutcome::ConsumedUnchanged
    } else if is_plainish_char(key, 'G') {
        let target = take_vim_count(state).map_or_else(
            || line_start_by_one_based_count(editor.text(), usize::MAX),
            |line| line_start_by_one_based_count(editor.text(), line),
        );
        vim_visual_line_set_head(editor, state, target, true, sentinel, tab_stop)
    } else if vim_unhandled_key_should_bubble(key, action) {
        VimKeyOutcome::Unhandled
    } else {
        state.clear_count_pending();
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn take_vim_count(state: &mut TextAreaVimState) -> Option<usize> {
    let count = state.count.take();
    state.pending = None;
    count
}

pub(super) fn vim_repeat_count(state: &mut TextAreaVimState) -> usize {
    take_vim_count(state).unwrap_or(1).max(1)
}

pub(super) fn vim_repeat_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    vertical: bool,
    mut f: impl FnMut(&mut TextEditor) -> bool,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        changed |= f(editor);
    }
    if changed {
        VimKeyOutcome::EditorChanged {
            vertical,
            mode_changed: None,
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_repeat_vertical_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    action: Action,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let moved = if layout.wrap {
            layout
                .visual_lines
                .map(|lines| {
                    perform_visual_vertical_nav(
                        editor,
                        action,
                        lines,
                        layout.sentinel,
                        layout.tab_stop,
                        layout.virtual_texts,
                    )
                })
                .unwrap_or(false)
        } else {
            false
        };
        changed |= if moved {
            true
        } else if matches!(action, Action::MoveDown) {
            editor.move_down()
        } else {
            editor.move_up()
        };
    }
    if changed {
        VimKeyOutcome::EditorChanged {
            vertical: true,
            mode_changed: None,
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_word_forward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_word_forward_start(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_word_backward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_word_backward_start(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_word_end_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_word_end(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_big_word_forward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_big_word_forward_start(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_big_word_backward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_big_word_backward_start(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_big_word_end_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let target = vim_big_word_end(editor.text(), editor.cursor());
        changed |= vim_set_cursor_raw(editor, target);
    }
    vim_motion_outcome(changed, false)
}

pub(super) fn vim_line_start(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    take_vim_count(state);
    let bounds = line_bounds_at(editor.text(), editor.cursor());
    vim_set_cursor(editor, bounds.start, false)
}

pub(super) fn vim_line_end(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    counted: bool,
) -> VimKeyOutcome {
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    let count = if counted {
        vim_repeat_count(state)
    } else {
        take_vim_count(state).unwrap_or(1)
    };
    if counted && count > 1 {
        for _ in 1..count {
            let _ = editor.move_down();
        }
    }
    let bounds = line_bounds_at(editor.text(), editor.cursor());
    editor.set_cursor(bounds.end);
    vim_motion_outcome(
        editor.cursor() != old_cursor || editor.anchor() != old_anchor,
        false,
    )
}

pub(super) fn vim_goto_line(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let target = take_vim_count(state).map_or_else(
        || line_start_by_one_based_count(editor.text(), usize::MAX),
        |line| line_start_by_one_based_count(editor.text(), line),
    );
    vim_set_cursor(editor, target, false)
}

pub(super) fn vim_enter_visual(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    let anchor = editor.cursor();
    state.visual_anchor = Some(anchor);
    let changed_mode = state.set_mode(TextAreaVimMode::Visual);
    state.visual_anchor = Some(anchor);
    editor.set_anchor(Some(anchor));
    if editor.cursor() != old_cursor || editor.anchor() != old_anchor {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: changed_mode.then_some(TextAreaVimMode::Visual),
        }
    } else if changed_mode {
        VimKeyOutcome::ModeChanged(TextAreaVimMode::Visual)
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_enter_visual_line(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> VimKeyOutcome {
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    let line_start = line_start_at(editor.text(), editor.cursor());
    let preferred_col =
        vim_visual_line_col_at_cursor(editor.text(), editor.cursor(), sentinel, tab_stop);
    state.visual_anchor = Some(line_start);
    state.visual_line_head = Some(line_start);
    state.visual_line_preferred_col = Some(preferred_col);
    let changed_mode = state.set_mode(TextAreaVimMode::VisualLine);
    state.visual_anchor = Some(line_start);
    state.visual_line_head = Some(line_start);
    state.visual_line_preferred_col = Some(preferred_col);
    select_visual_line_range(editor, line_start, line_start);
    sync_visual_line_caret(editor, state, line_start, sentinel, tab_stop);
    let changed_editor = editor.cursor() != old_cursor || editor.anchor() != old_anchor;
    if changed_editor {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: changed_mode.then_some(TextAreaVimMode::VisualLine),
        }
    } else if changed_mode {
        VimKeyOutcome::ModeChanged(TextAreaVimMode::VisualLine)
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_exit_visual(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    let changed_mode = state.set_mode(TextAreaVimMode::Normal);
    let cursor = editor.cursor();
    editor.set_cursor(cursor);
    let changed_editor = editor.cursor() != old_cursor || editor.anchor() != old_anchor;
    if changed_editor {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: changed_mode.then_some(TextAreaVimMode::Normal),
        }
    } else if changed_mode {
        VimKeyOutcome::ModeChanged(TextAreaVimMode::Normal)
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn ensure_visual_anchor(editor: &mut TextEditor, state: &mut TextAreaVimState) -> usize {
    let anchor = state.visual_anchor.unwrap_or_else(|| {
        let anchor = editor.anchor().unwrap_or(editor.cursor());
        state.visual_anchor = Some(anchor);
        anchor
    });
    editor.set_anchor(Some(anchor));
    anchor
}

pub(super) fn ensure_visual_line_anchor(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> usize {
    let anchor = state.visual_anchor.unwrap_or_else(|| {
        let anchor = line_start_at(editor.text(), editor.anchor().unwrap_or(editor.cursor()));
        state.visual_anchor = Some(anchor);
        anchor
    });
    let head = *state
        .visual_line_head
        .get_or_insert_with(|| line_start_at(editor.text(), editor.cursor()));
    state.visual_line_preferred_col.get_or_insert_with(|| {
        vim_visual_line_col_at_cursor(editor.text(), editor.cursor(), sentinel, tab_stop)
    });
    sync_visual_line_caret(editor, state, head, sentinel, tab_stop);
    anchor
}

pub(super) fn vim_visual_line_col_at_cursor(
    text: &str,
    cursor: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> usize {
    let bounds = line_bounds_at(text, cursor);
    let cursor = cursor.min(bounds.end);
    str_visual_width_with_tabs(&text[bounds.start..cursor], sentinel, 0, tab_stop)
}

pub(super) fn vim_visual_line_caret_at_col(
    text: &str,
    line_start: usize,
    col: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> usize {
    let line_start = line_start_at(text, line_start);
    let line_end = line_end_at(text, line_start);
    line_start.saturating_add(byte_at_col_sentinel_tabs(
        &text[line_start..line_end],
        col,
        sentinel,
        tab_stop,
    ))
}

pub(super) fn sync_visual_line_caret(
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    head_line: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) {
    let col = state.visual_line_preferred_col.unwrap_or_else(|| {
        vim_visual_line_col_at_cursor(editor.text(), editor.cursor(), sentinel, tab_stop)
    });
    state.visual_line_preferred_col = Some(col);
    state.visual_line_caret = Some(vim_visual_line_caret_at_col(
        editor.text(),
        head_line,
        col,
        sentinel,
        tab_stop,
    ));
}

pub(super) fn select_visual_line_range(
    editor: &mut TextEditor,
    origin_line: usize,
    head_line: usize,
) {
    let origin_line = line_start_at(editor.text(), origin_line);
    let head_line = line_start_at(editor.text(), head_line);
    if head_line >= origin_line {
        let cursor = line_end_including_newline(editor.text(), head_line);
        editor.set_anchor(Some(origin_line));
        editor.set_cursor_keep_anchor(cursor);
    } else {
        let anchor = line_end_including_newline(editor.text(), origin_line);
        editor.set_anchor(Some(anchor));
        editor.set_cursor_keep_anchor(head_line);
    }
}

pub(super) fn vim_visual_line_set_head(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    target: usize,
    vertical: bool,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> VimKeyOutcome {
    let origin = ensure_visual_line_anchor(editor, state, sentinel, tab_stop);
    let head = line_start_at(editor.text(), target);
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    state.visual_line_head = Some(head);
    select_visual_line_range(editor, origin, head);
    sync_visual_line_caret(editor, state, head, sentinel, tab_stop);
    vim_motion_outcome(
        editor.cursor() != old_cursor || editor.anchor() != old_anchor,
        vertical,
    )
}

pub(super) fn vim_visual_line_repeat_target(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    vertical: bool,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
    mut target: impl FnMut(&TextEditor) -> usize,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        let next = target(editor);
        let outcome = vim_visual_line_set_head(editor, state, next, vertical, sentinel, tab_stop);
        changed |= matches!(outcome, VimKeyOutcome::EditorChanged { .. });
    }
    vim_motion_outcome(changed, vertical)
}

pub(super) fn vim_visual_line_repeat_logical_lines(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    down: bool,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    vim_visual_line_move_logical_lines(editor, state, down, count, sentinel, tab_stop)
}

pub(super) fn vim_visual_line_move_logical_lines(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    down: bool,
    count: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> VimKeyOutcome {
    ensure_visual_line_anchor(editor, state, sentinel, tab_stop);
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    let head = state
        .visual_line_head
        .unwrap_or_else(|| line_start_at(editor.text(), editor.cursor()));
    let current = line_index_at(editor.text(), head);
    let last = line_count(editor.text()).saturating_sub(1);
    let target_line = if down {
        current.saturating_add(count).min(last)
    } else {
        current.saturating_sub(count)
    };
    let target = line_start_by_index(editor.text(), target_line);
    state.visual_line_head = Some(target);
    let origin = state.visual_anchor.unwrap_or(target);
    select_visual_line_range(editor, origin, target);
    sync_visual_line_caret(editor, state, target, sentinel, tab_stop);
    vim_motion_outcome(
        editor.cursor() != old_cursor || editor.anchor() != old_anchor,
        true,
    )
}

pub(super) fn vim_visual_set_cursor(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    target: usize,
    vertical: bool,
) -> VimKeyOutcome {
    ensure_visual_anchor(editor, state);
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    editor.set_cursor_keep_anchor(target);
    vim_motion_outcome(
        editor.cursor() != old_cursor || editor.anchor() != old_anchor,
        vertical,
    )
}

pub(super) fn vim_visual_repeat_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    vertical: bool,
    mut target: impl FnMut(&TextEditor) -> usize,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        ensure_visual_anchor(editor, state);
        let next = target(editor);
        let old_cursor = editor.cursor();
        let old_anchor = editor.anchor();
        editor.set_cursor_keep_anchor(next);
        changed |= editor.cursor() != old_cursor || editor.anchor() != old_anchor;
    }
    vim_motion_outcome(changed, vertical)
}

pub(super) fn vim_visual_repeat_vertical_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    action: Action,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    let count = vim_repeat_count(state);
    let mut changed = false;
    for _ in 0..count {
        ensure_visual_anchor(editor, state);
        let anchor = state.visual_anchor;
        let moved = if layout.wrap {
            layout
                .visual_lines
                .map(|lines| {
                    perform_visual_vertical_nav(
                        editor,
                        action,
                        lines,
                        layout.sentinel,
                        layout.tab_stop,
                        layout.virtual_texts,
                    )
                })
                .unwrap_or(false)
        } else {
            false
        };
        state.visual_anchor = anchor;
        if let Some(anchor) = anchor {
            editor.set_anchor(Some(anchor));
        }
        changed |= if moved {
            true
        } else {
            let mut probe = editor.clone();
            probe.set_anchor(None);
            let moved = if matches!(action, Action::SelectDown | Action::MoveDown) {
                probe.move_down()
            } else {
                probe.move_up()
            };
            if moved {
                let old_cursor = editor.cursor();
                let old_anchor = editor.anchor();
                editor.set_cursor_keep_anchor(probe.cursor());
                editor.cursor() != old_cursor || editor.anchor() != old_anchor
            } else {
                false
            }
        };
    }
    vim_motion_outcome(changed, true)
}

pub(super) fn vim_visual_word_forward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_word_forward_start(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_word_backward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_word_backward_start(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_word_end_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_word_end(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_big_word_forward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_big_word_forward_start(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_big_word_backward(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_big_word_backward_start(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_big_word_end_motion(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    vim_visual_repeat_motion(editor, state, false, |editor| {
        vim_big_word_end(editor.text(), editor.cursor())
    })
}

pub(super) fn vim_visual_line_start(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    take_vim_count(state);
    let bounds = line_bounds_at(editor.text(), editor.cursor());
    vim_visual_set_cursor(editor, state, bounds.start, false)
}

pub(super) fn vim_visual_line_end(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    counted: bool,
    layout: &VimLayoutCtx<'_>,
) -> VimKeyOutcome {
    let count = if counted {
        vim_repeat_count(state)
    } else {
        take_vim_count(state).unwrap_or(1)
    };
    if counted && count > 1 {
        for _ in 1..count {
            let _ = vim_visual_repeat_vertical_motion(editor, state, Action::SelectDown, layout);
        }
    }
    let bounds = line_bounds_at(editor.text(), editor.cursor());
    vim_visual_set_cursor(editor, state, bounds.end, false)
}

pub(super) fn vim_visual_goto_line(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
) -> VimKeyOutcome {
    let target = take_vim_count(state).map_or_else(
        || line_start_by_one_based_count(editor.text(), usize::MAX),
        |line| line_start_by_one_based_count(editor.text(), line),
    );
    vim_visual_set_cursor(editor, state, target, false)
}

pub(super) fn vim_enter_insert(
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    kind: VimInsertKind,
) -> VimKeyOutcome {
    begin_vim_insert_session(editor, state, VimInsertOrigin::Insert { kind });
    if state.set_mode(TextAreaVimMode::Insert) {
        VimKeyOutcome::ModeChanged(TextAreaVimMode::Insert)
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_enter_insert_after_cursor_move(
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    cursor_changed: bool,
    kind: VimInsertKind,
) -> VimKeyOutcome {
    begin_vim_insert_session(editor, state, VimInsertOrigin::Insert { kind });
    let changed_mode = state.set_mode(TextAreaVimMode::Insert);
    if cursor_changed {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: changed_mode.then_some(TextAreaVimMode::Insert),
        }
    } else if changed_mode {
        VimKeyOutcome::ModeChanged(TextAreaVimMode::Insert)
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_set_cursor(
    editor: &mut TextEditor,
    target: usize,
    vertical: bool,
) -> VimKeyOutcome {
    vim_motion_outcome(vim_set_cursor_raw(editor, target), vertical)
}

pub(super) fn vim_motion_outcome(changed: bool, vertical: bool) -> VimKeyOutcome {
    if changed {
        VimKeyOutcome::EditorChanged {
            vertical,
            mode_changed: None,
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_set_cursor_raw(editor: &mut TextEditor, target: usize) -> bool {
    let old_cursor = editor.cursor();
    let old_anchor = editor.anchor();
    editor.set_cursor(target);
    editor.cursor() != old_cursor || editor.anchor() != old_anchor
}

pub(super) fn plainish_digit_1_to_9(key: KeyEvent) -> Option<u8> {
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    if key.mods.ctrl || key.mods.alt || key.mods.super_key {
        return None;
    }
    ch.to_digit(10)
        .filter(|digit| (1..=9).contains(digit))
        .map(|digit| digit as u8)
}

pub(super) fn is_plainish_char(key: KeyEvent, expected: char) -> bool {
    matches!(key.code, KeyCode::Char(ch) if ch == expected)
        && !key.mods.ctrl
        && !key.mods.alt
        && !key.mods.super_key
}

pub(super) fn is_ctrl_plainish_char(key: KeyEvent, expected: char) -> bool {
    matches!(key.code, KeyCode::Char(ch) if ch == expected)
        && key.mods.ctrl
        && !key.mods.alt
        && !key.mods.super_key
}

pub(super) fn vim_action_should_bubble(action: Action) -> bool {
    matches!(
        action,
        Action::FocusNext
            | Action::FocusPrev
            | Action::Quit
            | Action::DismissOverlay
            | Action::ToggleDevTools
    )
}

pub(super) fn vim_unhandled_key_should_bubble(key: KeyEvent, action: Action) -> bool {
    let modified_shortcut = key.mods.ctrl || key.mods.alt || key.mods.super_key;
    vim_action_should_bubble(action)
        || modified_shortcut
        || matches!(action, Action::InsertNewline)
        || !matches!(key.code, KeyCode::Char(_))
}

pub(super) fn vim_pending_unhandled_key_should_bubble(key: KeyEvent, action: Action) -> bool {
    !key.is(KeyCode::Esc) && vim_unhandled_key_should_bubble(key, action)
}

pub(super) fn handle_text_area_vim_edit_command(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    clipboard: &VimClipboardCtx<'_>,
    _on_vim_mode_change: Option<&Callback<TextAreaVimMode>>,
) -> Option<VimKeyOutcome> {
    if matches!(state.mode, TextAreaVimMode::Insert) {
        return matches!(action, Action::Undo | Action::Redo)
            .then_some(VimKeyOutcome::ConsumedUnchanged);
    }

    if let Some(outcome) = handle_pending_vim_command(editor, state, key, action, clipboard) {
        return Some(outcome);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'u') {
        state.clear_count_pending();
        return Some(if editor.undo() {
            VimKeyOutcome::EditorChanged {
                vertical: false,
                mode_changed: None,
            }
        } else {
            VimKeyOutcome::ConsumedUnchanged
        });
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_ctrl_plainish_char(key, 'r') {
        state.clear_count_pending();
        return Some(if editor.redo() {
            VimKeyOutcome::EditorChanged {
                vertical: false,
                mode_changed: None,
            }
        } else {
            VimKeyOutcome::ConsumedUnchanged
        });
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, '.') {
        state.clear_count_pending();
        return Some(match state.last_change.clone() {
            Some(change) => execute_vim_repeat_change(editor, state, change, clipboard),
            None => VimKeyOutcome::ConsumedUnchanged,
        });
    }

    if is_plainish_char(key, '"') {
        state.pending = Some(TextAreaVimPending::Register);
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'm') {
        state.pending = Some(TextAreaVimPending::MarkSet);
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, '\'') {
        state.pending = Some(TextAreaVimPending::MarkJump { linewise: true });
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, '`') {
        state.pending = Some(TextAreaVimPending::MarkJump { linewise: false });
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, '/') {
        state.pending = Some(TextAreaVimPending::Search {
            forward: true,
            query: String::new(),
            cursor: 0,
        });
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, '?') {
        state.pending = Some(TextAreaVimPending::Search {
            forward: false,
            query: String::new(),
            cursor: 0,
        });
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'n') {
        state.clear_count_pending();
        return Some(vim_repeat_search(editor, state, true));
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'N') {
        state.clear_count_pending();
        return Some(vim_repeat_search(editor, state, false));
    }

    if matches!(state.mode, TextAreaVimMode::Normal)
        && let Some(op) = vim_operator_from_key(key)
    {
        let count = take_vim_count(state).unwrap_or(1).max(1);
        state.pending = Some(TextAreaVimPending::Operator {
            op,
            count,
            g_pending: false,
        });
        return Some(VimKeyOutcome::ConsumedUnchanged);
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'x') {
        let count = vim_repeat_count(state);
        let register = state.active_register.take();
        return Some(vim_delete_chars(
            editor, state, false, count, register, clipboard, true,
        ));
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'X') {
        let count = vim_repeat_count(state);
        let register = state.active_register.take();
        return Some(vim_delete_chars(
            editor, state, true, count, register, clipboard, true,
        ));
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'o') {
        state.clear_count_pending();
        return Some(vim_open_line(editor, state, false, true));
    }

    if matches!(state.mode, TextAreaVimMode::Normal) && is_plainish_char(key, 'O') {
        state.clear_count_pending();
        return Some(vim_open_line(editor, state, true, true));
    }

    if matches!(
        state.mode,
        TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
    ) {
        if is_plainish_char(key, 'd') || is_plainish_char(key, 'x') {
            let register = state.active_register.take();
            return Some(vim_visual_operator(
                editor,
                state,
                VimOperator::Delete,
                register,
                clipboard,
            ));
        }
        if is_plainish_char(key, 'c') {
            let register = state.active_register.take();
            return Some(vim_visual_operator(
                editor,
                state,
                VimOperator::Change,
                register,
                clipboard,
            ));
        }
    }

    if is_plainish_char(key, 'y') {
        let visual_mode = state.mode;
        if matches!(visual_mode, TextAreaVimMode::Normal) {
            return None;
        }
        let register = state.active_register.take();
        let text = match state.mode {
            TextAreaVimMode::Visual | TextAreaVimMode::VisualLine => editor
                .selection()
                .map(|(start, end)| {
                    vim_clipboard_text_for_range(editor, start, end, &clipboard.params)
                })
                .unwrap_or_default(),
            TextAreaVimMode::Insert | TextAreaVimMode::Normal => String::new(),
        };
        if !text.is_empty() {
            let yank_range = editor.selection();
            let raw = editor.selected_text().unwrap_or_default().to_string();
            let copied = vim_store_register(
                state,
                register,
                raw,
                text,
                matches!(visual_mode, TextAreaVimMode::VisualLine),
                VimOperator::Yank,
                clipboard,
            );
            if let Some(range) = yank_range {
                state.set_yank_feedback_range(range, copied);
            }
        }
        let mode_changed = if matches!(
            state.mode,
            TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
        ) {
            let changed = state.set_mode(TextAreaVimMode::Normal);
            editor.clear_selection();
            changed.then_some(TextAreaVimMode::Normal)
        } else {
            None
        };
        return Some(if mode_changed.is_some() {
            VimKeyOutcome::EditorChanged {
                vertical: false,
                mode_changed,
            }
        } else {
            VimKeyOutcome::ConsumedUnchanged
        });
    }

    if is_plainish_char(key, 'p') || is_plainish_char(key, 'P') {
        let before = is_plainish_char(key, 'P');
        let register = state.active_register.take();
        let outcome = vim_paste(editor, state, before, register, clipboard, true);
        return Some(outcome);
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VimResolvedRange {
    start: usize,
    end: usize,
    linewise: bool,
}

pub(super) fn handle_pending_vim_command(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    action: Action,
    clipboard: &VimClipboardCtx<'_>,
) -> Option<VimKeyOutcome> {
    let pending = state.pending.take()?;
    if !matches!(&pending, TextAreaVimPending::Search { .. })
        && vim_pending_unhandled_key_should_bubble(key, action)
    {
        state.pending = Some(pending);
        return Some(VimKeyOutcome::Unhandled);
    }

    match pending {
        TextAreaVimPending::G => {
            state.pending = Some(TextAreaVimPending::G);
            None
        }
        TextAreaVimPending::Register => {
            if let KeyCode::Char(ch) = key.code
                && !key.mods.ctrl
                && !key.mods.alt
                && !key.mods.super_key
            {
                state.active_register = Some(ch);
            }
            Some(VimKeyOutcome::ConsumedUnchanged)
        }
        TextAreaVimPending::MarkSet => {
            if let Some(ch) = plainish_mark_char(key) {
                state.marks.insert(ch, editor.cursor());
            }
            Some(VimKeyOutcome::ConsumedUnchanged)
        }
        TextAreaVimPending::MarkJump { linewise } => {
            let target = if is_plainish_char(key, '\'') || is_plainish_char(key, '`') {
                state.previous_jump
            } else {
                plainish_mark_char(key).and_then(|ch| state.marks.get(&ch).copied())
            };
            Some(match target {
                Some(target) => vim_jump_to_mark(editor, state, target, linewise),
                None => VimKeyOutcome::ConsumedUnchanged,
            })
        }
        TextAreaVimPending::Search {
            forward,
            mut query,
            mut cursor,
        } => {
            cursor = clamp_cursor(&query, cursor);
            if key.is(KeyCode::Esc) {
                state.search.visible = false;
                state.clear_count_pending();
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::Enter) {
                state.search.query = (!query.is_empty()).then_some(query.clone());
                state.search.forward = forward;
                state.search.visible = !query.is_empty();
                state.clear_count_pending();
                return Some(if query.is_empty() {
                    VimKeyOutcome::ConsumedUnchanged
                } else {
                    vim_search(editor, &query, forward)
                });
            }
            if key.is(KeyCode::Backspace) {
                if cursor > 0 {
                    let prev = prev_char_boundary(&query, cursor);
                    query.replace_range(prev..cursor, "");
                    cursor = prev;
                }
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::Delete) {
                if cursor < query.len() {
                    let next = next_char_boundary(&query, cursor);
                    query.replace_range(cursor..next, "");
                }
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::Left) {
                cursor = prev_char_boundary(&query, cursor);
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::Right) {
                cursor = next_char_boundary(&query, cursor);
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::Home) {
                cursor = 0;
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if key.is(KeyCode::End) {
                cursor = query.len();
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::ConsumedUnchanged);
            }
            if pending_vim_search_should_pass_through(key, action) {
                state.pending = Some(TextAreaVimPending::Search {
                    forward,
                    query,
                    cursor,
                });
                return Some(VimKeyOutcome::Unhandled);
            }
            if let KeyCode::Char(ch) = key.code
                && !key.mods.ctrl
                && !key.mods.alt
                && !key.mods.super_key
            {
                query.insert(cursor, ch);
                cursor += ch.len_utf8();
            }
            state.pending = Some(TextAreaVimPending::Search {
                forward,
                query,
                cursor,
            });
            Some(VimKeyOutcome::ConsumedUnchanged)
        }
        TextAreaVimPending::Operator {
            op,
            count,
            g_pending,
        } => Some(handle_pending_vim_operator(
            editor, state, key, op, count, g_pending, clipboard,
        )),
        TextAreaVimPending::TextObject { op, count, around } => {
            let Some(object) = vim_text_object_from_key(key) else {
                state.clear_count_pending();
                return Some(VimKeyOutcome::ConsumedUnchanged);
            };
            let register = state.active_register.take();
            let target = VimRepeatTarget::TextObject {
                object,
                around,
                count,
            };
            Some(execute_vim_operator_target(
                editor,
                state,
                VimOperatorTargetArgs {
                    op,
                    target,
                    repeat_count: 1,
                    register,
                    record_repeat: true,
                },
                clipboard,
            ))
        }
    }
}

pub(super) fn pending_vim_search_should_pass_through(key: KeyEvent, action: Action) -> bool {
    vim_unhandled_key_should_bubble(key, action)
}

pub(super) fn handle_pending_vim_operator(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    key: KeyEvent,
    op: VimOperator,
    op_count: usize,
    g_pending: bool,
    clipboard: &VimClipboardCtx<'_>,
) -> VimKeyOutcome {
    if g_pending {
        if is_plainish_char(key, 'g') {
            let count = take_vim_count(state).unwrap_or(op_count).max(1);
            let target = VimRepeatTarget::Motion {
                motion: VimMotion::GotoLine(count),
                count: 1,
            };
            let register = state.active_register.take();
            return execute_vim_operator_target(
                editor,
                state,
                VimOperatorTargetArgs {
                    op,
                    target,
                    repeat_count: 1,
                    register,
                    record_repeat: true,
                },
                clipboard,
            );
        }
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if is_plainish_char(key, '0') && state.count.is_some() {
        state.push_count_digit(0);
        state.pending = Some(TextAreaVimPending::Operator {
            op,
            count: op_count,
            g_pending: false,
        });
        return VimKeyOutcome::ConsumedUnchanged;
    }
    if let Some(digit) = plainish_digit_1_to_9(key) {
        state.push_count_digit(digit);
        state.pending = Some(TextAreaVimPending::Operator {
            op,
            count: op_count,
            g_pending: false,
        });
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if vim_operator_from_key(key) == Some(op) {
        let count = op_count.saturating_mul(vim_repeat_count(state)).max(1);
        let register = state.active_register.take();
        return execute_vim_operator_target(
            editor,
            state,
            VimOperatorTargetArgs {
                op,
                target: VimRepeatTarget::Line { count },
                repeat_count: 1,
                register,
                record_repeat: true,
            },
            clipboard,
        );
    }

    if is_plainish_char(key, 'i') || is_plainish_char(key, 'a') {
        let count = op_count.saturating_mul(vim_repeat_count(state)).max(1);
        state.pending = Some(TextAreaVimPending::TextObject {
            op,
            count,
            around: is_plainish_char(key, 'a'),
        });
        return VimKeyOutcome::ConsumedUnchanged;
    }

    if is_plainish_char(key, 'g') {
        state.pending = Some(TextAreaVimPending::Operator {
            op,
            count: op_count,
            g_pending: true,
        });
        return VimKeyOutcome::ConsumedUnchanged;
    }

    let Some(motion) = vim_motion_from_key(key) else {
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    };
    let motion = if matches!(op, VimOperator::Change) {
        match motion {
            VimMotion::WordForward => VimMotion::WordEnd,
            VimMotion::BigWordForward => VimMotion::BigWordEnd,
            motion => motion,
        }
    } else {
        motion
    };
    let motion_count = vim_repeat_count(state);
    let count = op_count.saturating_mul(motion_count).max(1);
    let target = VimRepeatTarget::Motion { motion, count };
    let register = state.active_register.take();
    execute_vim_operator_target(
        editor,
        state,
        VimOperatorTargetArgs {
            op,
            target,
            repeat_count: 1,
            register,
            record_repeat: true,
        },
        clipboard,
    )
}

pub(super) fn vim_operator_from_key(key: KeyEvent) -> Option<VimOperator> {
    if is_plainish_char(key, 'd') {
        Some(VimOperator::Delete)
    } else if is_plainish_char(key, 'y') {
        Some(VimOperator::Yank)
    } else if is_plainish_char(key, 'c') {
        Some(VimOperator::Change)
    } else {
        None
    }
}

pub(super) fn vim_motion_from_key(key: KeyEvent) -> Option<VimMotion> {
    if is_plainish_char(key, 'h') {
        Some(VimMotion::Left)
    } else if is_plainish_char(key, 'l') {
        Some(VimMotion::Right)
    } else if is_plainish_char(key, 'j') {
        Some(VimMotion::Down)
    } else if is_plainish_char(key, 'k') {
        Some(VimMotion::Up)
    } else if is_plainish_char(key, 'w') {
        Some(VimMotion::WordForward)
    } else if is_plainish_char(key, 'b') {
        Some(VimMotion::WordBackward)
    } else if is_plainish_char(key, 'e') {
        Some(VimMotion::WordEnd)
    } else if is_plainish_char(key, 'W') {
        Some(VimMotion::BigWordForward)
    } else if is_plainish_char(key, 'B') {
        Some(VimMotion::BigWordBackward)
    } else if is_plainish_char(key, 'E') {
        Some(VimMotion::BigWordEnd)
    } else if is_plainish_char(key, '0') {
        Some(VimMotion::LineStart)
    } else if is_plainish_char(key, '$') {
        Some(VimMotion::LineEnd)
    } else if is_plainish_char(key, 'G') {
        Some(VimMotion::GotoLastLine)
    } else {
        None
    }
}

pub(super) fn vim_text_object_from_key(key: KeyEvent) -> Option<VimTextObject> {
    if is_plainish_char(key, 'w') {
        Some(VimTextObject::Word)
    } else if is_plainish_char(key, 'W') {
        Some(VimTextObject::BigWord)
    } else if is_plainish_char(key, 'p') {
        Some(VimTextObject::Paragraph)
    } else if is_plainish_char(key, '\'') {
        Some(VimTextObject::SingleQuote)
    } else if is_plainish_char(key, '"') {
        Some(VimTextObject::DoubleQuote)
    } else if is_plainish_char(key, '`') {
        Some(VimTextObject::Backtick)
    } else if is_plainish_char(key, '(') || is_plainish_char(key, ')') {
        Some(VimTextObject::Paren)
    } else if is_plainish_char(key, '[') || is_plainish_char(key, ']') {
        Some(VimTextObject::Bracket)
    } else if is_plainish_char(key, '{') || is_plainish_char(key, '}') {
        Some(VimTextObject::Brace)
    } else if is_plainish_char(key, '<') || is_plainish_char(key, '>') {
        Some(VimTextObject::Angle)
    } else {
        None
    }
}

pub(super) fn execute_vim_operator_target(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    args: VimOperatorTargetArgs,
    clipboard: &VimClipboardCtx<'_>,
) -> VimKeyOutcome {
    let VimOperatorTargetArgs {
        op,
        target,
        repeat_count,
        register,
        record_repeat,
    } = args;
    let Some(range) = vim_resolve_repeat_target(editor, &target, repeat_count) else {
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    };
    execute_vim_operator_range(
        editor,
        state,
        op,
        range,
        register,
        clipboard,
        record_repeat.then_some(target),
    )
}

pub(super) fn execute_vim_operator_range(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    op: VimOperator,
    range: VimResolvedRange,
    register: Option<char>,
    clipboard: &VimClipboardCtx<'_>,
    repeat_target: Option<VimRepeatTarget>,
) -> VimKeyOutcome {
    let start = range.start.min(editor.text().len());
    let end = range.end.min(editor.text().len());
    if start >= end {
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    }

    let raw = editor.text()[start..end].to_string();
    let clipboard_text = vim_clipboard_text_for_range(editor, start, end, &clipboard.params);
    let copied = vim_store_register(
        state,
        register,
        raw,
        clipboard_text,
        range.linewise,
        op,
        clipboard,
    );
    if matches!(op, VimOperator::Yank) {
        state.set_yank_feedback_range((start, end), copied);
    }

    match op {
        VimOperator::Yank => {
            state.clear_count_pending();
            VimKeyOutcome::ConsumedUnchanged
        }
        VimOperator::Delete | VimOperator::Change => {
            let repeat_target_for_change = repeat_target.clone();
            let changed = editor.replace_range(
                start,
                end,
                if matches!(op, VimOperator::Change) && range.linewise {
                    "\n"
                } else {
                    ""
                },
                crate::text::edit::TextEditKind::Replace,
            );
            if matches!(op, VimOperator::Change) && range.linewise {
                editor.set_cursor(start);
            }
            vim_clamp_normal_cursor_after_edit(editor);
            if let Some(target) = repeat_target {
                if matches!(op, VimOperator::Change) {
                    begin_vim_insert_session(
                        editor,
                        state,
                        VimInsertOrigin::Change { target, register },
                    );
                } else {
                    state.last_change = Some(VimRepeatChange::Operator {
                        op,
                        target,
                        register,
                    });
                }
            } else if matches!(op, VimOperator::Change)
                && let Some(target) = repeat_target_for_change
            {
                begin_vim_insert_session(
                    editor,
                    state,
                    VimInsertOrigin::Change { target, register },
                );
            }
            let mode_changed = matches!(op, VimOperator::Change)
                .then(|| state.set_mode(TextAreaVimMode::Insert))
                .unwrap_or_else(|| {
                    state.clear_count_pending();
                    false
                });
            if changed || mode_changed {
                VimKeyOutcome::EditorChanged {
                    vertical: false,
                    mode_changed: mode_changed.then_some(TextAreaVimMode::Insert),
                }
            } else {
                VimKeyOutcome::ConsumedUnchanged
            }
        }
    }
}

pub(super) fn vim_resolve_repeat_target(
    editor: &TextEditor,
    target: &VimRepeatTarget,
    repeat_count: usize,
) -> Option<VimResolvedRange> {
    match target {
        VimRepeatTarget::Motion { motion, count } => {
            vim_motion_range(editor, *motion, count.saturating_mul(repeat_count).max(1))
        }
        VimRepeatTarget::Line { count } => Some(vim_line_range(
            editor.text(),
            editor.cursor(),
            count.saturating_mul(repeat_count),
        )),
        VimRepeatTarget::TextObject {
            object,
            around,
            count,
        } => vim_text_object_range(
            editor.text(),
            editor.cursor(),
            *object,
            *around,
            count.saturating_mul(repeat_count).max(1),
        ),
    }
}

pub(super) fn vim_motion_range(
    editor: &TextEditor,
    motion: VimMotion,
    count: usize,
) -> Option<VimResolvedRange> {
    let text = editor.text();
    let cursor = editor.cursor();
    let count = count.max(1);
    match motion {
        VimMotion::Left => {
            let mut target = cursor;
            for _ in 0..count {
                target = prev_char_boundary(text, target);
            }
            (target < cursor).then_some(VimResolvedRange {
                start: target,
                end: cursor,
                linewise: false,
            })
        }
        VimMotion::Right => {
            let mut target = cursor;
            for _ in 0..count {
                target = next_char_boundary(text, target);
            }
            (target > cursor).then_some(VimResolvedRange {
                start: cursor,
                end: target,
                linewise: false,
            })
        }
        VimMotion::WordForward => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_word_forward_start(text, target);
            }
            (target > cursor).then_some(VimResolvedRange {
                start: cursor,
                end: target,
                linewise: false,
            })
        }
        VimMotion::WordBackward => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_word_backward_start(text, target);
            }
            (target < cursor).then_some(VimResolvedRange {
                start: target,
                end: cursor,
                linewise: false,
            })
        }
        VimMotion::WordEnd => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_word_end(text, target);
            }
            (target > cursor).then_some(VimResolvedRange {
                start: cursor,
                end: target,
                linewise: false,
            })
        }
        VimMotion::BigWordForward => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_big_word_forward_start(text, target);
            }
            (target > cursor).then_some(VimResolvedRange {
                start: cursor,
                end: target,
                linewise: false,
            })
        }
        VimMotion::BigWordBackward => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_big_word_backward_start(text, target);
            }
            (target < cursor).then_some(VimResolvedRange {
                start: target,
                end: cursor,
                linewise: false,
            })
        }
        VimMotion::BigWordEnd => {
            let mut target = cursor;
            for _ in 0..count {
                target = vim_big_word_end(text, target);
            }
            (target > cursor).then_some(VimResolvedRange {
                start: cursor,
                end: target,
                linewise: false,
            })
        }
        VimMotion::LineStart => {
            let start = line_start_at(text, cursor);
            (start < cursor).then_some(VimResolvedRange {
                start,
                end: cursor,
                linewise: false,
            })
        }
        VimMotion::LineEnd => {
            let current_line = line_index_at(text, cursor);
            let target_line = current_line.saturating_add(count.saturating_sub(1));
            let target_start = line_start_by_index(text, target_line.min(line_count(text) - 1));
            let end = line_end_at(text, target_start);
            (end > cursor).then_some(VimResolvedRange {
                start: cursor,
                end,
                linewise: false,
            })
        }
        VimMotion::Up | VimMotion::Down => {
            let current = line_index_at(text, cursor);
            let last = line_count(text).saturating_sub(1);
            let target = if matches!(motion, VimMotion::Down) {
                current.saturating_add(count).min(last)
            } else {
                current.saturating_sub(count)
            };
            Some(vim_line_range_between(text, current, target))
        }
        VimMotion::GotoLastLine => Some(vim_line_range_between(
            text,
            line_index_at(text, cursor),
            line_count(text).saturating_sub(1),
        )),
        VimMotion::GotoLine(line) => Some(vim_line_range_between(
            text,
            line_index_at(text, cursor),
            line.saturating_sub(1)
                .min(line_count(text).saturating_sub(1)),
        )),
    }
}

pub(super) fn vim_line_range(text: &str, cursor: usize, count: usize) -> VimResolvedRange {
    let start = line_start_at(text, cursor);
    let start_line = line_index_at(text, cursor);
    let last_line = line_count(text).saturating_sub(1);
    let target_line = start_line
        .saturating_add(count.max(1).saturating_sub(1))
        .min(last_line);
    let target_start = line_start_by_index(text, target_line);
    VimResolvedRange {
        start,
        end: line_end_including_newline(text, target_start),
        linewise: true,
    }
}

pub(super) fn vim_line_range_between(text: &str, line_a: usize, line_b: usize) -> VimResolvedRange {
    let first = line_a.min(line_b);
    let last = line_a.max(line_b);
    let start = line_start_by_index(text, first);
    let end_start = line_start_by_index(text, last);
    VimResolvedRange {
        start,
        end: line_end_including_newline(text, end_start),
        linewise: true,
    }
}

pub(super) fn vim_text_object_range(
    text: &str,
    cursor: usize,
    object: VimTextObject,
    around: bool,
    count: usize,
) -> Option<VimResolvedRange> {
    match object {
        VimTextObject::Word => vim_word_text_object_range(text, cursor, around, count),
        VimTextObject::BigWord => vim_word_text_object_range(text, cursor, around, count),
        VimTextObject::Paragraph => vim_paragraph_text_object_range(text, cursor, around, count),
        VimTextObject::SingleQuote => {
            vim_delimited_text_object_range(text, cursor, '\'', '\'', around)
        }
        VimTextObject::DoubleQuote => {
            vim_delimited_text_object_range(text, cursor, '"', '"', around)
        }
        VimTextObject::Backtick => vim_delimited_text_object_range(text, cursor, '`', '`', around),
        VimTextObject::Paren => vim_delimited_text_object_range(text, cursor, '(', ')', around),
        VimTextObject::Bracket => vim_delimited_text_object_range(text, cursor, '[', ']', around),
        VimTextObject::Brace => vim_delimited_text_object_range(text, cursor, '{', '}', around),
        VimTextObject::Angle => vim_delimited_text_object_range(text, cursor, '<', '>', around),
    }
}

pub(super) fn vim_word_text_object_range(
    text: &str,
    cursor: usize,
    around: bool,
    count: usize,
) -> Option<VimResolvedRange> {
    let (mut start, mut end) = vim_word_text_object_base_range(text, cursor)?;
    for _ in 1..count.max(1) {
        let mut next_cursor = end;
        while next_cursor < text.len()
            && text[next_cursor..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            let next = next_char_boundary(text, next_cursor);
            if next == next_cursor {
                break;
            }
            next_cursor = next;
        }
        let Some((_, next_end)) = vim_word_text_object_base_range(text, next_cursor) else {
            break;
        };
        end = next_end;
    }
    if around {
        let mut around_end = end;
        while around_end < text.len() {
            let Some(ch) = text[around_end..].chars().next() else {
                break;
            };
            if !ch.is_whitespace() || ch == '\n' {
                break;
            }
            around_end += ch.len_utf8();
        }
        if around_end == end {
            while let Some((prev, ch)) = text[..start].char_indices().next_back() {
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                start = prev;
            }
        } else {
            end = around_end;
        }
    }
    (start < end).then_some(VimResolvedRange {
        start,
        end,
        linewise: false,
    })
}

pub(super) fn vim_word_text_object_base_range(text: &str, cursor: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    let mut start = cursor.min(text.len());
    if start == text.len() {
        start = prev_char_boundary(text, start);
    }
    while start < text.len()
        && text[start..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    {
        let next = next_char_boundary(text, start);
        if next == start {
            break;
        }
        start = next;
    }
    if start >= text.len() {
        return None;
    }
    let is_word = |ch: char| !ch.is_whitespace();
    while let Some((prev, ch)) = text[..start].char_indices().next_back() {
        if !is_word(ch) {
            break;
        }
        start = prev;
    }
    let mut end = start;
    while end < text.len() {
        let Some(ch) = text[end..].chars().next() else {
            break;
        };
        if !is_word(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    Some((start, end))
}

pub(super) fn vim_paragraph_text_object_range(
    text: &str,
    cursor: usize,
    around: bool,
    count: usize,
) -> Option<VimResolvedRange> {
    if text.is_empty() {
        return None;
    }
    let mut start = line_start_at(text, cursor);
    while start > 0 {
        let prev_end = start.saturating_sub(1);
        let prev_start = line_start_at(text, prev_end);
        if text[prev_start..prev_end].trim().is_empty() {
            break;
        }
        start = prev_start;
    }
    let mut end = line_end_including_newline(text, cursor);
    let mut remaining = count.max(1);
    while end < text.len() {
        let line_end = line_end_at(text, end);
        if text[end..line_end].trim().is_empty() {
            remaining = remaining.saturating_sub(1);
            if around {
                end = line_end_including_newline(text, end);
            }
            if remaining <= 1 {
                break;
            }
            end = line_end_including_newline(text, end);
            break;
        }
        end = line_end_including_newline(text, end);
    }
    while remaining > 1 && end < text.len() {
        let next_line_end = line_end_at(text, end);
        if text[end..next_line_end].trim().is_empty() {
            remaining = remaining.saturating_sub(1);
            end = line_end_including_newline(text, end);
        } else {
            end = line_end_including_newline(text, end);
        }
    }
    (start < end).then_some(VimResolvedRange {
        start,
        end,
        linewise: true,
    })
}

pub(super) fn vim_delimited_text_object_range(
    text: &str,
    cursor: usize,
    open: char,
    close: char,
    around: bool,
) -> Option<VimResolvedRange> {
    let line = line_bounds_at(text, cursor);
    let cursor = cursor.min(line.end);
    let search_end = next_char_boundary(text, cursor).min(line.end);
    let open_idx = text[line.start..search_end]
        .char_indices()
        .filter_map(|(offset, ch)| (ch == open).then_some(line.start + offset))
        .next_back()?;
    let close_search_start = (open_idx + open.len_utf8()).max(cursor).min(line.end);
    let close_idx = text[close_search_start..line.end]
        .char_indices()
        .find_map(|(offset, ch)| (ch == close).then_some(close_search_start + offset))?;
    let start = if around {
        open_idx
    } else {
        open_idx + open.len_utf8()
    };
    let end = if around {
        close_idx + close.len_utf8()
    } else {
        close_idx
    };
    (start <= end).then_some(VimResolvedRange {
        start,
        end,
        linewise: false,
    })
}

pub(super) fn vim_clipboard_text_for_range(
    editor: &TextEditor,
    start: usize,
    end: usize,
    clipboard_params: &TextAreaClipboardParams<'_>,
) -> String {
    let raw = &editor.text()[start..end];
    let sentinel = sentinel_info_for(
        clipboard_params.image_mode,
        clipboard_params.images.len(),
        clipboard_params.image_placeholder,
        clipboard_params.sentinels,
    );
    let text = crate::utils::text::replace_sentinels(
        raw,
        sentinel.as_ref(),
        clipboard_params.image_placeholder,
    );
    match &clipboard_params.clipboard_transform {
        Some(transform) => transform(crate::widgets::TextAreaClipboardTransformEvent {
            text: &text,
            raw_text: raw,
        }),
        None => text.into_owned(),
    }
}

pub(super) fn vim_store_register(
    state: &mut TextAreaVimState,
    register: Option<char>,
    raw_text: String,
    clipboard_text: String,
    linewise: bool,
    op: VimOperator,
    clipboard: &VimClipboardCtx<'_>,
) -> bool {
    if raw_text.is_empty() && clipboard_text.is_empty() {
        return false;
    }
    let resolved_register = register.map(normalize_vim_register);
    if resolved_register == Some('_') {
        return false;
    }
    let value = VimRegisterValue {
        text: raw_text,
        linewise,
    };
    state.registers.values.insert('"', value.clone());
    if matches!(op, VimOperator::Yank) {
        state.registers.values.insert('0', value.clone());
    } else {
        for idx in (2..=9).rev() {
            let prev = char::from_digit(idx - 1, 10).unwrap_or('1');
            let current = char::from_digit(idx, 10).unwrap_or('9');
            if let Some(existing) = state.registers.values.get(&prev).cloned() {
                state.registers.values.insert(current, existing);
            }
        }
        state.registers.values.insert('1', value.clone());
    }
    if let Some(raw_reg) = register.filter(|reg| reg.is_ascii_alphanumeric()) {
        if raw_reg.is_ascii_uppercase() {
            let lower = raw_reg.to_ascii_lowercase();
            if let Some(existing) = state.registers.values.get_mut(&lower) {
                existing.text.push_str(&value.text);
                existing.linewise |= value.linewise;
            } else {
                state.registers.values.insert(lower, value.clone());
            }
        } else if let Some(reg) = resolved_register {
            state.registers.values.insert(reg, value.clone());
        }
    }
    if register.is_none() || register == Some('"') || register == Some('+') {
        router::write_to_clipboard(&clipboard_text, clipboard.clipboard, clipboard.config)
    } else {
        false
    }
}

pub(super) fn normalize_vim_register(register: char) -> char {
    if register.is_ascii_uppercase() {
        register.to_ascii_lowercase()
    } else {
        register
    }
}

pub(super) fn vim_load_register(
    state: &TextAreaVimState,
    register: Option<char>,
    clipboard: &crate::clipboard::ClipboardService,
) -> Option<VimRegisterValue> {
    let register = register.map(normalize_vim_register);
    if register == Some('_') {
        return None;
    }
    if let Some(reg) = register
        && reg != '+'
    {
        return state.registers.values.get(&reg).cloned();
    }
    if register.is_none()
        && let Some(value) = state.registers.values.get(&'"')
    {
        return Some(value.clone());
    }
    match clipboard.read_clipboard_text() {
        Ok(text) if !text.is_empty() => Some(VimRegisterValue {
            linewise: text.ends_with('\n'),
            text,
        }),
        Ok(_) => None,
        Err(err) => {
            clipboard.report_error(err);
            None
        }
    }
}

pub(super) fn vim_delete_chars(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    backward: bool,
    count: usize,
    register: Option<char>,
    clipboard: &VimClipboardCtx<'_>,
    record_repeat: bool,
) -> VimKeyOutcome {
    let cursor = editor.cursor();
    let mut target = cursor;
    for _ in 0..count.max(1) {
        target = if backward {
            prev_char_boundary(editor.text(), target)
        } else {
            next_char_boundary(editor.text(), target)
        };
    }
    let (start, end) = if backward {
        (target, cursor)
    } else {
        (cursor, target)
    };
    if start >= end {
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    }
    let range = VimResolvedRange {
        start,
        end,
        linewise: false,
    };
    let outcome = execute_vim_operator_range(
        editor,
        state,
        VimOperator::Delete,
        range,
        register,
        clipboard,
        None,
    );
    if record_repeat {
        state.last_change = Some(VimRepeatChange::DeleteChar {
            backward,
            count: count.max(1),
            register,
        });
    }
    outcome
}

pub(super) fn vim_visual_operator(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    op: VimOperator,
    register: Option<char>,
    clipboard: &VimClipboardCtx<'_>,
) -> VimKeyOutcome {
    let Some((start, end)) = editor.selection() else {
        let changed = state.set_mode(TextAreaVimMode::Normal);
        return if changed {
            VimKeyOutcome::ModeChanged(TextAreaVimMode::Normal)
        } else {
            VimKeyOutcome::ConsumedUnchanged
        };
    };
    let linewise = matches!(state.mode, TextAreaVimMode::VisualLine);
    let raw = editor.text()[start..end].to_string();
    let clipboard_text = vim_clipboard_text_for_range(editor, start, end, &clipboard.params);
    let copied = vim_store_register(
        state,
        register,
        raw,
        clipboard_text,
        linewise,
        op,
        clipboard,
    );
    if matches!(op, VimOperator::Yank) {
        state.set_yank_feedback_range((start, end), copied);
    }
    let mode_changed = state.set_mode(if matches!(op, VimOperator::Change) {
        TextAreaVimMode::Insert
    } else {
        TextAreaVimMode::Normal
    });
    let changed = if matches!(op, VimOperator::Yank) {
        editor.clear_selection();
        true
    } else {
        editor.replace_range(
            start,
            end,
            if linewise && matches!(op, VimOperator::Change) {
                "\n"
            } else {
                ""
            },
            crate::text::edit::TextEditKind::Replace,
        )
    };
    if linewise && matches!(op, VimOperator::Change) {
        editor.set_cursor(start);
    }
    vim_clamp_normal_cursor_after_edit(editor);
    if matches!(op, VimOperator::Change) {
        let repeat_target = if linewise {
            let start_line = line_index_at(editor.text(), start);
            let end_line = if end == 0 {
                start_line
            } else {
                line_index_at(editor.text(), end.saturating_sub(1))
            };
            VimRepeatTarget::Line {
                count: end_line.saturating_sub(start_line).saturating_add(1),
            }
        } else {
            VimRepeatTarget::Motion {
                motion: VimMotion::Right,
                count: editor.text()[start..end].chars().count().max(1),
            }
        };
        begin_vim_insert_session(
            editor,
            state,
            VimInsertOrigin::Change {
                target: repeat_target,
                register,
            },
        );
    }
    if changed || mode_changed {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: mode_changed.then_some(state.mode),
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn vim_paste(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    before: bool,
    register: Option<char>,
    clipboard: &VimClipboardCtx<'_>,
    record_repeat: bool,
) -> VimKeyOutcome {
    let visual = matches!(
        state.mode,
        TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
    );
    let Some(value) = vim_load_register(state, register, clipboard.clipboard) else {
        if let Some(outcome) = vim_paste_image(editor, state, before, visual, clipboard) {
            if record_repeat && !matches!(outcome, VimKeyOutcome::ConsumedUnchanged) {
                state.last_change = Some(VimRepeatChange::Paste { before, register });
            }
            return outcome;
        }
        if visual {
            let changed_mode = state.set_mode(TextAreaVimMode::Normal);
            editor.clear_selection();
            return VimKeyOutcome::EditorChanged {
                vertical: false,
                mode_changed: changed_mode.then_some(TextAreaVimMode::Normal),
            };
        }
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    };
    if value.text.is_empty() {
        if let Some(outcome) = vim_paste_image(editor, state, before, visual, clipboard) {
            if record_repeat && !matches!(outcome, VimKeyOutcome::ConsumedUnchanged) {
                state.last_change = Some(VimRepeatChange::Paste { before, register });
            }
            return outcome;
        }
        state.clear_count_pending();
        return VimKeyOutcome::ConsumedUnchanged;
    }

    let mut paste_text = value.text.clone();
    let mut linewise_cursor_adjust = 0usize;
    if !visual {
        if value.linewise {
            let target = if before {
                line_start_at(editor.text(), editor.cursor())
            } else {
                line_end_including_newline(editor.text(), editor.cursor())
            };
            if !before && target == editor.text().len() && !editor.text().ends_with('\n') {
                paste_text.insert(0, '\n');
                linewise_cursor_adjust = 1;
            }
            editor.set_cursor(target);
        } else if !before {
            let target = next_char_boundary(editor.text(), editor.cursor());
            editor.set_cursor(target);
        }
    }

    let start = editor
        .selection()
        .map_or(editor.cursor(), |(start, _)| start);
    let changed = editor.insert_text(&paste_text);
    if changed {
        if value.linewise {
            let len = editor.text().len();
            editor.set_cursor(start.saturating_add(linewise_cursor_adjust).min(len));
        } else {
            let end = editor.cursor();
            let target = prev_char_boundary(editor.text(), end);
            editor.set_cursor(target);
        }
    }
    let mode_changed = if visual {
        state.set_mode(TextAreaVimMode::Normal)
    } else {
        state.clear_count_pending();
        false
    };
    if record_repeat {
        state.last_change = Some(VimRepeatChange::Paste { before, register });
    }
    if changed || mode_changed {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: mode_changed.then_some(TextAreaVimMode::Normal),
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

fn vim_paste_image(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    before: bool,
    visual: bool,
    clipboard: &VimClipboardCtx<'_>,
) -> Option<VimKeyOutcome> {
    if clipboard.params.on_image_paste.is_none() && clipboard.params.on_images_change.is_none() {
        return None;
    }

    let content = match router::read_image_paste_content(clipboard.clipboard, clipboard.config) {
        router::ImagePasteContent::Image(content) => content,
        router::ImagePasteContent::SuppressTextFallback => {
            state.clear_count_pending();
            return Some(VimKeyOutcome::ConsumedUnchanged);
        }
        router::ImagePasteContent::None => return None,
    };

    if let Some(cb) = clipboard.params.on_image_paste {
        cb.emit(content.clone());
    }

    let Some(on_images_change) = clipboard.params.on_images_change else {
        state.clear_count_pending();
        return Some(VimKeyOutcome::ConsumedUnchanged);
    };

    let mut new_images = clipboard.params.images.to_vec();
    let new_index = new_images.len();
    new_images.push(content);

    let mut changed = false;
    match clipboard.params.image_mode {
        TextAreaImageMode::Inline => {
            if !visual && !before {
                let target = next_char_boundary(editor.text(), editor.cursor());
                editor.set_cursor(target);
            }
            let insert_end = editor
                .selection()
                .map(|(_, end)| end)
                .unwrap_or_else(|| editor.cursor());
            let tail_starts_with_space = editor
                .text()
                .get(insert_end..)
                .is_some_and(|tail| tail.starts_with(' '));
            let sentinel = char::from_u32(IMAGE_SENTINEL_BASE as u32 + new_index as u32)
                .unwrap_or(IMAGE_SENTINEL_BASE);
            let mut text = sentinel.to_string();
            if !tail_starts_with_space {
                text.push(' ');
            }
            changed = editor.insert_text(&text);
            if changed {
                let text = editor.text().to_owned();
                editor.remember_text_area_images(&text, &new_images);
            }
        }
        TextAreaImageMode::Attachment => {
            on_images_change.emit(new_images);
        }
    }

    let mode_changed = if visual {
        state.set_mode(TextAreaVimMode::Normal)
    } else {
        state.clear_count_pending();
        false
    };

    if changed || mode_changed {
        Some(VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: mode_changed.then_some(TextAreaVimMode::Normal),
        })
    } else {
        Some(VimKeyOutcome::ConsumedUnchanged)
    }
}

pub(super) fn vim_open_line(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    above: bool,
    _record_repeat: bool,
) -> VimKeyOutcome {
    let bounds = line_bounds_at(editor.text(), editor.cursor());
    let indent_end = first_nonblank_in_line(editor.text(), bounds.start, bounds.end);
    let indent = editor.text()[bounds.start..indent_end].to_string();
    let insert_at = if above { bounds.start } else { bounds.end };
    let inserted = if above {
        format!("{indent}\n")
    } else {
        format!("\n{indent}")
    };
    editor.set_cursor(insert_at);
    let changed = editor.insert_str(&inserted);
    let cursor = if above {
        insert_at + indent.len()
    } else {
        insert_at + 1 + indent.len()
    };
    editor.set_cursor(cursor);
    begin_vim_insert_session(editor, state, VimInsertOrigin::OpenLine { above });
    let mode_changed = state.set_mode(TextAreaVimMode::Insert);
    if changed || mode_changed {
        VimKeyOutcome::EditorChanged {
            vertical: false,
            mode_changed: mode_changed.then_some(TextAreaVimMode::Insert),
        }
    } else {
        VimKeyOutcome::ConsumedUnchanged
    }
}

pub(super) fn begin_vim_insert_session(
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    origin: VimInsertOrigin,
) {
    state.insert_session = Some(VimInsertSession {
        origin,
        text_before: editor.text().to_string(),
        insert_at: editor.cursor(),
    });
}

pub(super) fn finalize_vim_insert_session(editor: &TextEditor, state: &mut TextAreaVimState) {
    let Some(session) = state.insert_session.take() else {
        return;
    };
    let inserted = vim_inserted_text_since(&session.text_before, editor.text(), session.insert_at);
    state.last_change = Some(match session.origin {
        VimInsertOrigin::Change { target, register } => VimRepeatChange::Change {
            target,
            register,
            inserted,
        },
        VimInsertOrigin::OpenLine { above } => VimRepeatChange::OpenLine { above, inserted },
        VimInsertOrigin::Insert { kind } => VimRepeatChange::Insert { kind, inserted },
    });
}

pub(super) fn vim_inserted_text_since(before: &str, after: &str, insert_at: usize) -> String {
    let insert_at = insert_at.min(before.len()).min(after.len());
    if before == after {
        return String::new();
    }
    let prefix = &before[..insert_at];
    if !after.starts_with(prefix) {
        return after[insert_at.min(after.len())..].to_string();
    }
    let mut before_suffix = before.len();
    let mut after_suffix = after.len();
    while before_suffix > insert_at && after_suffix > insert_at {
        let Some((next_before, before_ch)) = before[..before_suffix].char_indices().next_back()
        else {
            break;
        };
        let Some((next_after, after_ch)) = after[..after_suffix].char_indices().next_back() else {
            break;
        };
        if before_ch != after_ch {
            break;
        }
        before_suffix = next_before;
        after_suffix = next_after;
    }
    after[insert_at..after_suffix].to_string()
}

pub(super) fn vim_repeat_change_with_insert(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    target: VimRepeatTarget,
    register: Option<char>,
    inserted: &str,
    clipboard: &VimClipboardCtx<'_>,
) -> VimKeyOutcome {
    let outcome = execute_vim_operator_target(
        editor,
        state,
        VimOperatorTargetArgs {
            op: VimOperator::Change,
            target,
            repeat_count: 1,
            register,
            record_repeat: false,
        },
        clipboard,
    );
    if !matches!(outcome, VimKeyOutcome::EditorChanged { .. }) {
        return outcome;
    }
    if !inserted.is_empty()
        && let Some(edit) = editor.core.last_edit.clone()
    {
        remap_vim_marks_range(state, edit.start, edit.deleted.len(), edit.inserted.len());
    }
    state.insert_session = None;
    if !inserted.is_empty() {
        let _ = editor.insert_str(inserted);
    }
    state.set_mode(TextAreaVimMode::Normal);
    VimKeyOutcome::EditorChanged {
        vertical: false,
        mode_changed: None,
    }
}

pub(super) fn vim_repeat_open_line(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    above: bool,
    inserted: &str,
) -> VimKeyOutcome {
    let outcome = vim_open_line(editor, state, above, false);
    if !matches!(outcome, VimKeyOutcome::EditorChanged { .. }) {
        return outcome;
    }
    if !inserted.is_empty()
        && let Some(edit) = editor.core.last_edit.clone()
    {
        remap_vim_marks_range(state, edit.start, edit.deleted.len(), edit.inserted.len());
    }
    state.insert_session = None;
    if !inserted.is_empty() {
        let _ = editor.insert_str(inserted);
    }
    state.set_mode(TextAreaVimMode::Normal);
    VimKeyOutcome::EditorChanged {
        vertical: false,
        mode_changed: None,
    }
}

pub(super) fn vim_repeat_insert(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    kind: VimInsertKind,
    inserted: &str,
) -> VimKeyOutcome {
    match kind {
        VimInsertKind::Insert => {}
        VimInsertKind::Append => {
            let _ = editor.move_right();
        }
        VimInsertKind::InsertLineStart => {
            let bounds = line_bounds_at(editor.text(), editor.cursor());
            let target = first_nonblank_in_line(editor.text(), bounds.start, bounds.end);
            editor.set_cursor(target);
        }
        VimInsertKind::AppendLineEnd => {
            let bounds = line_bounds_at(editor.text(), editor.cursor());
            editor.set_cursor(bounds.end);
        }
    }
    if !inserted.is_empty() {
        let _ = editor.insert_str(inserted);
    }
    state.set_mode(TextAreaVimMode::Normal);
    VimKeyOutcome::EditorChanged {
        vertical: false,
        mode_changed: None,
    }
}

pub(super) fn execute_vim_repeat_change(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    change: VimRepeatChange,
    clipboard: &VimClipboardCtx<'_>,
) -> VimKeyOutcome {
    match change {
        VimRepeatChange::Operator {
            op,
            target,
            register,
        } => execute_vim_operator_target(
            editor,
            state,
            VimOperatorTargetArgs {
                op,
                target,
                repeat_count: 1,
                register,
                record_repeat: false,
            },
            clipboard,
        ),
        VimRepeatChange::Change {
            target,
            register,
            inserted,
        } => vim_repeat_change_with_insert(editor, state, target, register, &inserted, clipboard),
        VimRepeatChange::DeleteChar {
            backward,
            count,
            register,
        } => vim_delete_chars(editor, state, backward, count, register, clipboard, false),
        VimRepeatChange::Paste { before, register } => {
            vim_paste(editor, state, before, register, clipboard, false)
        }
        VimRepeatChange::OpenLine { above, inserted } => {
            vim_repeat_open_line(editor, state, above, &inserted)
        }
        VimRepeatChange::Insert { kind, inserted } => {
            vim_repeat_insert(editor, state, kind, &inserted)
        }
    }
}

pub(super) fn vim_search(editor: &mut TextEditor, query: &str, forward: bool) -> VimKeyOutcome {
    let Some(target) = vim_find_search(editor.text(), editor.cursor(), query, forward) else {
        return VimKeyOutcome::ConsumedUnchanged;
    };
    vim_set_cursor(editor, target, false)
}

pub(super) fn vim_repeat_search(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    forward: bool,
) -> VimKeyOutcome {
    let Some(query) = state.search.query.clone() else {
        return VimKeyOutcome::ConsumedUnchanged;
    };
    state.search.visible = true;
    vim_search(editor, &query, forward)
}

pub(super) fn vim_jump_to_mark(
    editor: &mut TextEditor,
    state: &mut TextAreaVimState,
    target: usize,
    linewise: bool,
) -> VimKeyOutcome {
    let old = editor.cursor();
    state.previous_jump = Some(old);
    let target = if linewise {
        let bounds = line_bounds_at(editor.text(), target);
        first_nonblank_in_line(editor.text(), bounds.start, bounds.end)
    } else {
        target
    };
    vim_set_cursor(editor, target, false)
}

pub(super) fn plainish_mark_char(key: KeyEvent) -> Option<char> {
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    (!key.mods.ctrl && !key.mods.alt && !key.mods.super_key && ch.is_ascii_alphabetic())
        .then_some(ch.to_ascii_lowercase())
}

pub(super) fn vim_clamp_normal_cursor_after_edit(editor: &mut TextEditor) {
    if editor.text().is_empty() || editor.cursor() < editor.text().len() {
        return;
    }
    let target = prev_char_boundary(editor.text(), editor.text().len());
    editor.set_cursor(target);
}

pub(super) fn text_area_clipboard_action_may_mutate(action: Action) -> bool {
    matches!(
        action,
        Action::Cut | Action::Paste | Action::PasteFromSelection | Action::PasteImage
    )
}

pub(super) fn exit_text_area_visual_mode_if_needed(
    state: &mut TextAreaVimState,
    on_vim_mode_change: Option<&Callback<TextAreaVimMode>>,
) {
    if matches!(
        state.mode,
        TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
    ) && state.set_mode(TextAreaVimMode::Normal)
        && let Some(cb) = on_vim_mode_change
    {
        cb.emit(TextAreaVimMode::Normal);
    }
}
