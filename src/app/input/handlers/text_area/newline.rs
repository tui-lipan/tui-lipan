use super::*;

pub(super) fn effective_text_area_newline_binding(
    widget_binding: Option<TextAreaNewlineBinding>,
    app_binding: TextAreaNewlineBinding,
) -> TextAreaNewlineBinding {
    widget_binding.unwrap_or(app_binding)
}

pub(super) fn text_area_should_insert_newline(
    key: KeyEvent,
    binding: TextAreaNewlineBinding,
) -> bool {
    if !matches!(key.code, KeyCode::Enter) {
        return false;
    }

    match binding {
        TextAreaNewlineBinding::Enter => false,
        TextAreaNewlineBinding::ShiftEnter | TextAreaNewlineBinding::EnterOrShiftEnter => {
            key.mods.shift && !key.mods.ctrl && !key.mods.alt && !key.mods.super_key
        }
    }
}

pub(super) fn text_area_should_block_enter(key: KeyEvent, binding: TextAreaNewlineBinding) -> bool {
    if !matches!(binding, TextAreaNewlineBinding::ShiftEnter) {
        return false;
    }

    matches!(key.code, KeyCode::Enter)
        && !key.mods.shift
        && !key.mods.ctrl
        && !key.mods.alt
        && !key.mods.super_key
}

// ── Test-only re-exports ────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) fn test_effective_text_area_newline_binding(
    widget_binding: Option<TextAreaNewlineBinding>,
    app_binding: TextAreaNewlineBinding,
) -> TextAreaNewlineBinding {
    effective_text_area_newline_binding(widget_binding, app_binding)
}

#[cfg(test)]
pub(crate) fn test_text_area_should_insert_newline(
    key: KeyEvent,
    binding: TextAreaNewlineBinding,
) -> bool {
    text_area_should_insert_newline(key, binding)
}

#[cfg(test)]
pub(crate) fn test_text_area_should_block_enter(
    key: KeyEvent,
    binding: TextAreaNewlineBinding,
) -> bool {
    text_area_should_block_enter(key, binding)
}
