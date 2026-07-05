//! Keyboard event dispatch logic.
//!
//! The per-widget handler logic lives in [`super::handlers`]; this module
//! provides the thin [`dispatch_key`] dispatcher that classifies the focused
//! node and delegates to the appropriate handler.

use crate::app::input::drag::{self, document_view_selected_text_from_node};
use crate::app::input::handlers::{self, KeyCtx};
use crate::app::input::keymap::{Action, Keymap};
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::ui::capabilities::{
    InputClipboardContext, ReadOnlyClipboardContext, TextAreaClipboardContext,
    TextAreaClipboardParams, selection_range,
};
#[cfg(feature = "terminal")]
use crate::widgets::internal::terminal_selection_text;

fn selection_clipboard_cut_requested(keymap: &Keymap, key: KeyEvent) -> Option<bool> {
    let mut has_clipboard_shortcut = false;
    let mut cut_requested = false;
    for binding in keymap.matches(key) {
        match binding.action {
            Action::Copy => has_clipboard_shortcut = true,
            Action::Cut => {
                has_clipboard_shortcut = true;
                cut_requested = true;
            }
            _ => {}
        }
    }

    has_clipboard_shortcut.then_some(cut_requested)
}

fn dispatch_clipboard_with_feedback(
    key: KeyEvent,
    context: &mut dyn crate::ui::capabilities::ClipboardContext,
    ctx: &mut KeyCtx<'_>,
    id: NodeId,
) -> bool {
    let dispatch = crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
        key,
        ctx.keymap,
        context,
        ctx.clipboard,
        ctx.clipboard_config,
        ctx.copy_feedback,
        id,
    );
    ctx.record_copy_feedback_dispatch(dispatch)
}

pub(crate) fn dispatch_selection_clipboard_shortcut(
    tree: &mut NodeTree,
    key: KeyEvent,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let Some(cut_requested) = selection_clipboard_cut_requested(ctx.keymap, key) else {
        return false;
    };

    let read_only_selection = ctx.read_only_selection;

    let mut candidates: Vec<NodeId> = read_only_selection
        .map(|selections| selections.keys().copied().collect())
        .unwrap_or_default();
    candidates.extend(tree.iter().filter_map(|node| {
        let has_selection = match &node.kind {
            NodeKind::Input(input) => input.anchor.is_some_and(|anchor| anchor != input.cursor),
            NodeKind::TextArea(text_area) => text_area
                .anchor
                .is_some_and(|anchor| anchor != text_area.cursor),
            NodeKind::DocumentView(doc) => {
                doc.table_rect_selection.is_some()
                    || doc
                        .selection_anchor
                        .is_some_and(|anchor| anchor != doc.selection_cursor)
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(term) => term
                .selection
                .as_ref()
                .is_some_and(|selection| !selection.is_empty()),
            _ => false,
        };

        has_selection.then_some(node.id)
    }));

    candidates.sort_unstable_by_key(|id| std::cmp::Reverse((id.index, id.generation)));
    candidates.dedup();

    let mut handled_shared_groups = std::collections::HashSet::new();
    #[cfg(feature = "diff-view")]
    let mut handled_diff_split_pairs = std::collections::HashSet::new();

    for id in candidates {
        if !tree.is_valid(id) {
            continue;
        }

        let handled = match &tree.node(id).kind {
            NodeKind::Input(node) => {
                let (cursor, anchor) = read_only_selection
                    .and_then(|selections| selections.get(&id).copied())
                    .unwrap_or((node.cursor, node.anchor));
                let disabled = node.disabled;
                let read_only = node.read_only;
                let value = node.value.clone();
                let is_masked = node.mask.is_some();
                let on_change = node.on_change.clone();
                let on_edit = node.on_edit.clone();
                let selection = selection_range(cursor, anchor, node.value.len());
                if cut_requested && !disabled && !read_only && on_change.is_some() {
                    let input = ctx.input_history.entry(id).or_insert_with(|| {
                        let mut input = crate::text::input::TextInput::new(value.as_ref());
                        input.set_cursor(cursor);
                        input.set_anchor(anchor);
                        input
                    });
                    input.sync_from(value.as_ref(), cursor, anchor);

                    let dispatch = {
                        let mut context = InputClipboardContext::new(
                            input,
                            on_change.as_ref(),
                            on_edit.as_ref(),
                            !is_masked,
                            true,
                            is_masked,
                        );
                        crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
                            key,
                            ctx.keymap,
                            &mut context,
                            ctx.clipboard,
                            ctx.clipboard_config,
                            ctx.copy_feedback,
                            id,
                        )
                    };
                    ctx.record_copy_feedback_dispatch(dispatch)
                } else {
                    let mut context = ReadOnlyClipboardContext::new(
                        value.as_ref(),
                        selection,
                        !is_masked,
                        is_masked,
                    );
                    dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                }
            }
            NodeKind::TextArea(node) => {
                let (cursor, anchor) = read_only_selection
                    .and_then(|selections| selections.get(&id).copied())
                    .unwrap_or((node.cursor, node.anchor));
                let disabled = node.disabled;
                let read_only = node.read_only;
                let value = node.value.clone();
                let on_change = node.on_change.clone();
                let on_edit = node.on_edit.clone();
                let on_editor_state_change = node.on_editor_state_change.clone();
                let on_image_paste = node.on_image_paste.clone();
                let on_text_paste = node.on_text_paste.clone();
                let images = node.images.clone();
                let on_images_change = node.on_images_change.clone();
                let image_mode = node.image_mode;
                let image_placeholder = node.image_placeholder.clone();
                let sentinels = node.sentinels.clone();
                let copy_excluded_bytes = node.copy_excluded_bytes.clone();
                let clipboard_transform = node.clipboard_transform.clone();
                let selection = selection_range(cursor, anchor, value.len());

                if cut_requested && !disabled && !read_only && on_change.is_some() {
                    let editor = ctx.textarea_history.entry(id).or_insert_with(|| {
                        let mut editor = crate::text::editor::TextEditor::new(value.as_ref());
                        editor.set_cursor(cursor);
                        editor.set_anchor(anchor);
                        editor
                    });
                    editor.sync_from(value.as_ref(), cursor, anchor);

                    let dispatch = {
                        let mut context = TextAreaClipboardContext::new(
                            editor,
                            TextAreaClipboardParams {
                                on_change: on_change.as_ref(),
                                on_edit: on_edit.as_ref(),
                                on_editor_state_change: on_editor_state_change.as_ref(),
                                on_image_paste: on_image_paste.as_ref(),
                                on_text_paste: on_text_paste.as_ref(),
                                images: images.as_slice(),
                                on_images_change: on_images_change.as_ref(),
                                image_mode,
                                image_placeholder: &image_placeholder,
                                sentinels: sentinels.as_slice(),
                                clipboard_transform: clipboard_transform.clone(),
                                editable: true,
                            },
                        );
                        crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
                            key,
                            ctx.keymap,
                            &mut context,
                            ctx.clipboard,
                            ctx.clipboard_config,
                            ctx.copy_feedback,
                            id,
                        )
                    };
                    ctx.record_copy_feedback_dispatch(dispatch)
                } else {
                    let excluded: &[(usize, usize)] = copy_excluded_bytes
                        .as_deref()
                        .map(|v| v.as_slice())
                        .unwrap_or(&[]);
                    let sentinel = crate::widgets::sentinel_info_for(
                        image_mode,
                        images.len(),
                        &image_placeholder,
                        &sentinels,
                    );
                    let mut context =
                        ReadOnlyClipboardContext::new(value.as_ref(), selection, true, false)
                            .with_sentinel(sentinel, &image_placeholder)
                            .with_excluded_bytes(excluded)
                            .with_clipboard_transform(clipboard_transform.clone());
                    dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                }
            }
            NodeKind::DocumentView(node) => {
                #[cfg(feature = "diff-view")]
                {
                    if let Some(diff_split) =
                        drag::document_view_diff_split_selection_text(tree, id)
                    {
                        let group_key = (diff_split.left_id, diff_split.right_id);
                        if !handled_diff_split_pairs.insert(group_key) {
                            false
                        } else {
                            let mut context = ReadOnlyClipboardContext::new(
                                diff_split.selected_text.as_ref(),
                                Some((0, diff_split.selected_text.len())),
                                true,
                                false,
                            );
                            dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                        }
                    } else if let Some(shared) =
                        drag::document_view_shared_selection_text(tree, id, true)
                    {
                        let group_key = (shared.scroll_view_id, shared.shared_selection_id.clone());
                        if !handled_shared_groups.insert(group_key) {
                            false
                        } else {
                            let mut context = ReadOnlyClipboardContext::new(
                                shared.selected_text.as_ref(),
                                Some((0, shared.selected_text.len())),
                                true,
                                false,
                            );
                            dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                        }
                    } else {
                        let selected_text = selection_range(
                            node.selection_cursor,
                            node.selection_anchor,
                            node.visual_cache.flat_text.len(),
                        )
                        .and_then(|(start, end)| {
                            document_view_selected_text_from_node(node, start, end, true)
                        });
                        let selection = selected_text.as_ref().map(|text| (0, text.len()));
                        let text = selected_text.as_deref().unwrap_or("");

                        let mut context = if let Some(table_sel) = &node.table_rect_selection {
                            ReadOnlyClipboardContext::new(
                                table_sel.tsv_text.as_ref(),
                                Some((0, table_sel.tsv_text.len())),
                                true,
                                false,
                            )
                        } else {
                            ReadOnlyClipboardContext::new(text, selection, true, false)
                        };
                        dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                    }
                }
                #[cfg(not(feature = "diff-view"))]
                {
                    if let Some(shared) = drag::document_view_shared_selection_text(tree, id, true)
                    {
                        let group_key = (shared.scroll_view_id, shared.shared_selection_id.clone());
                        if !handled_shared_groups.insert(group_key) {
                            false
                        } else {
                            let mut context = ReadOnlyClipboardContext::new(
                                shared.selected_text.as_ref(),
                                Some((0, shared.selected_text.len())),
                                true,
                                false,
                            );
                            dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                        }
                    } else {
                        let selected_text = selection_range(
                            node.selection_cursor,
                            node.selection_anchor,
                            node.visual_cache.flat_text.len(),
                        )
                        .and_then(|(start, end)| {
                            document_view_selected_text_from_node(node, start, end, true)
                        });
                        let selection = selected_text.as_ref().map(|text| (0, text.len()));
                        let text = selected_text.as_deref().unwrap_or("");

                        let mut context = if let Some(table_sel) = &node.table_rect_selection {
                            ReadOnlyClipboardContext::new(
                                table_sel.tsv_text.as_ref(),
                                Some((0, table_sel.tsv_text.len())),
                                true,
                                false,
                            )
                        } else {
                            ReadOnlyClipboardContext::new(text, selection, true, false)
                        };
                        dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
                    }
                }
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(node) => {
                let Some(selection) = node
                    .selection
                    .as_ref()
                    .filter(|selection| !selection.is_empty())
                else {
                    continue;
                };
                let text = terminal_selection_text(node.lines.as_ref(), selection);
                let mut context = ReadOnlyClipboardContext::new(
                    text.as_str(),
                    Some((0, text.len())),
                    true,
                    false,
                );
                dispatch_clipboard_with_feedback(key, &mut context, ctx, id)
            }
            _ => false,
        };

        if handled {
            return true;
        }
    }

    for node in tree.iter() {
        let NodeKind::ScrollView(_) = &node.kind else {
            continue;
        };
        let Some(selected_text) =
            drag::scroll_view_offscreen_document_selection_text(tree, node.id, true)
        else {
            continue;
        };

        let mut context = ReadOnlyClipboardContext::new(
            selected_text.as_ref(),
            Some((0, selected_text.len())),
            true,
            false,
        );
        if dispatch_clipboard_with_feedback(key, &mut context, ctx, node.id) {
            return true;
        }
    }

    false
}

/// Dispatch a key event to the focused node.
/// Returns true if the event was handled.
pub(crate) fn dispatch_key(
    tree: &mut NodeTree,
    focused: Option<NodeId>,
    key: KeyEvent,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let Some(id) = focused else {
        return false;
    };

    if !tree.is_valid(id) {
        return false;
    }

    // Classify the node before taking a &mut borrow so the immutable
    // reference is dropped before we call into the handler.
    let tag = handlers::classify_interactive(&tree.node(id).kind);
    let rect = tree.node(id).rect;

    let handled = match tag {
        handlers::InteractiveTag::Button => handlers::button::handle_key(tree, id, key),
        handlers::InteractiveTag::Checkbox => handlers::checkbox::handle_key(tree, id, key),
        handlers::InteractiveTag::Graph => handlers::graph::handle_key(tree, id, key),
        handlers::InteractiveTag::Input => handlers::input_widget::handle_key(tree, id, key, ctx),
        handlers::InteractiveTag::HexArea => handlers::hex_area::handle_key(tree, id, key, ctx),
        handlers::InteractiveTag::List => handlers::list_table::handle_list_key(tree, id, key),
        handlers::InteractiveTag::Table => {
            handlers::list_table::handle_table_key(tree, id, key, rect)
        }
        handlers::InteractiveTag::TextArea => handlers::text_area::handle_key(tree, id, key, ctx),
        #[cfg(feature = "terminal")]
        handlers::InteractiveTag::Terminal => {
            handlers::terminal::handle_key(tree, id, key, ctx.clipboard, ctx.clipboard_config)
        }
        handlers::InteractiveTag::PanView => handlers::pan_view::handle_key(tree, id, &key),
        handlers::InteractiveTag::ScrollView => handlers::scroll_view::handle_key(tree, id, &key),
        handlers::InteractiveTag::Tabs | handlers::InteractiveTag::DraggableTabBar => {
            handlers::tabs::handle_key(tree, id, key)
        }
        handlers::InteractiveTag::DocumentView => {
            handlers::document_view::handle_key(tree, id, key, ctx)
        }
        handlers::InteractiveTag::NonInteractive => false,
    };

    if handled {
        return true;
    }

    // Bubble unhandled keys to ancestor ScrollView nodes.
    let mut cur = tree.node(id).parent;
    while let Some(pid) = cur {
        if !tree.is_valid(pid) {
            break;
        }
        if handlers::pan_view::handle_key(tree, pid, &key)
            || handlers::scroll_view::handle_key(tree, pid, &key)
        {
            return true;
        }
        cur = tree.node(pid).parent;
    }

    false
}

pub(crate) fn dispatch_paste(
    tree: &mut NodeTree,
    focused: Option<NodeId>,
    text: &str,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let Some(id) = focused else {
        return false;
    };

    if !tree.is_valid(id) {
        return false;
    }

    match handlers::classify_interactive(&tree.node(id).kind) {
        handlers::InteractiveTag::Input => {
            handlers::input_widget::handle_paste(tree, id, text, ctx)
        }
        handlers::InteractiveTag::TextArea => {
            handlers::text_area::handle_paste(tree, id, text, ctx)
        }
        #[cfg(feature = "terminal")]
        handlers::InteractiveTag::Terminal => handlers::terminal::handle_paste(tree, id, text),
        _ => false,
    }
}

pub(crate) fn dispatch_ambient_page_scroll(tree: &mut NodeTree, key: KeyEvent) -> bool {
    if !matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
        return false;
    }

    let mut target = None;
    for node in tree.iter() {
        let NodeKind::ScrollView(scroll) = &node.kind else {
            continue;
        };
        if !scroll.ambient_page_scroll {
            continue;
        }
        if target.replace(node.id).is_some() {
            return false;
        }
    }

    let Some(target) = target else {
        return false;
    };

    let can_scroll = match (&tree.node(target).kind, key.code) {
        (NodeKind::ScrollView(scroll), KeyCode::PageUp) => scroll.offset > 0,
        (NodeKind::ScrollView(scroll), KeyCode::PageDown) => scroll.offset < scroll.max_offset,
        _ => false,
    };

    can_scroll && handlers::scroll_view::handle_key(tree, target, &key)
}

#[cfg(test)]
mod tests {
    use super::dispatch_selection_clipboard_shortcut;
    use crate::app::context::TextAreaNewlineBinding;
    use crate::app::input::handlers::KeyCtx;
    use crate::app::input::handlers::text_area::{
        test_effective_text_area_newline_binding, test_text_area_should_block_enter,
        test_text_area_should_insert_newline,
    };
    use crate::app::input::keymap::{
        Action, BindingMode, Keymap, binding_for_test, keymap_for_test,
    };
    use crate::callback::Callback;
    use crate::clipboard::error::ClipboardOperation;
    use crate::clipboard::{ClipboardConfig, ClipboardError, ClipboardProvider, ClipboardService};
    use crate::core::element::{Element, IntoElement};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::core::node::{NodeId, NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect};
    use crate::widgets::{DocumentView, Input, ScrollView, TextArea, TextAreaEvent};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Default)]
    struct RecordingClipboard {
        writes: Rc<RefCell<Vec<String>>>,
    }

    impl ClipboardProvider for RecordingClipboard {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            Err(ClipboardError::unsupported(
                ClipboardOperation::ReadClipboard,
            ))
        }

        fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.writes.borrow_mut().push(text.to_string());
            Ok(())
        }
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn ctrl_insert() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Insert,
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn ctrl_x() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('x'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn dispatch_selection_shortcut(
        tree: &mut NodeTree,
        read_only_selection: Option<&HashMap<NodeId, (usize, Option<usize>)>>,
        keymap: &Keymap,
        clipboard: &ClipboardService,
        clipboard_config: &ClipboardConfig,
        key: KeyEvent,
    ) -> bool {
        let mut input_history = HashMap::new();
        let mut textarea_history = HashMap::new();
        let mut text_area_vim_state = HashMap::new();
        let mut hex_history = HashMap::new();
        let mut hex_pending_edit = HashMap::new();
        let mut copy_feedback = crate::app::copy_feedback::CopyFeedbackState::default();
        let mut ctx = KeyCtx {
            read_only_selection,
            input_history: &mut input_history,
            textarea_history: &mut textarea_history,
            text_area_vim_state: &mut text_area_vim_state,
            hex_history: &mut hex_history,
            hex_pending_edit: &mut hex_pending_edit,
            keymap,
            text_area_newline_binding: TextAreaNewlineBinding::default(),
            clipboard,
            clipboard_config,
            copy_feedback: &mut copy_feedback,
            dirty_override: None,
        };

        dispatch_selection_clipboard_shortcut(tree, key, &mut ctx)
    }

    fn enter_key(mods: KeyMods) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Enter,
            mods,
        }
    }

    #[test]
    fn ctrl_c_copies_selected_non_focusable_input() {
        let root = Input::new("hello world")
            .cursor(5)
            .anchor(Some(0))
            .focusable(false)
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
            None,
        );

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["hello"]);
    }

    #[test]
    fn ctrl_c_copies_read_only_selection_snapshot() {
        let root = Input::new("hello world").focusable(false).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
            None,
        );

        let input_id = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Input(_)))
            .map(|node| node.id)
            .expect("input exists");
        let read_only_selection = HashMap::from([(input_id, (5, Some(0)))]);

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            Some(&read_only_selection),
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["hello"]);
    }

    #[test]
    fn ctrl_c_copies_selected_non_focusable_text_area() {
        let root = TextArea::new("alpha beta")
            .cursor(5)
            .anchor(Some(0))
            .focusable(false)
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["alpha"]);
    }

    #[test]
    fn ctrl_c_copies_selected_non_focusable_document_view() {
        let root = DocumentView::new("hello world").focusable(false).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let doc_id = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .expect("document view exists");
        if let NodeKind::DocumentView(doc) = &mut tree.node_mut(doc_id).kind {
            doc.selection_cursor = 5;
            doc.selection_anchor = Some(0);
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["hello"]);
    }

    #[test]
    fn ctrl_insert_copies_selected_non_focusable_text_area() {
        let root = TextArea::new("alpha beta")
            .cursor(5)
            .anchor(Some(0))
            .focusable(false)
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-insert",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_insert(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["alpha"]);
    }

    #[test]
    fn ctrl_insert_copies_selected_non_focusable_document_view() {
        let root = DocumentView::new("hello world").focusable(false).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let doc_id = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .expect("document view exists");
        if let NodeKind::DocumentView(doc) = &mut tree.node_mut(doc_id).kind {
            doc.selection_cursor = 5;
            doc.selection_anchor = Some(0);
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-insert",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_insert(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["hello"]);
    }

    #[test]
    fn ctrl_x_cuts_selected_editable_text_area() {
        let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
        let changes_ref = changes.clone();
        let on_change = Callback::new(move |event: TextAreaEvent| {
            changes_ref.borrow_mut().push(event);
        });
        let root = TextArea::new("alpha beta")
            .cursor(5)
            .anchor(Some(0))
            .focusable(false)
            .on_change(on_change)
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-x",
            Action::Cut,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_x(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["alpha"]);

        let changes = changes.borrow();
        let event = changes.last().expect("cut emits TextAreaEvent");
        assert_eq!(event.value.as_ref(), " beta");
        assert_eq!(event.cursor, 0);
        assert_eq!(event.anchor, None);
    }

    #[test]
    fn ctrl_c_copies_shared_document_view_selection_in_group_order() {
        let root = crate::widgets::ScrollView::new()
            .children([
                DocumentView::new("alpha")
                    .focusable(false)
                    .shared_selection_id("shared-docs")
                    .into(),
                DocumentView::new("beta")
                    .focusable(false)
                    .shared_selection_id("shared-docs")
                    .into(),
                DocumentView::new("gamma")
                    .focusable(false)
                    .shared_selection_id("other-group")
                    .into(),
            ])
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 8,
            },
            None,
        );

        let doc_ids: Vec<_> = tree
            .iter()
            .filter(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .collect();
        assert_eq!(doc_ids.len(), 3, "expected three document views");

        let mut shared_doc_ids = Vec::new();
        for id in doc_ids {
            let NodeKind::DocumentView(doc) = &tree.node(id).kind else {
                continue;
            };
            if let Some("shared-docs") = doc.shared_selection_id.as_deref() {
                shared_doc_ids.push(id);
            }
        }
        assert_eq!(shared_doc_ids.len(), 2, "expected two shared group docs");

        for id in &shared_doc_ids {
            if let NodeKind::DocumentView(doc) = &mut tree.node_mut(*id).kind {
                doc.selection_cursor = doc.visual_cache.flat_text.len();
                doc.selection_anchor = Some(0);
            }
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));

        let copied = writes.borrow().clone();
        assert_eq!(copied.as_slice(), &["alpha\n\nbeta".to_string()]);
    }

    #[test]
    fn ctrl_c_copies_offscreen_document_view_selection() {
        fn root(offset: usize) -> Element {
            ScrollView::new()
                .offset(offset)
                .children((0..12).map(|i| {
                    DocumentView::new(format!("row {i}"))
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .focusable(false)
                        .height(Length::Auto)
                        .shared_selection_id("shared-docs")
                        .key(format!("row-{i}"))
                }))
                .into()
        }

        let mut tree = NodeTree::new();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &root(0), viewport, None);

        let first_doc_id = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .expect("first document view exists");
        if let NodeKind::DocumentView(doc) = &mut tree.node_mut(first_doc_id).kind {
            doc.selection_cursor = doc.visual_cache.flat_text.len();
            doc.selection_anchor = Some(0);
        }

        LayoutEngine::reconcile_with_focus(&mut tree, &root(4), viewport, None);
        assert!(tree.iter().all(|node| {
            if let NodeKind::DocumentView(doc) = &node.kind {
                doc.selection_anchor.is_none()
            } else {
                true
            }
        }));

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["row 0"]);
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn ctrl_c_copies_split_diff_document_selection_as_tsv_rows() {
        let root: Element = crate::widgets::DiffView::with_content(
            "fn old() {}\nlet before = 1;\n",
            "fn new() {}\nlet after = 2;\n",
        )
        .backend(crate::widgets::DiffViewBackend::DocumentView)
        .document_view(DocumentView::new("").scroll_wheel(false))
        .mode(crate::widgets::DiffViewMode::Split)
        .line_numbers(false)
        .wrap(true)
        .scrollbar(false)
        .h_scrollbar(false)
        .focusable(false)
        .border(false)
        .panels_border(false)
        .height(Length::Auto)
        .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 60,
                h: 6,
            },
            None,
        );

        let mut left_id = None;
        let mut right_id = None;
        for node in tree.iter() {
            let NodeKind::DocumentView(doc) = &node.kind else {
                continue;
            };
            match doc.diff_split_pane {
                Some(crate::widgets::DiffPane::Left) => left_id = Some(node.id),
                Some(crate::widgets::DiffPane::Right) => right_id = Some(node.id),
                _ => {}
            }
        }
        for id in [
            left_id.expect("left diff pane exists"),
            right_id.expect("right diff pane exists"),
        ] {
            if let NodeKind::DocumentView(doc) = &mut tree.node_mut(id).kind {
                doc.selection_anchor = Some(0);
                doc.selection_cursor = doc.visual_cache.line_lengths[0];
            }
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["fn old() {}\tfn new() {}"]);
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn ctrl_c_copies_split_diff_selection_by_logical_rows_when_wrapped() {
        let root: Element = crate::widgets::DiffView::with_content(
            "fn default_diff(before: &str, after: &str) -> DiffView {",
            "fn default_diff(before: &str, after: &str) -> DiffView {",
        )
        .backend(crate::widgets::DiffViewBackend::DocumentView)
        .document_view(DocumentView::new("").scroll_wheel(false))
        .mode(crate::widgets::DiffViewMode::Split)
        .line_numbers(false)
        .wrap(true)
        .scrollbar(false)
        .h_scrollbar(false)
        .focusable(false)
        .border(false)
        .panels_border(false)
        .height(Length::Auto)
        .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 22,
                h: 8,
            },
            None,
        );

        let mut left_id = None;
        let mut right_id = None;
        for node in tree.iter() {
            let NodeKind::DocumentView(doc) = &node.kind else {
                continue;
            };
            match doc.diff_split_pane {
                Some(crate::widgets::DiffPane::Left) => left_id = Some(node.id),
                Some(crate::widgets::DiffPane::Right) => right_id = Some(node.id),
                _ => {}
            }
        }
        for id in [
            left_id.expect("left diff pane exists"),
            right_id.expect("right diff pane exists"),
        ] {
            if let NodeKind::DocumentView(doc) = &mut tree.node_mut(id).kind {
                let last_visual = doc
                    .visual_cache
                    .source_line_map
                    .iter()
                    .rposition(|source_line| *source_line == 0)
                    .expect("wrapped logical row should remain visible");
                doc.selection_anchor = Some(0);
                doc.selection_cursor = doc.visual_cache.line_starts[last_visual]
                    .saturating_add(doc.visual_cache.line_lengths[last_visual]);
            }
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(
            writes.borrow().as_slice(),
            &[
                "fn default_diff(before: &str, after: &str) -> DiffView {\tfn default_diff(before: &str, after: &str) -> DiffView {"
            ]
        );
    }

    #[test]
    fn ctrl_c_without_selection_falls_through() {
        let root = Input::new("hello").focusable(false).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
            None,
        );

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(!dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert!(writes.borrow().is_empty());
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn ctrl_c_copies_terminal_selection() {
        let root = crate::widgets::Terminal::new().focusable(false).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 4,
            },
            None,
        );

        let terminal_id = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Terminal(_)))
            .map(|node| node.id)
            .expect("terminal exists");
        if let NodeKind::Terminal(term) = &mut tree.node_mut(terminal_id).kind {
            term.lines = vec![vec![crate::style::Span::new("hello")]].into();
            let mut selection =
                crate::utils::selection::GridSelection::new(crate::utils::selection::GridPos {
                    row: 0,
                    col: 0,
                });
            selection.extend_to(crate::utils::selection::GridPos { row: 0, col: 5 });
            term.selection = Some(selection);
        }

        let writes = Rc::new(RefCell::new(Vec::new()));
        let clipboard = ClipboardService::new(
            Box::new(RecordingClipboard {
                writes: writes.clone(),
            }),
            Rc::new(|_| {}),
        );
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )]);

        assert!(dispatch_selection_shortcut(
            &mut tree,
            None,
            &keymap,
            &clipboard,
            &ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            },
            ctrl_c(),
        ));
        assert_eq!(writes.borrow().as_slice(), &["hello"]);
    }

    #[test]
    fn shift_enter_inserts_newline_when_enabled() {
        let shift_enter = enter_key(KeyMods {
            shift: true,
            ..KeyMods::default()
        });

        assert!(test_text_area_should_insert_newline(
            shift_enter,
            TextAreaNewlineBinding::ShiftEnter
        ));
        assert!(test_text_area_should_insert_newline(
            shift_enter,
            TextAreaNewlineBinding::EnterOrShiftEnter
        ));
    }

    #[test]
    fn plain_enter_is_blocked_in_shift_enter_mode() {
        let enter = enter_key(KeyMods::default());

        assert!(test_text_area_should_block_enter(
            enter,
            TextAreaNewlineBinding::ShiftEnter
        ));
        assert!(!test_text_area_should_block_enter(
            enter,
            TextAreaNewlineBinding::EnterOrShiftEnter
        ));
        assert!(!test_text_area_should_block_enter(
            enter,
            TextAreaNewlineBinding::Enter
        ));
    }

    #[test]
    fn widget_newline_binding_overrides_app_binding() {
        assert_eq!(
            test_effective_text_area_newline_binding(
                Some(TextAreaNewlineBinding::Enter),
                TextAreaNewlineBinding::ShiftEnter,
            ),
            TextAreaNewlineBinding::Enter
        );

        assert_eq!(
            test_effective_text_area_newline_binding(None, TextAreaNewlineBinding::ShiftEnter),
            TextAreaNewlineBinding::ShiftEnter
        );
    }
}
