use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::visual_nav::perform_visual_vertical_nav;
use super::{handle_key, handle_paste, handle_scroll};
use crate::animation::{Easing, TransitionConfig};
use crate::app::context::TextAreaNewlineBinding;
use crate::app::input::handlers::KeyCtx;
use crate::app::input::hex_history::HexHistory;
use crate::app::input::keymap::{Action, Keymap, KeymapConfig};
use crate::app::input::text_area_vim::{TextAreaVimPending, TextAreaVimState, VimOperator};
use crate::app::interaction_state::{DirtyLevel, HexPendingEdit};
use crate::callback::{Callback, KeyHandler};
use crate::clipboard::{
    ClipboardConfig, ClipboardProvider, ClipboardService, ImageContent, ImageFormat,
};
use crate::core::event::{KeyCode, KeyEvent, KeyMods};
use crate::core::node::{NodeKind, NodeTree};
use crate::input::KeyBindings;
use crate::layout::LayoutEngine;
use crate::style::{Rect, Span};
use crate::text::edit::{TextEditEvent, TextEditKind};
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;
use crate::widgets::internal::ScrollAction;
use crate::widgets::{
    ScrollBehavior, TextArea, TextAreaEvent, TextAreaImageMode, TextAreaPasteEvent,
    TextAreaSentinel, TextAreaVimKeymap, TextAreaVimMode, TextAreaVirtualText, TextAreaVisualLine,
    text_area_visual_line_for_cursor,
};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

fn ctrl_char(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    }
}

fn linear_smooth() -> ScrollBehavior {
    ScrollBehavior::smooth(TransitionConfig {
        duration: Duration::from_millis(100),
        easing: Easing::Linear,
    })
}

fn numbered_lines(count: usize) -> String {
    (0..count)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

struct TestClipboardProvider;

impl ClipboardProvider for TestClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, crate::clipboard::ClipboardError> {
        Ok(String::new())
    }

    fn write_clipboard_text(
        &mut self,
        _text: &str,
    ) -> Result<(), crate::clipboard::ClipboardError> {
        Ok(())
    }
}

struct StaticClipboardProvider(&'static str);

impl ClipboardProvider for StaticClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, crate::clipboard::ClipboardError> {
        Ok(self.0.to_string())
    }

    fn write_clipboard_text(
        &mut self,
        _text: &str,
    ) -> Result<(), crate::clipboard::ClipboardError> {
        Ok(())
    }
}

struct ImageClipboardProvider(ImageContent);

impl ClipboardProvider for ImageClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, crate::clipboard::ClipboardError> {
        Ok(String::new())
    }

    fn write_clipboard_text(
        &mut self,
        _text: &str,
    ) -> Result<(), crate::clipboard::ClipboardError> {
        Ok(())
    }

    fn read_clipboard_image(&mut self) -> Result<ImageContent, crate::clipboard::ClipboardError> {
        Ok(self.0.clone())
    }
}

struct CapturingClipboardProvider {
    text: Rc<RefCell<String>>,
}

impl ClipboardProvider for CapturingClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, crate::clipboard::ClipboardError> {
        Ok(self.text.borrow().clone())
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), crate::clipboard::ClipboardError> {
        *self.text.borrow_mut() = text.to_string();
        Ok(())
    }
}

struct FailingClipboardProvider;

impl ClipboardProvider for FailingClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, crate::clipboard::ClipboardError> {
        Ok(String::new())
    }

    fn write_clipboard_text(
        &mut self,
        _text: &str,
    ) -> Result<(), crate::clipboard::ClipboardError> {
        Err(crate::clipboard::ClipboardError::provider(
            crate::clipboard::error::ClipboardOperation::WriteClipboard,
            "failed",
        ))
    }
}

fn reconcile_text_area(root: crate::core::element::Element) -> NodeTree {
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        },
        None,
    );
    tree
}

fn apply_last_text_area_event(tree: &mut NodeTree, event: &TextAreaEvent) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(tree.root).kind {
        ta.value = Arc::clone(&event.value);
        ta.cursor = event.cursor;
        ta.anchor = event.anchor;
    }
}

fn apply_last_text_area_sentinels(tree: &mut NodeTree, sentinels: &[TextAreaSentinel]) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(tree.root).kind {
        ta.sentinels = sentinels.to_vec();
    }
}

fn apply_last_text_area_images(tree: &mut NodeTree, images: &[ImageContent]) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(tree.root).kind {
        ta.images = images.to_vec();
    }
}

#[allow(clippy::too_many_arguments)]
fn default_key_ctx<'a>(
    read_only_selection: &'a HashMap<crate::core::node::NodeId, (usize, Option<usize>)>,
    input_history: &'a mut HashMap<crate::core::node::NodeId, TextInput>,
    textarea_history: &'a mut HashMap<crate::core::node::NodeId, TextEditor>,
    text_area_vim_state: &'a mut HashMap<
        crate::core::node::NodeId,
        crate::app::input::text_area_vim::TextAreaVimState,
    >,
    hex_history: &'a mut HashMap<crate::core::node::NodeId, HexHistory>,
    hex_pending_edit: &'a mut HashMap<crate::core::node::NodeId, HexPendingEdit>,
    keymap: &'a Keymap,
    clipboard: &'a ClipboardService,
    clipboard_config: &'a ClipboardConfig,
) -> KeyCtx<'a> {
    KeyCtx {
        read_only_selection: Some(read_only_selection),
        input_history,
        textarea_history,
        text_area_vim_state,
        hex_history,
        hex_pending_edit,
        keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard,
        clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    }
}

#[test]
fn vim_disabled_still_inserts_plain_characters() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("")
            .vim_motions(false)
            .on_change(on_change)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('x')),
        &mut ctx
    ));
    assert_eq!(changes.borrow()[0].value.as_ref(), "x");
}

#[test]
fn vim_insert_typing_then_escape_changes_mode_without_text_event() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let edits = Rc::new(RefCell::new(Vec::<TextEditEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("")
            .vim_motions(true)
            .on_change(on_change)
            .on_edit(on_edit)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('i')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('x')),
        &mut ctx
    ));
    assert_eq!(changes.borrow()[0].value.as_ref(), "x");
    assert_eq!(edits.borrow().len(), 1);

    assert!(handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));
    assert_eq!(
        modes.borrow().as_slice(),
        [TextAreaVimMode::Insert, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.borrow().len(), 1);
    assert_eq!(edits.borrow().len(), 1);
}

#[test]
fn vim_default_state_starts_in_normal_mode() {
    assert_eq!(TextAreaVimMode::default(), TextAreaVimMode::Normal);
    assert_eq!(TextAreaVimState::default().mode, TextAreaVimMode::Normal);
}

#[test]
fn vim_insert_ctrl_z_and_ctrl_y_are_consumed_without_mutation() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('i')),
        &mut ctx
    ));
    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert!(handle_key(&mut tree, root, ctrl_char('y'), &mut ctx));
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_normal_u_and_ctrl_r_perform_undo_redo() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("")
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('i')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('x')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    assert!(handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('u')),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "");
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    assert!(handle_key(&mut tree, root, ctrl_char('r'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "x");
}

#[test]
fn vim_normal_yy_yanks_current_logical_line() {
    let copied = Rc::new(RefCell::new(String::new()));
    let mut tree = reconcile_text_area(
        TextArea::new("one\ntwo\nthree")
            .cursor(4)
            .vim_motions(true)
            .on_change(Callback::new(|_| {}))
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(
        Box::new(CapturingClipboardProvider {
            text: Rc::clone(&copied),
        }),
        Rc::new(|_| {}),
    );
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));
    assert_eq!(copied.borrow().as_str(), "two\n");
    assert_eq!(ctx.dirty_override, Some(DirtyLevel::PaintOnly));
    assert!(ctx.copy_feedback.is_active(root));
    assert_eq!(
        ctx.text_area_vim_state
            .get(&root)
            .unwrap()
            .yank_feedback_range,
        Some((4, 8))
    );
}

#[test]
fn vim_normal_yy_does_not_flash_when_clipboard_write_fails() {
    let mut tree = reconcile_text_area(
        TextArea::new("one\ntwo\nthree")
            .cursor(4)
            .vim_motions(true)
            .on_change(Callback::new(|_| {}))
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(FailingClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig {
        enable_osc52: false,
        enable_primary_selection: false,
        ..Default::default()
    };
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));

    assert_eq!(ctx.dirty_override, None);
    assert!(!ctx.copy_feedback.is_active(root));
    assert_eq!(
        ctx.text_area_vim_state
            .get(&root)
            .unwrap()
            .yank_feedback_range,
        None
    );
}

#[test]
fn vim_visual_y_yanks_selection_and_exits_visual() {
    let copied = Rc::new(RefCell::new(String::new()));
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(
        Box::new(CapturingClipboardProvider {
            text: Rc::clone(&copied),
        }),
        Rc::new(|_| {}),
    );
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('l')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    changes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));
    assert_eq!(copied.borrow().as_str(), "a");
    assert_eq!(
        ctx.text_area_vim_state.get(&root).unwrap().mode,
        TextAreaVimMode::Normal
    );
    assert_eq!(changes.borrow().last().unwrap().anchor, None);
    assert_eq!(ctx.dirty_override, Some(DirtyLevel::PaintOnly));
    assert!(ctx.copy_feedback.is_active(root));
    assert_eq!(
        ctx.text_area_vim_state
            .get(&root)
            .unwrap()
            .yank_feedback_range,
        Some((0, 1))
    );
    let NodeKind::TextArea(ta) = &tree.node(root).kind else {
        panic!("expected text area");
    };
    assert_eq!(ta.anchor, None);
    assert_eq!(ta.vim_yank_feedback_range, Some((0, 1)));
}

#[test]
fn vim_visual_line_y_yanks_whole_line_selection_and_exits_visual() {
    let copied = Rc::new(RefCell::new(String::new()));
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("aa\nbb")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(
        Box::new(CapturingClipboardProvider {
            text: Rc::clone(&copied),
        }),
        Rc::new(|_| {}),
    );
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('V')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    changes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('y')),
        &mut ctx
    ));
    assert_eq!(copied.borrow().as_str(), "aa\n");
    assert_eq!(
        ctx.text_area_vim_state.get(&root).unwrap().mode,
        TextAreaVimMode::Normal
    );
    assert_eq!(changes.borrow().last().unwrap().anchor, None);
}

#[test]
fn vim_normal_p_pastes_after_cursor_character() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("X")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('p')),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "abXc");
}

#[test]
fn vim_visual_p_replaces_selection_and_exits_visual() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("X")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('l')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    changes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('p')),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "Xbc");
    assert_eq!(
        ctx.text_area_vim_state.get(&root).unwrap().mode,
        TextAreaVimMode::Normal
    );
}

#[test]
fn vim_visual_p_with_empty_clipboard_exits_and_clears_selection() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('l')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    changes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('p')),
        &mut ctx
    ));

    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "abc");
    assert_eq!(changes.borrow().last().unwrap().anchor, None);
    assert_eq!(
        ctx.text_area_vim_state.get(&root).unwrap().mode,
        TextAreaVimMode::Normal
    );
}

#[test]
fn vim_visual_line_p_replaces_whole_line_selection_and_exits_visual() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("aa\nbb")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("X")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('V')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    changes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('p')),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "Xbb");
    assert_eq!(
        ctx.text_area_vim_state.get(&root).unwrap().mode,
        TextAreaVimMode::Normal
    );
}

fn vim_motion_result(value: &str, cursor: usize, keys: &[KeyEvent]) -> Vec<TextAreaEvent> {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let edits = Rc::new(RefCell::new(Vec::<TextEditEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new(value)
            .cursor(cursor)
            .vim_motions(true)
            .on_change(on_change)
            .on_edit(on_edit)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    changes.borrow_mut().clear();
    edits.borrow_mut().clear();
    for key_event in keys {
        assert!(handle_key(&mut tree, root, *key_event, &mut ctx));
    }
    assert!(edits.borrow().is_empty());
    changes.borrow().clone()
}

fn vim_visual_result(
    value: &str,
    cursor: usize,
    keys: &[KeyEvent],
) -> (
    Vec<TextAreaEvent>,
    Vec<TextEditEvent>,
    Vec<TextAreaVimMode>,
    Option<usize>,
) {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let edits = Rc::new(RefCell::new(Vec::<TextEditEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new(value)
            .cursor(cursor)
            .vim_motions(true)
            .on_change(on_change)
            .on_edit(on_edit)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    changes.borrow_mut().clear();
    edits.borrow_mut().clear();
    modes.borrow_mut().clear();
    for key_event in keys {
        let before_changes = changes.borrow().len();
        assert!(handle_key(&mut tree, root, *key_event, &mut ctx));
        let latest = changes.borrow().last().cloned();
        if changes.borrow().len() != before_changes
            && let Some(event) = latest
        {
            let on_change = {
                let changes = Rc::clone(&changes);
                Callback::new(move |event| changes.borrow_mut().push(event))
            };
            let on_edit = {
                let edits = Rc::clone(&edits);
                Callback::new(move |event| edits.borrow_mut().push(event))
            };
            let on_mode = {
                let modes = Rc::clone(&modes);
                Callback::new(move |mode| modes.borrow_mut().push(mode))
            };
            let rerendered: crate::core::element::Element = TextArea::new(value)
                .cursor(event.cursor)
                .anchor(event.anchor)
                .vim_motions(true)
                .on_change(on_change)
                .on_edit(on_edit)
                .on_vim_mode_change(on_mode)
                .into();
            LayoutEngine::reconcile_with_focus(
                &mut tree,
                &rerendered,
                Rect {
                    x: 0,
                    y: 0,
                    w: 40,
                    h: 8,
                },
                None,
            );
        }
    }
    let visual_anchor = ctx
        .text_area_vim_state
        .get(&root)
        .and_then(|s| s.visual_anchor);
    (
        changes.borrow().clone(),
        edits.borrow().clone(),
        modes.borrow().clone(),
        visual_anchor,
    )
}

fn vim_edit_result(
    value: &str,
    cursor: usize,
    keys: &[KeyEvent],
    clipboard_text: &str,
) -> (
    Vec<TextAreaEvent>,
    Vec<TextEditEvent>,
    Vec<TextAreaVimMode>,
    String,
    TextAreaVimState,
) {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let edits = Rc::new(RefCell::new(Vec::<TextEditEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new(value)
            .cursor(cursor)
            .vim_motions(true)
            .on_change(on_change)
            .on_edit(on_edit)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let clipboard_value = Rc::new(RefCell::new(clipboard_text.to_string()));
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(
        Box::new(CapturingClipboardProvider {
            text: Rc::clone(&clipboard_value),
        }),
        Rc::new(|_| {}),
    );
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    for key_event in keys {
        let before = changes.borrow().len();
        assert!(handle_key(&mut tree, root, *key_event, &mut ctx));
        if changes.borrow().len() != before {
            let event = changes.borrow().last().cloned().unwrap();
            apply_last_text_area_event(&mut tree, &event);
        }
    }
    let state = ctx
        .text_area_vim_state
        .get(&root)
        .cloned()
        .unwrap_or_default();
    (
        changes.borrow().clone(),
        edits.borrow().clone(),
        modes.borrow().clone(),
        clipboard_value.borrow().clone(),
        state,
    )
}

#[test]
fn vim_normal_motions_move_cursor_without_text_edits() {
    let value = "one two\nthree four\nfive";
    let cases = [
        (8, vec![key(KeyCode::Char('h'))], 7),
        (7, vec![key(KeyCode::Char('l'))], 8),
        (8, vec![key(KeyCode::Char('w'))], 14),
        (14, vec![key(KeyCode::Char('b'))], 8),
        (8, vec![key(KeyCode::Char('e'))], 13),
        (14, vec![key(KeyCode::Char('0'))], 8),
        (8, vec![key(KeyCode::Char('$'))], 18),
        (
            19,
            vec![key(KeyCode::Char('g')), key(KeyCode::Char('g'))],
            0,
        ),
        (0, vec![key(KeyCode::Char('G'))], 19),
        (0, vec![key(KeyCode::Char('2')), key(KeyCode::Char('w'))], 8),
        (0, vec![key(KeyCode::Char('j'))], 8),
        (8, vec![key(KeyCode::Char('k'))], 0),
    ];

    for (start, keys, expected_cursor) in cases {
        let changes = vim_motion_result(value, start, &keys);
        assert_eq!(
            changes.last().map(|event| event.cursor),
            Some(expected_cursor),
            "motion from cursor {start} via {keys:?} should move to {expected_cursor}"
        );
        assert_eq!(changes.last().unwrap().value.as_ref(), value);
    }
}

#[test]
fn vim_normal_big_word_motions_treat_non_whitespace_runs_as_words() {
    let value = "open-code next foo.bar/baz tail";
    let cases = [
        (0, vec![key(KeyCode::Char('W'))], "open-code ".len()),
        ("open-code ".len(), vec![key(KeyCode::Char('B'))], 0),
        (0, vec![key(KeyCode::Char('E'))], "open-code".len()),
        (
            0,
            vec![key(KeyCode::Char('2')), key(KeyCode::Char('W'))],
            "open-code next ".len(),
        ),
    ];

    for (start, keys, expected_cursor) in cases {
        let changes = vim_motion_result(value, start, &keys);
        assert_eq!(
            changes.last().map(|event| event.cursor),
            Some(expected_cursor),
            "big WORD motion from cursor {start} via {keys:?} should move to {expected_cursor}"
        );
        assert_eq!(changes.last().unwrap().value.as_ref(), value);
    }
}

#[test]
fn vim_normal_unsupported_printable_keys_do_not_insert() {
    let changes = vim_motion_result("abc", 1, &[key(KeyCode::Char('z'))]);
    assert!(changes.is_empty());
}

#[test]
fn vim_normal_esc_bubbles_when_no_vim_state_needs_it() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(!handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));

    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
    assert!(state.count.is_none());
    assert!(state.pending.is_none());
    assert!(!state.search.visible);
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_normal_esc_consumes_when_clearing_count() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('2')),
        &mut ctx
    ));
    assert_eq!(ctx.text_area_vim_state.get(&root).unwrap().count, Some(2));

    assert!(handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));

    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert!(state.count.is_none());
    assert!(state.pending.is_none());
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_normal_enter_bubbles_without_inserting_newline() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(!handle_key(&mut tree, root, key(KeyCode::Enter), &mut ctx));

    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_modes_bubble_unhandled_non_char_editor_keys() {
    let ctrl_backspace = KeyEvent {
        code: KeyCode::Backspace,
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    };
    let cases = [
        (key(KeyCode::Backspace), Action::Backspace),
        (key(KeyCode::Delete), Action::Delete),
        (ctrl_backspace, Action::DeleteWordLeft),
        (key(KeyCode::Insert), Action::None),
        (key(KeyCode::PageUp), Action::None),
        (key(KeyCode::PageDown), Action::None),
    ];

    for mode in [
        TextAreaVimMode::Normal,
        TextAreaVimMode::Visual,
        TextAreaVimMode::VisualLine,
    ] {
        for (key_event, action) in cases {
            let mut editor = TextEditor::new("abc".to_string());
            editor.set_cursor(1);
            let mut state = TextAreaVimState {
                mode,
                visual_anchor: (mode == TextAreaVimMode::Visual).then_some(1),
                visual_line_head: (mode == TextAreaVimMode::VisualLine).then_some(1),
                ..TextAreaVimState::default()
            };

            let outcome = super::vim::dispatch_text_area_vim_key(
                &mut editor,
                &mut state,
                key_event,
                action,
                &super::vim::VimLayoutCtx {
                    wrap: false,
                    visual_lines: None,
                    sentinel: None,
                    tab_stop: 8,
                    virtual_texts: &[],
                },
            );

            assert_eq!(
                outcome,
                super::vim::VimKeyOutcome::Unhandled,
                "{mode:?} should bubble {key_event:?} / {action:?}"
            );
            assert_eq!(editor.text(), "abc");
            assert_eq!(editor.cursor(), 1);
        }
    }
}

#[test]
fn vim_normal_supported_arrow_motion_still_handles() {
    let mut editor = TextEditor::new("abc".to_string());
    editor.set_cursor(1);
    let mut state = TextAreaVimState::default();

    let outcome = super::vim::dispatch_text_area_vim_key(
        &mut editor,
        &mut state,
        key(KeyCode::Left),
        Action::MoveLeft,
        &super::vim::VimLayoutCtx {
            wrap: false,
            visual_lines: None,
            sentinel: None,
            tab_stop: 8,
            virtual_texts: &[],
        },
    );

    assert!(matches!(
        outcome,
        super::vim::VimKeyOutcome::EditorChanged { .. }
    ));
    assert_eq!(editor.cursor(), 0);
}

fn temp_keymap(contents: &str) -> Keymap {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("tui-lipan-text-area-keymap-{unique}.conf"));
    std::fs::write(&path, contents).expect("write test keymap");

    let keymap = Keymap::new(
        KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&path),
    );

    let _ = std::fs::remove_file(&path);
    keymap
}

#[test]
fn vim_normal_unhandled_ctrl_char_bubbles_without_editor_fallback() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = temp_keymap("clear = ctrl-l\n");
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(!handle_key(&mut tree, root, ctrl_char('l'), &mut ctx));

    assert!(changes.borrow().is_empty());
    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
}

#[test]
fn vim_visual_modes_unhandled_ctrl_char_bubbles_without_exiting() {
    for (enter_key, expected_mode) in [
        (key(KeyCode::Char('v')), TextAreaVimMode::Visual),
        (key(KeyCode::Char('V')), TextAreaVimMode::VisualLine),
    ] {
        let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
        let on_change = {
            let changes = Rc::clone(&changes);
            Callback::new(move |event| changes.borrow_mut().push(event))
        };
        let mut tree = reconcile_text_area(
            TextArea::new("abc")
                .cursor(1)
                .vim_motions(true)
                .on_change(on_change)
                .into(),
        );
        let read_only_selection = HashMap::new();
        let mut input_history = HashMap::new();
        let mut textarea_history = HashMap::new();
        let mut text_area_vim_state = HashMap::new();
        let mut hex_history = HashMap::new();
        let mut hex_pending_edit = HashMap::new();
        let keymap = Keymap::default();
        let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
        let clipboard_config = ClipboardConfig::default();
        let mut ctx = default_key_ctx(
            &read_only_selection,
            &mut input_history,
            &mut textarea_history,
            &mut text_area_vim_state,
            &mut hex_history,
            &mut hex_pending_edit,
            &keymap,
            &clipboard,
            &clipboard_config,
        );

        let root = tree.root;
        assert!(handle_key(&mut tree, root, enter_key, &mut ctx));
        assert_eq!(
            ctx.text_area_vim_state.get(&root).unwrap().mode,
            expected_mode
        );
        changes.borrow_mut().clear();

        assert!(!handle_key(&mut tree, root, ctrl_char('l'), &mut ctx));

        assert_eq!(
            ctx.text_area_vim_state.get(&root).unwrap().mode,
            expected_mode
        );
        assert!(changes.borrow().is_empty());
    }
}

#[test]
fn vim_pending_operator_unhandled_ctrl_char_bubbles_and_preserves_pending() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('d')),
        &mut ctx
    ));
    assert!(matches!(
        ctx.text_area_vim_state.get(&root).unwrap().pending.as_ref(),
        Some(TextAreaVimPending::Operator {
            op,
            count,
            g_pending,
        }) if *op == VimOperator::Delete && *count == 1 && !*g_pending
    ));

    assert!(!handle_key(&mut tree, root, ctrl_char('l'), &mut ctx));

    assert!(matches!(
        ctx.text_area_vim_state.get(&root).unwrap().pending.as_ref(),
        Some(TextAreaVimPending::Operator {
            op,
            count,
            g_pending,
        }) if *op == VimOperator::Delete && *count == 1 && !*g_pending
    ));
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_normal_delete_change_chars_open_line_and_repeat() {
    let (changes, edits, modes, clipboard, _state) = vim_edit_result(
        "one two\nthree",
        0,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('c')),
            key(KeyCode::Char('w')),
            key(KeyCode::Esc),
            key(KeyCode::Char('x')),
            key(KeyCode::Char('X')),
            key(KeyCode::Char('o')),
            key(KeyCode::Esc),
            key(KeyCode::Char('.')),
        ],
        "",
    );

    assert_eq!(changes[0].value.as_ref(), "two\nthree");
    assert_eq!(changes[1].value.as_ref(), "\nthree");
    assert_eq!(
        modes,
        vec![
            TextAreaVimMode::Insert,
            TextAreaVimMode::Normal,
            TextAreaVimMode::Insert,
            TextAreaVimMode::Normal
        ]
    );
    assert_eq!(changes.last().unwrap().value.as_ref(), "three\n\n");
    assert!(edits.len() >= 5);
    assert_eq!(clipboard, "\n");
}

#[test]
fn vim_big_word_operators_and_repeat_use_non_whitespace_runs() {
    let (changes, _edits, modes, clipboard, _state) = vim_edit_result(
        "open-code next",
        0,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('W')),
            key(KeyCode::Char('c')),
            key(KeyCode::Char('W')),
            key(KeyCode::Esc),
        ],
        "",
    );

    assert_eq!(changes[0].value.as_ref(), "next");
    assert_eq!(changes[1].value.as_ref(), "");
    assert_eq!(
        modes,
        vec![TextAreaVimMode::Insert, TextAreaVimMode::Normal]
    );
    assert_eq!(clipboard, "next");

    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "one-two three-four five",
        0,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('W')),
            key(KeyCode::Char('.')),
        ],
        "",
    );

    assert_eq!(changes[0].value.as_ref(), "three-four five");
    assert_eq!(changes.last().unwrap().value.as_ref(), "five");
}

#[test]
fn vim_normal_dd_deletes_line_yanks_and_p_paste_linewise() {
    let (changes, _edits, _modes, clipboard, _state) = vim_edit_result(
        "one\ntwo\nthree",
        4,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('d')),
            key(KeyCode::Char('p')),
        ],
        "",
    );

    assert_eq!(changes[0].value.as_ref(), "one\nthree");
    assert_eq!(clipboard, "two\n");
    assert_eq!(changes.last().unwrap().value.as_ref(), "one\nthree\ntwo\n");
}

#[test]
fn vim_text_objects_delete_inner_word_and_around_paragraph() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "alpha beta\n\ngamma delta\n",
        2,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('j')),
            key(KeyCode::Char('d')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('p')),
        ],
        "",
    );

    assert_eq!(changes[0].value.as_ref(), " beta\n\ngamma delta\n");
    assert_eq!(changes.last().unwrap().value.as_ref(), " beta\n\n");
}

#[test]
fn vim_text_object_counts_delete_multiple_words() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "alpha beta gamma delta",
        0,
        &[
            key(KeyCode::Char('2')),
            key(KeyCode::Char('d')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('w')),
        ],
        "",
    );

    assert_eq!(changes.last().unwrap().value.as_ref(), " gamma delta");
}

#[test]
fn vim_big_word_text_objects_accept_uppercase_w() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "open-code next",
        "open".len(),
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('W')),
        ],
        "",
    );

    assert_eq!(changes.last().unwrap().value.as_ref(), " next");

    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "open-code next",
        "open".len(),
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('W')),
        ],
        "",
    );

    assert_eq!(changes.last().unwrap().value.as_ref(), "next");
}

#[test]
fn vim_delimited_text_objects_work_at_opening_delimiter() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "\"hello\" tail",
        0,
        &[
            key(KeyCode::Char('d')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('"')),
        ],
        "",
    );

    assert_eq!(changes.last().unwrap().value.as_ref(), "\"\" tail");
}

#[test]
fn vim_registers_blackhole_numbered_and_paste_before_work() {
    let (changes, _edits, _modes, clipboard, state) = vim_edit_result(
        "one two three",
        0,
        &[
            key(KeyCode::Char('"')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('y')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('"')),
            key(KeyCode::Char('_')),
            key(KeyCode::Char('d')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('"')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('P')),
        ],
        "",
    );

    assert_eq!(clipboard, "");
    assert_eq!(changes.last().unwrap().value.as_ref(), "one one three");
    assert_eq!(state.registers.values.get(&'a').unwrap().text, "one ");
    assert!(!state.registers.values.contains_key(&'1'));
}

#[test]
fn vim_uppercase_register_appends_to_existing_named_register() {
    let (changes, _edits, _modes, _clipboard, state) = vim_edit_result(
        "one\ntwo\nthree",
        0,
        &[
            key(KeyCode::Char('"')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('y')),
            key(KeyCode::Char('y')),
            key(KeyCode::Char('j')),
            key(KeyCode::Char('"')),
            key(KeyCode::Char('A')),
            key(KeyCode::Char('y')),
            key(KeyCode::Char('y')),
            key(KeyCode::Char('G')),
            key(KeyCode::Char('"')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('P')),
        ],
        "",
    );

    assert_eq!(state.registers.values.get(&'a').unwrap().text, "one\ntwo\n");
    assert_eq!(
        changes.last().unwrap().value.as_ref(),
        "one\ntwo\none\ntwo\nthree"
    );
}

#[test]
fn vim_explicit_empty_register_does_not_fall_back_to_clipboard() {
    let (changes, _edits, _modes, clipboard, _state) = vim_edit_result(
        "abc",
        1,
        &[
            key(KeyCode::Char('"')),
            key(KeyCode::Char('b')),
            key(KeyCode::Char('p')),
        ],
        "CLIP",
    );

    assert!(changes.is_empty());
    assert_eq!(clipboard, "CLIP");
}

#[test]
fn vim_keymap_remaps_keys_to_canonical_commands() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let vim_keymap = TextAreaVimKeymap::new().bind(
        KeyBindings::from_str("ctrl+n").expect("binding parses"),
        'x',
    );
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .vim_motions(true)
            .vim_keymap(vim_keymap)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(&mut tree, root, ctrl_char('n'), &mut ctx));

    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "bc");
}

#[test]
fn vim_search_and_marks_jump_between_matches() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "alpha beta alpha\n  gamma",
        0,
        &[
            key(KeyCode::Char('m')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('/')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('l')),
            key(KeyCode::Char('p')),
            key(KeyCode::Char('h')),
            key(KeyCode::Char('a')),
            key(KeyCode::Enter),
            key(KeyCode::Char('\'')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('`')),
            key(KeyCode::Char('`')),
            key(KeyCode::Char('n')),
        ],
        "",
    );

    assert_eq!(changes[0].cursor, 11);
    assert_eq!(changes[1].cursor, 0);
    assert_eq!(changes[2].cursor, 11);
    assert_eq!(changes.last().unwrap().cursor, 0);
}

#[test]
fn vim_pending_search_passes_through_global_quit_shortcut() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("alpha")
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('/')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('a')),
        &mut ctx
    ));
    assert!(!handle_key(&mut tree, root, ctrl_char('q'), &mut ctx));

    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert!(matches!(
        &state.pending,
        Some(TextAreaVimPending::Search { forward: true, query, cursor: 1 }) if query == "a"
    ));
    assert!(changes.borrow().is_empty());
}

#[test]
fn vim_pending_search_edits_query_at_cursor_with_arrow_navigation() {
    let (_changes, _edits, _modes, _clipboard, state) = vim_edit_result(
        "alpha beta alpha",
        0,
        &[
            key(KeyCode::Char('/')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('c')),
            key(KeyCode::Left),
            key(KeyCode::Char('b')),
        ],
        "",
    );

    assert!(matches!(
        state.pending,
        Some(TextAreaVimPending::Search {
            forward: true,
            query,
            cursor: 2,
        }) if query == "abc"
    ));
}

#[test]
fn vim_pending_search_home_end_move_query_cursor() {
    let (_changes, _edits, _modes, _clipboard, state) = vim_edit_result(
        "alpha beta alpha",
        0,
        &[
            key(KeyCode::Char('/')),
            key(KeyCode::Char('b')),
            key(KeyCode::Char('t')),
            key(KeyCode::Left),
            key(KeyCode::Char('e')),
            key(KeyCode::End),
            key(KeyCode::Char('a')),
            key(KeyCode::Home),
        ],
        "",
    );

    assert!(matches!(
        state.pending,
        Some(TextAreaVimPending::Search {
            forward: true,
            query,
            cursor: 0,
        }) if query == "beta"
    ));
}

#[test]
fn vim_marks_track_insert_edits_before_jump_target() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "one two",
        4,
        &[
            key(KeyCode::Char('m')),
            key(KeyCode::Char('a')),
            key(KeyCode::Char('0')),
            key(KeyCode::Char('i')),
            key(KeyCode::Char('x')),
            key(KeyCode::Esc),
            key(KeyCode::Char('`')),
            key(KeyCode::Char('a')),
        ],
        "",
    );

    assert_eq!(changes[1].value.as_ref(), "xone two");
    assert_eq!(changes.last().unwrap().cursor, 5);
}

#[test]
fn vim_dot_repeat_replays_change_insert_and_open_line_text() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "alpha beta\ngamma",
        0,
        &[
            key(KeyCode::Char('c')),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('X')),
            key(KeyCode::Esc),
            key(KeyCode::Char('w')),
            key(KeyCode::Char('.')),
            key(KeyCode::Char('o')),
            key(KeyCode::Char('!')),
            key(KeyCode::Esc),
            key(KeyCode::Char('.')),
        ],
        "",
    );

    assert_eq!(changes[3].value.as_ref(), "X X\ngamma");
    assert_eq!(changes.last().unwrap().value.as_ref(), "X X\n!\n!\ngamma");
}

#[test]
fn vim_backward_search_and_reverse_repeat_work() {
    let (changes, _edits, _modes, _clipboard, _state) = vim_edit_result(
        "alpha beta gamma beta",
        21,
        &[
            key(KeyCode::Char('?')),
            key(KeyCode::Char('b')),
            key(KeyCode::Char('e')),
            key(KeyCode::Char('t')),
            key(KeyCode::Char('a')),
            key(KeyCode::Enter),
            key(KeyCode::Char('N')),
        ],
        "",
    );

    assert_eq!(changes[0].cursor, 17);
    assert_eq!(changes[1].cursor, 6);
}

#[test]
fn vim_visual_c_deletes_selection_and_enters_insert() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "abcd",
        1,
        &[
            key(KeyCode::Char('v')),
            key(KeyCode::Char('l')),
            key(KeyCode::Char('c')),
        ],
    );

    assert_eq!(
        modes.as_slice(),
        [TextAreaVimMode::Visual, TextAreaVimMode::Insert]
    );
    assert_eq!(changes.last().unwrap().value.as_ref(), "acd");
    assert_eq!(changes.last().unwrap().anchor, None);
    assert_eq!(edits.len(), 1);
    assert_eq!(visual_anchor, None);
}

#[test]
fn vim_visual_enter_sets_anchor_and_emits_no_edit() {
    let (changes, edits, modes, visual_anchor) =
        vim_visual_result("abc", 1, &[key(KeyCode::Char('v'))]);

    assert_eq!(modes.as_slice(), [TextAreaVimMode::Visual]);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].value.as_ref(), "abc");
    assert_eq!(changes[0].cursor, 1);
    assert_eq!(changes[0].anchor, Some(1));
    assert!(edits.is_empty());
    assert_eq!(visual_anchor, Some(1));
}

#[test]
fn vim_visual_motions_and_counts_extend_selection_without_edits() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "one two\nthree",
        1,
        &[
            key(KeyCode::Char('v')),
            key(KeyCode::Char('2')),
            key(KeyCode::Char('l')),
            key(KeyCode::Char('w')),
        ],
    );

    assert_eq!(modes.as_slice(), [TextAreaVimMode::Visual]);
    assert!(edits.is_empty());
    assert_eq!(changes.last().unwrap().value.as_ref(), "one two\nthree");
    assert_eq!(changes.last().unwrap().cursor, 4);
    assert_eq!(changes.last().unwrap().anchor, Some(1));
    assert_eq!(visual_anchor, Some(1));
}

#[test]
fn vim_visual_big_word_motions_extend_selection_without_edits() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "open-code next",
        0,
        &[key(KeyCode::Char('v')), key(KeyCode::Char('W'))],
    );

    assert_eq!(modes.as_slice(), [TextAreaVimMode::Visual]);
    assert!(edits.is_empty());
    assert_eq!(changes.last().unwrap().value.as_ref(), "open-code next");
    assert_eq!(changes.last().unwrap().cursor, "open-code ".len());
    assert_eq!(changes.last().unwrap().anchor, Some(0));
    assert_eq!(visual_anchor, Some(0));
}

#[test]
fn vim_visual_crossing_anchor_preserves_original_anchor() {
    let (changes, edits, _modes, visual_anchor) = vim_visual_result(
        "abcd",
        2,
        &[
            key(KeyCode::Char('v')),
            key(KeyCode::Char('l')),
            key(KeyCode::Char('h')),
            key(KeyCode::Char('h')),
        ],
    );

    assert!(edits.is_empty());
    assert_eq!(changes.last().unwrap().cursor, 1);
    assert_eq!(changes.last().unwrap().anchor, Some(2));
    assert_eq!(visual_anchor, Some(2));
}

#[test]
fn vim_visual_exit_clears_selection_and_anchor_without_edits() {
    for exit_key in [KeyCode::Char('v'), KeyCode::Char('V'), KeyCode::Esc] {
        let (changes, edits, modes, visual_anchor) = vim_visual_result(
            "abcd",
            1,
            &[
                key(KeyCode::Char('v')),
                key(KeyCode::Char('l')),
                key(exit_key),
            ],
        );

        assert_eq!(
            modes.as_slice(),
            [TextAreaVimMode::Visual, TextAreaVimMode::Normal]
        );
        assert!(edits.is_empty());
        assert_eq!(changes.last().unwrap().cursor, 2);
        assert_eq!(changes.last().unwrap().anchor, None);
        assert_eq!(visual_anchor, None);
    }
}

#[test]
fn vim_visual_x_deletes_selection_and_exits_visual() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "abcd",
        1,
        &[
            key(KeyCode::Char('v')),
            key(KeyCode::Char('l')),
            key(KeyCode::Char('x')),
        ],
    );

    assert_eq!(
        modes.as_slice(),
        [TextAreaVimMode::Visual, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.last().unwrap().value.as_ref(), "acd");
    assert_eq!(changes.last().unwrap().anchor, None);
    assert_eq!(edits.len(), 1);
    assert_eq!(visual_anchor, None);
}

#[test]
fn vim_visual_line_enter_selects_current_whole_line() {
    let (changes, edits, modes, visual_anchor) =
        vim_visual_result("aa\nbbb\nc", 4, &[key(KeyCode::Char('V'))]);

    assert_eq!(modes.as_slice(), [TextAreaVimMode::VisualLine]);
    assert!(edits.is_empty());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].value.as_ref(), "aa\nbbb\nc");
    assert_eq!(changes[0].anchor, Some(3));
    assert_eq!(changes[0].cursor, 7);
    assert_eq!(visual_anchor, Some(3));
}

#[test]
fn vim_visual_line_jk_counts_select_whole_line_ranges_and_clamp() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "aa\nbbb\nc",
        1,
        &[
            key(KeyCode::Char('V')),
            key(KeyCode::Char('2')),
            key(KeyCode::Char('j')),
            key(KeyCode::Char('j')),
            key(KeyCode::Char('k')),
        ],
    );

    assert_eq!(modes.as_slice(), [TextAreaVimMode::VisualLine]);
    assert!(edits.is_empty());
    assert_eq!(changes[1].anchor, Some(0));
    assert_eq!(changes[1].cursor, 8);
    assert_eq!(changes.last().unwrap().anchor, Some(0));
    assert_eq!(changes.last().unwrap().cursor, 7);
    assert_eq!(visual_anchor, Some(0));
}

#[test]
fn vim_visual_line_caret_preserves_start_column_while_selection_stays_linewise() {
    let (changes, _edits, modes, _clipboard, state) = vim_edit_result(
        "abcd\nxy\nabcdef",
        2,
        &[
            key(KeyCode::Char('V')),
            key(KeyCode::Char('j')),
            key(KeyCode::Char('j')),
        ],
        "",
    );

    assert_eq!(modes.as_slice(), [TextAreaVimMode::VisualLine]);
    assert_eq!(changes.last().unwrap().anchor, Some(0));
    assert_eq!(changes.last().unwrap().cursor, "abcd\nxy\nabcdef".len());
    assert_eq!(state.visual_line_preferred_col, Some(2));
    assert_eq!(state.visual_line_caret, Some(10));
}

#[test]
fn vim_visual_line_gg_and_g_select_whole_lines() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "aa\nbbb\nc",
        4,
        &[
            key(KeyCode::Char('V')),
            key(KeyCode::Char('G')),
            key(KeyCode::Char('g')),
            key(KeyCode::Char('g')),
        ],
    );

    assert_eq!(modes.as_slice(), [TextAreaVimMode::VisualLine]);
    assert!(edits.is_empty());
    assert_eq!(changes[1].anchor, Some(3));
    assert_eq!(changes[1].cursor, 8);
    assert_eq!(changes.last().unwrap().anchor, Some(7));
    assert_eq!(changes.last().unwrap().cursor, 0);
    assert_eq!(visual_anchor, Some(3));
}

#[test]
fn vim_visual_line_exits_clear_selection_and_state() {
    for exit_key in [KeyCode::Char('v'), KeyCode::Char('V'), KeyCode::Esc] {
        let (changes, edits, modes, visual_anchor) = vim_visual_result(
            "aa\nbb",
            1,
            &[
                key(KeyCode::Char('V')),
                key(KeyCode::Char('j')),
                key(exit_key),
            ],
        );

        assert_eq!(
            modes.as_slice(),
            [TextAreaVimMode::VisualLine, TextAreaVimMode::Normal]
        );
        assert!(edits.is_empty());
        assert_eq!(changes.last().unwrap().anchor, None);
        assert_eq!(visual_anchor, None);
    }
}

#[test]
fn vim_visual_line_x_deletes_selection_and_exits_visual() {
    let (changes, edits, modes, visual_anchor) = vim_visual_result(
        "aa\nbb",
        1,
        &[key(KeyCode::Char('V')), key(KeyCode::Char('x'))],
    );

    assert_eq!(
        modes.as_slice(),
        [TextAreaVimMode::VisualLine, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.last().unwrap().value.as_ref(), "bb");
    assert_eq!(changes.last().unwrap().anchor, None);
    assert_eq!(edits.len(), 1);
    assert_eq!(visual_anchor, None);
}

#[test]
fn vim_visual_clear_binding_exits_visual_and_clears_selection_state() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abcd")
            .cursor(1)
            .vim_motions(true)
            .clear_bindings(KeyBindings::from_str("ctrl-c").expect("binding parses"))
            .on_change(on_change)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;

    modes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('l')),
        &mut ctx
    ));
    assert!(handle_key(&mut tree, root, ctrl_char('c'), &mut ctx));

    assert_eq!(
        modes.borrow().as_slice(),
        [TextAreaVimMode::Visual, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "");
    assert_eq!(changes.borrow().last().unwrap().anchor, None);
    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
    assert_eq!(state.visual_anchor, None);
    let NodeKind::TextArea(node) = &tree.node(root).kind else {
        panic!("expected textarea node");
    };
    assert_eq!(node.vim_mode, TextAreaVimMode::Normal);
    assert_eq!(node.vim_visual_line_caret, None);
    assert!(node.vim_search_feedback.is_none());
}

#[test]
fn vim_visual_line_clipboard_shortcut_syncs_render_state() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("aa\nbb")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("Z")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;

    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('V')),
        &mut ctx
    ));
    let selected = changes.borrow().last().unwrap().clone();
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let rerendered: crate::core::element::Element = TextArea::new("aa\nbb")
        .cursor(selected.cursor)
        .anchor(selected.anchor)
        .vim_motions(true)
        .on_change(on_change)
        .on_vim_mode_change(on_mode)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        },
        None,
    );
    assert!(handle_key(&mut tree, root, ctrl_char('v'), &mut ctx));

    assert_eq!(
        modes.borrow().as_slice(),
        [TextAreaVimMode::VisualLine, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "Zbb");
    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
    assert_eq!(state.visual_anchor, None);
    assert_eq!(state.visual_line_caret, None);
    let NodeKind::TextArea(node) = &tree.node(root).kind else {
        panic!("expected textarea node");
    };
    assert_eq!(node.vim_mode, TextAreaVimMode::Normal);
    assert_eq!(node.vim_visual_line_caret, None);
    assert!(node.vim_search_feedback.is_none());
}

#[test]
fn vim_visual_mutating_clipboard_shortcut_exits_visual_mode() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abcd")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .on_vim_mode_change(on_mode)
            .into(),
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("Z")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;

    modes.borrow_mut().clear();
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('l')),
        &mut ctx
    ));
    let selected = changes.borrow().last().unwrap().clone();
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let rerendered: crate::core::element::Element = TextArea::new("abcd")
        .cursor(selected.cursor)
        .anchor(selected.anchor)
        .vim_motions(true)
        .on_change(on_change)
        .on_vim_mode_change(on_mode)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        },
        None,
    );
    assert!(handle_key(&mut tree, root, ctrl_char('v'), &mut ctx));

    assert_eq!(
        modes.borrow().as_slice(),
        [TextAreaVimMode::Visual, TextAreaVimMode::Normal]
    );
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "aZcd");
    assert_eq!(changes.borrow().last().unwrap().cursor, 2);
    assert_eq!(changes.borrow().last().unwrap().anchor, None);
    let state = ctx.text_area_vim_state.get(&root).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Normal);
    assert_eq!(state.visual_anchor, None);
}

#[test]
fn vim_visual_modes_programmatic_paste_exit_and_clear_state() {
    for mode in [TextAreaVimMode::Visual, TextAreaVimMode::VisualLine] {
        let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
        let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
        let on_change = {
            let changes = Rc::clone(&changes);
            Callback::new(move |event| changes.borrow_mut().push(event))
        };
        let on_mode = {
            let modes = Rc::clone(&modes);
            Callback::new(move |mode| modes.borrow_mut().push(mode))
        };
        let mut tree = reconcile_text_area(
            TextArea::new("aa\nbb")
                .cursor(3)
                .anchor(Some(0))
                .vim_motions(true)
                .on_change(on_change)
                .on_vim_mode_change(on_mode)
                .into(),
        );

        let read_only_selection = HashMap::new();
        let mut input_history = HashMap::new();
        let mut textarea_history = HashMap::new();
        let mut text_area_vim_state = HashMap::new();
        let mut hex_history = HashMap::new();
        let mut hex_pending_edit = HashMap::new();
        let keymap = Keymap::default();
        let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
        let clipboard_config = ClipboardConfig::default();
        let mut ctx = default_key_ctx(
            &read_only_selection,
            &mut input_history,
            &mut textarea_history,
            &mut text_area_vim_state,
            &mut hex_history,
            &mut hex_pending_edit,
            &keymap,
            &clipboard,
            &clipboard_config,
        );
        let root = tree.root;
        ctx.text_area_vim_state.insert(
            root,
            TextAreaVimState {
                mode,
                count: None,
                pending: None,
                visual_anchor: Some(0),
                visual_line_head: matches!(mode, TextAreaVimMode::VisualLine).then_some(0),
                ..Default::default()
            },
        );

        assert!(handle_paste(&mut tree, root, "Z", &mut ctx));

        assert_eq!(modes.borrow().as_slice(), [TextAreaVimMode::Normal]);
        assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "Zbb");
        assert_eq!(changes.borrow().last().unwrap().cursor, 1);
        assert_eq!(changes.borrow().last().unwrap().anchor, None);
        let state = ctx.text_area_vim_state.get(&root).unwrap();
        assert_eq!(state.mode, TextAreaVimMode::Normal);
        assert_eq!(state.visual_anchor, None);
        assert_eq!(state.visual_line_head, None);
    }
}

#[test]
fn clipboard_paste_syncs_node_so_immediate_undo_preserves_history() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(TextArea::new("a").cursor(1).on_change(on_change).into());
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("X")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig {
        enable_osc52: false,
        enable_primary_selection: false,
        ..Default::default()
    };
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('b')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('v'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "abX");

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "a");
}

#[test]
fn terminal_paste_syncs_node_so_immediate_undo_preserves_sentinel_history() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut sentinels = Vec::new();
    let (value, cursor) = crate::widgets::insert_sentinel(
        "a",
        1,
        &mut sentinels,
        crate::widgets::TextAreaSentinel::new("@file"),
    );
    let mut tree = reconcile_text_area(
        TextArea::new(value.clone())
            .cursor(cursor)
            .sentinels(sentinels)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('b')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    let before_paste = changes.borrow().last().unwrap().value.clone();

    assert!(handle_paste(&mut tree, root, "X", &mut ctx));
    assert_eq!(
        changes.borrow().last().unwrap().value.as_ref(),
        format!("{before_paste}X")
    );

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value, before_paste);
}

#[test]
fn undo_restores_custom_sentinel_metadata_after_delete() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let sentinel_changes = Rc::new(RefCell::new(Vec::<Vec<TextAreaSentinel>>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_sentinels_change = {
        let sentinel_changes = Rc::clone(&sentinel_changes);
        Callback::new(move |sentinels| sentinel_changes.borrow_mut().push(sentinels))
    };
    let mut sentinels = Vec::new();
    let (value, cursor) =
        crate::widgets::insert_sentinel("ab", 1, &mut sentinels, TextAreaSentinel::new("@file"));
    let sentinel = sentinels[0].clone();
    let mut tree = reconcile_text_area(
        TextArea::new(value.clone())
            .cursor(cursor)
            .sentinels(sentinels)
            .on_change(on_change)
            .on_sentinels_change(on_sentinels_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Backspace),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    assert_eq!(sentinel_changes.borrow().last().unwrap().as_slice(), []);
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_sentinels(&mut tree, sentinel_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), value);
    assert_eq!(
        sentinel_changes.borrow().last().unwrap().as_slice(),
        std::slice::from_ref(&sentinel)
    );
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_sentinels(&mut tree, sentinel_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('y'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    assert_eq!(sentinel_changes.borrow().last().unwrap().as_slice(), []);
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_sentinels(&mut tree, sentinel_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), value);
    assert_eq!(
        sentinel_changes.borrow().last().unwrap().as_slice(),
        std::slice::from_ref(&sentinel)
    );
}

#[test]
fn undo_restores_inline_image_metadata_after_delete() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let image_changes = Rc::new(RefCell::new(Vec::<Vec<ImageContent>>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_images_change = {
        let image_changes = Rc::clone(&image_changes);
        Callback::new(move |images| image_changes.borrow_mut().push(images))
    };
    let image = ImageContent::from_bytes(b"fake-png", ImageFormat::Png);
    let image_sentinel = crate::widgets::IMAGE_SENTINEL_BASE;
    let value = format!("a{image_sentinel}b");
    let cursor = 1 + image_sentinel.len_utf8();
    let mut tree = reconcile_text_area(
        TextArea::new(value.clone())
            .cursor(cursor)
            .images(vec![image.clone()])
            .on_change(on_change)
            .on_images_change(on_images_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Backspace),
        &mut ctx
    ));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    assert_eq!(image_changes.borrow().last().unwrap().as_slice(), []);
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_images(&mut tree, image_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), value);
    assert_eq!(
        image_changes.borrow().last().unwrap().as_slice(),
        std::slice::from_ref(&image)
    );
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_images(&mut tree, image_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('y'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    assert_eq!(image_changes.borrow().last().unwrap().as_slice(), []);
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());
    apply_last_text_area_images(&mut tree, image_changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), value);
    assert_eq!(
        image_changes.borrow().last().unwrap().as_slice(),
        std::slice::from_ref(&image)
    );
}

#[test]
fn vim_normal_p_pastes_image_clipboard_when_no_text_register() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let image_changes = Rc::new(RefCell::new(Vec::<Vec<ImageContent>>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_images_change = {
        let image_changes = Rc::clone(&image_changes);
        Callback::new(move |images| image_changes.borrow_mut().push(images))
    };
    let image = ImageContent::from_bytes(b"fake-png", ImageFormat::Png);
    let image_sentinel = crate::widgets::IMAGE_SENTINEL_BASE;
    let mut tree = reconcile_text_area(
        TextArea::new("ab")
            .cursor(1)
            .vim_motions(true)
            .image_mode(TextAreaImageMode::Inline)
            .on_change(on_change)
            .on_images_change(on_images_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(
        Box::new(ImageClipboardProvider(image.clone())),
        Rc::new(|_| {}),
    );
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('p')),
        &mut ctx
    ));

    assert_eq!(
        changes.borrow().last().unwrap().value.as_ref(),
        format!("ab{image_sentinel} ")
    );
    assert_eq!(
        image_changes.borrow().last().unwrap().as_slice(),
        std::slice::from_ref(&image)
    );
}

#[test]
fn delegated_text_paste_preserves_prior_undo_history_after_external_sync() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let paste_events = Rc::new(RefCell::new(Vec::<TextAreaPasteEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_text_paste = {
        let paste_events = Rc::clone(&paste_events);
        Callback::new(move |event| paste_events.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("a")
            .cursor(1)
            .on_change(on_change)
            .on_text_paste(on_text_paste)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );

    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('b')),
        &mut ctx
    ));
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());

    assert!(handle_paste(&mut tree, root, "X", &mut ctx));
    assert_eq!(paste_events.borrow().last().unwrap().text.as_ref(), "X");
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");

    apply_last_text_area_event(
        &mut tree,
        &TextAreaEvent {
            value: Arc::from("abX"),
            cursor: 3,
            anchor: None,
        },
    );

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "ab");
    apply_last_text_area_event(&mut tree, changes.borrow().last().unwrap());

    assert!(handle_key(&mut tree, root, ctrl_char('z'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "a");
}

#[test]
fn vim_insert_commands_switch_mode_and_move_cursor_when_expected() {
    let value = "  ab\ncd";
    let cases = [
        (2, KeyCode::Char('i'), None),
        (2, KeyCode::Char('a'), Some(3)),
        (4, KeyCode::Char('I'), Some(2)),
        (2, KeyCode::Char('A'), Some(4)),
    ];

    for (start, command, expected_change_cursor) in cases {
        let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
        let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
        let on_change = {
            let changes = Rc::clone(&changes);
            Callback::new(move |event| changes.borrow_mut().push(event))
        };
        let on_mode = {
            let modes = Rc::clone(&modes);
            Callback::new(move |mode| modes.borrow_mut().push(mode))
        };
        let mut tree = reconcile_text_area(
            TextArea::new(value)
                .cursor(start)
                .vim_motions(true)
                .on_change(on_change)
                .on_vim_mode_change(on_mode)
                .into(),
        );

        let read_only_selection = HashMap::new();
        let mut input_history = HashMap::new();
        let mut textarea_history = HashMap::new();
        let mut text_area_vim_state = HashMap::new();
        let mut hex_history = HashMap::new();
        let mut hex_pending_edit = HashMap::new();
        let keymap = Keymap::default();
        let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
        let clipboard_config = ClipboardConfig::default();
        let mut ctx = default_key_ctx(
            &read_only_selection,
            &mut input_history,
            &mut textarea_history,
            &mut text_area_vim_state,
            &mut hex_history,
            &mut hex_pending_edit,
            &keymap,
            &clipboard,
            &clipboard_config,
        );

        let root = tree.root;
        modes.borrow_mut().clear();
        assert!(handle_key(&mut tree, root, key(command), &mut ctx));

        assert_eq!(modes.borrow().as_slice(), [TextAreaVimMode::Insert]);
        match expected_change_cursor {
            Some(cursor) => assert_eq!(changes.borrow().last().unwrap().cursor, cursor),
            None => assert!(changes.borrow().is_empty()),
        }
    }
}

#[test]
fn vim_wrapped_jk_uses_visual_line_navigation() {
    let value = "abcdefghij klmnopqrst uvwxyz0123";
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = NodeTree::new();
    let root: crate::core::element::Element = TextArea::new(value)
        .cursor(0)
        .vim_motions(true)
        .border(false)
        .scrollbar(false)
        .on_change(on_change)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 6,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root_id = tree.root;

    assert!(handle_key(
        &mut tree,
        root_id,
        key(KeyCode::Char('j')),
        &mut ctx
    ));
    let after_j = changes.borrow().last().unwrap().cursor;
    assert!(after_j > 0 && after_j < value.len());

    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let rerendered: crate::core::element::Element = TextArea::new(value)
        .cursor(after_j)
        .vim_motions(true)
        .border(false)
        .scrollbar(false)
        .on_change(on_change)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 6,
        },
        None,
    );
    assert!(handle_key(
        &mut tree,
        root_id,
        key(KeyCode::Char('k')),
        &mut ctx
    ));
    let after_k = changes.borrow().last().unwrap().cursor;
    assert!(after_k < after_j);
}

#[test]
fn vim_visual_wrapped_jk_preserves_anchor_across_rerender() {
    let value = "abcdefghij klmnopqrst uvwxyz0123";
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = NodeTree::new();
    let root: crate::core::element::Element = TextArea::new(value)
        .cursor(0)
        .vim_motions(true)
        .border(false)
        .scrollbar(false)
        .on_change(on_change)
        .on_vim_mode_change(on_mode)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 6,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root_id = tree.root;

    assert!(handle_key(
        &mut tree,
        root_id,
        key(KeyCode::Char('v')),
        &mut ctx
    ));
    assert!(handle_key(
        &mut tree,
        root_id,
        key(KeyCode::Char('j')),
        &mut ctx
    ));
    let after_j = changes.borrow().last().unwrap().cursor;
    assert!(after_j > 0 && after_j < value.len());
    assert_eq!(changes.borrow().last().unwrap().anchor, Some(0));

    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let rerendered: crate::core::element::Element = TextArea::new(value)
        .cursor(after_j)
        .anchor(Some(0))
        .vim_motions(true)
        .border(false)
        .scrollbar(false)
        .on_change(on_change)
        .on_vim_mode_change(on_mode)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 6,
        },
        None,
    );
    assert!(handle_key(
        &mut tree,
        root_id,
        key(KeyCode::Char('k')),
        &mut ctx
    ));
    let last = changes.borrow().last().unwrap().clone();
    assert!(last.cursor < after_j);
    assert_eq!(last.anchor, Some(0));
    assert_eq!(
        ctx.text_area_vim_state.get(&root_id).unwrap().visual_anchor,
        Some(0)
    );
}

#[test]
fn clear_interceptor_and_clipboard_shortcuts_precede_vim_dispatch() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("hello")
            .cursor(5)
            .vim_motions(true)
            .clear_bindings(KeyBindings::from_str("ctrl-c").expect("binding parses"))
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;
    assert!(handle_key(&mut tree, root, ctrl_char('c'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "");

    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .key_interceptor(KeyHandler::new(|key| key.code == KeyCode::Char('h')))
            .on_change(on_change)
            .into(),
    );
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;
    assert!(handle_key(
        &mut tree,
        root,
        key(KeyCode::Char('h')),
        &mut ctx
    ));
    assert!(changes.borrow().is_empty());

    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("a")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let paste_clipboard =
        ClipboardService::new(Box::new(StaticClipboardProvider("PASTE")), Rc::new(|_| {}));
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &paste_clipboard,
        &clipboard_config,
    );
    let root = tree.root;
    assert!(handle_key(&mut tree, root, ctrl_char('v'), &mut ctx));
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "aPASTE");
}

#[test]
fn read_only_or_missing_on_change_do_not_enter_vim_behavior() {
    let modes = Rc::new(RefCell::new(Vec::<TextAreaVimMode>::new()));
    let on_mode = {
        let modes = Rc::clone(&modes);
        Callback::new(move |mode| modes.borrow_mut().push(mode))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .read_only(true)
            .vim_motions(true)
            .on_vim_mode_change(on_mode)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root = tree.root;
    assert!(!handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));
    assert!(modes.borrow().is_empty());
    assert!(ctx.text_area_vim_state.is_empty());

    let mut tree = reconcile_text_area(TextArea::new("abc").vim_motions(true).into());
    let root = tree.root;
    assert!(!handle_key(&mut tree, root, key(KeyCode::Esc), &mut ctx));
    assert!(ctx.text_area_vim_state.is_empty());
}

#[test]
fn disabling_vim_motions_clears_existing_mode_state() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let mut tree = reconcile_text_area(
        TextArea::new("abc")
            .cursor(1)
            .vim_motions(true)
            .on_change(on_change)
            .into(),
    );
    let read_only_selection = HashMap::new();
    let mut input_history = HashMap::new();
    let mut textarea_history = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history = HashMap::new();
    let mut hex_pending_edit = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(StaticClipboardProvider("Z")), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = default_key_ctx(
        &read_only_selection,
        &mut input_history,
        &mut textarea_history,
        &mut text_area_vim_state,
        &mut hex_history,
        &mut hex_pending_edit,
        &keymap,
        &clipboard,
        &clipboard_config,
    );
    let root_id = tree.root;
    ctx.text_area_vim_state
        .insert(root_id, TextAreaVimState::default());
    assert_eq!(
        ctx.text_area_vim_state.get(&root_id).unwrap().mode,
        TextAreaVimMode::Normal
    );

    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let rerendered: crate::core::element::Element = TextArea::new("abc")
        .cursor(1)
        .vim_motions(false)
        .on_change(on_change)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        },
        None,
    );
    assert!(handle_key(&mut tree, root_id, ctrl_char('v'), &mut ctx));
    assert!(ctx.text_area_vim_state.is_empty());
    assert_eq!(changes.borrow().last().unwrap().value.as_ref(), "aZbc");
}

#[test]
fn tab_width_inserts_spaces_when_tab_reaches_text_area() {
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    let root = TextArea::new("").tab_width(4).on_change(on_change).into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let root_id = tree.root;
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    assert!(handle_key(&mut tree, root_id, key(KeyCode::Tab), &mut ctx));
    let emitted = emitted.borrow();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].value.as_ref(), "    ");
}

#[test]
fn tab_width_aligns_after_literal_tab() {
    // A literal `\t` displays as `tab_stop` columns. With tab_stop=8 (default)
    // and tab_width=4, pressing Tab after a `\t` should jump from column 8 to
    // column 12 — i.e. insert 4 spaces.
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    let root = TextArea::new("\t")
        .cursor(1)
        .tab_width(4)
        .on_change(on_change)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let root_id = tree.root;
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    assert!(handle_key(&mut tree, root_id, key(KeyCode::Tab), &mut ctx));
    let emitted = emitted.borrow();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].value.as_ref(), "\t    ");
}

#[test]
fn tab_width_aligns_to_next_tab_stop_mid_line() {
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    // Cursor sits after "ab" (column 2); next tab stop with width 4 is column 4,
    // so Tab should insert 2 spaces, not 4.
    let root = TextArea::new("ab")
        .cursor(2)
        .tab_width(4)
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
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let root_id = tree.root;
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    assert!(handle_key(&mut tree, root_id, key(KeyCode::Tab), &mut ctx));
    let emitted = emitted.borrow();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].value.as_ref(), "ab  ");
}

#[test]
fn scroll_wheel_reaches_bottom_with_horizontal_scrollbar_row() {
    let value = (0..10)
        .map(|i| format!("line {i} with enough text to require horizontal scrolling"))
        .collect::<Vec<_>>()
        .join("\n");
    let root = TextArea::new(value)
        .wrap(false)
        .h_scrollbar(true)
        .border(false)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );

    let root_id = tree.root;
    assert!(handle_scroll(&mut tree, root_id, ScrollAction::End, false));

    let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
        panic!("root should be a TextArea");
    };
    assert!(node.h_scrollbar);
    assert_eq!(node.geometry.content_viewport_h(false), 4);
    assert_eq!(node.scroll_offset, 6);
}

#[test]
fn noop_vertical_scroll_cancels_active_smooth_line_target() {
    let emitted_offsets = Rc::new(RefCell::new(Vec::new()));
    let on_scroll_to = {
        let emitted_offsets = Rc::clone(&emitted_offsets);
        Callback::new(move |offset| emitted_offsets.borrow_mut().push(offset))
    };
    let root = TextArea::new(numbered_lines(20))
        .scroll_to_line(10)
        .scroll_behavior(linear_smooth())
        .on_scroll_to(on_scroll_to)
        .border(false)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );
    let root_id = tree.root;
    assert!(matches!(
        &tree.node(root_id).kind,
        NodeKind::TextArea(node) if node.smooth_scroll.is_animating()
    ));

    assert!(handle_scroll(
        &mut tree,
        root_id,
        ScrollAction::LineUp(1),
        false,
    ));

    let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
        panic!("root should be a TextArea");
    };
    assert_eq!(node.scroll_offset, 0);
    assert_eq!(node.cancelled_scroll_to_line, Some(10));
    assert!(!node.smooth_scroll.is_animating());
    assert!(emitted_offsets.borrow().is_empty());
}

#[test]
fn noop_horizontal_scroll_cancels_active_smooth_line_target() {
    let value = (0..10)
        .map(|i| format!("line {i} with enough text to require horizontal scrolling"))
        .collect::<Vec<_>>()
        .join("\n");
    let root = TextArea::new(value)
        .scroll_to_line(8)
        .scroll_behavior(linear_smooth())
        .wrap(false)
        .h_scrollbar(true)
        .border(false)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );
    let root_id = tree.root;
    assert!(matches!(
        &tree.node(root_id).kind,
        NodeKind::TextArea(node) if node.smooth_scroll.is_animating()
    ));

    assert!(handle_scroll(
        &mut tree,
        root_id,
        ScrollAction::LineLeft(1),
        false,
    ));

    let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
        panic!("root should be a TextArea");
    };
    assert_eq!(node.h_scroll_offset, 0);
    assert_eq!(node.cancelled_scroll_to_line, Some(8));
    assert!(!node.smooth_scroll.is_animating());
}

#[test]
fn key_interceptor_consumes_tab_before_tab_width() {
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    let root = TextArea::new("")
        .tab_width(4)
        .key_interceptor(KeyHandler::new(|key| key.code == KeyCode::Tab))
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
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let root_id = tree.root;
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    assert!(handle_key(&mut tree, root_id, key(KeyCode::Tab), &mut ctx));
    assert!(emitted.borrow().is_empty());
}

#[test]
fn key_interceptor_consumes_clear_binding_before_clear() {
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    let root = TextArea::new("hello")
        .cursor(5)
        .clear_bindings(KeyBindings::from_str("ctrl-c").expect("clear binding parses"))
        .key_interceptor(KeyHandler::new(|key| {
            key.code == KeyCode::Char('c') && key.mods.ctrl
        }))
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
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let root_id = tree.root;
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    assert!(handle_key(&mut tree, root_id, ctrl_char('c'), &mut ctx));
    assert!(emitted.borrow().is_empty());
}

#[test]
fn clear_binding_clears_and_preserves_history_across_rerender() {
    let changes = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let edits = Rc::new(RefCell::new(Vec::<TextEditEvent>::new()));
    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let clear_bindings = KeyBindings::from_str("ctrl-c").expect("clear binding parses");

    let root = TextArea::new("hello")
        .cursor(5)
        .anchor(Some(1))
        .clear_bindings(clear_bindings.clone())
        .on_change(on_change)
        .on_edit(on_edit)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    let root_id = tree.root;
    assert!(handle_key(&mut tree, root_id, ctrl_char('c'), &mut ctx));
    {
        let changes = changes.borrow();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].value.as_ref(), "");
        assert_eq!(changes[0].cursor, 0);
        assert_eq!(changes[0].anchor, None);
    }
    {
        let edits = edits.borrow();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start, 0);
        assert_eq!(edits[0].deleted.as_ref(), "hello");
        assert_eq!(edits[0].inserted.as_ref(), "");
        assert_eq!(edits[0].cursor_before, 5);
        assert_eq!(edits[0].anchor_before, Some(1));
        assert_eq!(edits[0].kind, TextEditKind::Replace);
    }

    let on_change = {
        let changes = Rc::clone(&changes);
        Callback::new(move |event| changes.borrow_mut().push(event))
    };
    let on_edit = {
        let edits = Rc::clone(&edits);
        Callback::new(move |event| edits.borrow_mut().push(event))
    };
    let rerendered = TextArea::new("")
        .clear_bindings(clear_bindings)
        .on_change(on_change)
        .on_edit(on_edit)
        .into();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &rerendered,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );

    assert!(handle_key(&mut tree, root_id, ctrl_char('z'), &mut ctx));
    let changes = changes.borrow();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].value.as_ref(), "hello");
    assert_eq!(changes[1].cursor, 5);
    assert_eq!(changes[1].anchor, Some(1));
}

/// Up/Down arrows should walk through wrapped (visual) lines, not jump
/// over the wrap to the previous/next logical line.
#[test]
fn arrow_up_down_walks_wrapped_visual_lines() {
    let emitted = Rc::new(RefCell::new(Vec::<TextAreaEvent>::new()));
    let on_change = {
        let emitted = Rc::clone(&emitted);
        Callback::new(move |event| emitted.borrow_mut().push(event))
    };

    // Single logical line, ~30 cols wide, will wrap at width ~10.
    // "abcdefghij klmnopqrst uvwxyz0123" → 32 chars.
    let value = "abcdefghij klmnopqrst uvwxyz0123";
    let root = TextArea::new(value)
        .cursor(0)
        .border(false)
        .scrollbar(false)
        .on_change(on_change)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 6,
        },
        None,
    );

    // Verify the textarea actually wraps into multiple visual lines.
    let root_id = tree.root;
    let visual_lines_count = if let NodeKind::TextArea(node) = &tree.node(root_id).kind {
        node.visual_lines_count
    } else {
        panic!("expected TextArea node");
    };
    assert!(
        visual_lines_count >= 3,
        "expected wrapped value to span ≥3 visual lines, got {visual_lines_count}"
    );

    let read_only_selection = HashMap::new();
    let mut input_history: HashMap<_, TextInput> = HashMap::new();
    let mut textarea_history: HashMap<_, TextEditor> = HashMap::new();
    let mut text_area_vim_state = HashMap::new();
    let mut hex_history: HashMap<_, HexHistory> = HashMap::new();
    let mut hex_pending_edit: HashMap<_, HexPendingEdit> = HashMap::new();
    let keymap = Keymap::default();
    let clipboard = ClipboardService::new(Box::new(TestClipboardProvider), Rc::new(|_| {}));
    let clipboard_config = ClipboardConfig::default();
    let mut ctx = KeyCtx {
        read_only_selection: Some(&read_only_selection),
        input_history: &mut input_history,
        textarea_history: &mut textarea_history,
        text_area_vim_state: &mut text_area_vim_state,
        hex_history: &mut hex_history,
        hex_pending_edit: &mut hex_pending_edit,
        keymap: &keymap,
        text_area_newline_binding: TextAreaNewlineBinding::default(),
        clipboard: &clipboard,
        clipboard_config: &clipboard_config,
        copy_feedback: Box::leak(Box::new(
            crate::app::copy_feedback::CopyFeedbackState::default(),
        )),
        dirty_override: None,
    };

    // Re-render with updated cursor between key events, the way a real
    // app would on `on_change`.
    let mut current_cursor = 0usize;
    let mut press = |code: KeyCode,
                     tree: &mut NodeTree,
                     emitted: &Rc<RefCell<Vec<TextAreaEvent>>>,
                     ctx: &mut KeyCtx<'_>|
     -> Option<usize> {
        handle_key(tree, tree.root, key(code), ctx);
        let new_cursor = emitted.borrow().last().map(|e| e.cursor);
        if let Some(c) = new_cursor {
            current_cursor = c;
            let cb = {
                let emitted = Rc::clone(emitted);
                Callback::new(move |event| emitted.borrow_mut().push(event))
            };
            let next: crate::core::element::Element = TextArea::new(value)
                .cursor(current_cursor)
                .border(false)
                .scrollbar(false)
                .on_change(cb)
                .into();
            LayoutEngine::reconcile_with_focus(
                tree,
                &next,
                Rect {
                    x: 0,
                    y: 0,
                    w: 12,
                    h: 6,
                },
                None,
            );
        }
        new_cursor
    };

    // Down from the start of a wrapped line should land inside the buffer
    // (not at the very end as logical move_down would, since there's only
    // one logical line).
    let after_down =
        press(KeyCode::Down, &mut tree, &emitted, &mut ctx).expect("Down should be handled");
    assert!(after_down > 0, "Down should advance cursor across wrap");
    assert!(
        after_down < value.len(),
        "Down should not jump to end of buffer; got cursor={after_down}"
    );

    // Up should bring the cursor back near the original position.
    let after_up = press(KeyCode::Up, &mut tree, &emitted, &mut ctx).expect("Up should be handled");
    assert!(
        after_up < after_down,
        "Up should move cursor back above (after_up={after_up}, after_down={after_down})"
    );
}

#[test]
fn visual_boundary_nav_moves_to_buffer_edges() {
    let lines = vec![
        TextAreaVisualLine {
            line_num: 0,
            continuation: false,
            start: 0,
            end: 5,
            visual_start_col: 0,
            visual_end_col: 5,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 0,
            continuation: true,
            start: 5,
            end: 10,
            visual_start_col: 5,
            visual_end_col: 10,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
    ];

    let mut editor = TextEditor::new("abcdefghij");
    editor.set_cursor(3);
    assert!(perform_visual_vertical_nav(
        &mut editor,
        Action::MoveUp,
        &lines,
        None,
        8,
        &[]
    ));
    assert_eq!(editor.cursor(), 0);

    editor.set_cursor(7);
    assert!(perform_visual_vertical_nav(
        &mut editor,
        Action::MoveDown,
        &lines,
        None,
        8,
        &[]
    ));
    assert_eq!(editor.cursor(), 10);
}

#[test]
fn visual_vertical_nav_from_wrap_boundary_clamps_to_shorter_adjacent_line_end() {
    let value = "abcdefghijklm";
    let lines = vec![
        TextAreaVisualLine {
            line_num: 1,
            continuation: false,
            start: 0,
            end: 5,
            visual_start_col: 0,
            visual_end_col: 5,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 1,
            continuation: true,
            start: 5,
            end: 8,
            visual_start_col: 5,
            visual_end_col: 8,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 1,
            continuation: true,
            start: 8,
            end: 13,
            visual_start_col: 8,
            visual_end_col: 13,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
    ];

    let mut from_first_boundary = TextEditor::new(value);
    from_first_boundary.set_cursor(5);
    assert!(perform_visual_vertical_nav(
        &mut from_first_boundary,
        Action::MoveDown,
        &lines,
        None,
        8,
        &[]
    ));
    assert_eq!(from_first_boundary.cursor(), 8);

    // Byte 8 is the soft-wrap boundary shared by rows 1 and 2; it now belongs
    // to row 2 (renders/measures there). Moving up onto row 1 must therefore
    // stop at byte 7 (row 1's last cell), not fall through to the boundary and
    // stay visually on row 2.
    let mut from_last_boundary = TextEditor::new(value);
    from_last_boundary.set_cursor(13);
    assert!(perform_visual_vertical_nav(
        &mut from_last_boundary,
        Action::MoveUp,
        &lines,
        None,
        8,
        &[]
    ));
    assert_eq!(from_last_boundary.cursor(), 7);
}

#[test]
fn visual_vertical_nav_from_wrap_row_end_descends_one_row_at_a_time() {
    // A single logical line wrapped into three visual rows. The cursor sits at
    // the end of row 0's content (the shared soft-wrap boundary). Moving down
    // must land on row 1, not skip past it to row 2; and moving back up must
    // return to row 0 rather than getting stuck.
    let value = "abcde fghij\nklmno pqrst\nuvwxy z";
    let lines = vec![
        TextAreaVisualLine {
            line_num: 1,
            continuation: false,
            start: 0,
            end: 6,
            visual_start_col: 0,
            visual_end_col: 6,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 1,
            continuation: true,
            start: 6,
            end: 12,
            visual_start_col: 6,
            visual_end_col: 12,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 1,
            continuation: true,
            start: 12,
            end: 18,
            visual_start_col: 12,
            visual_end_col: 18,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
    ];

    // Cursor at byte 5 sits on row 0 (5 < row0.end == 6).
    let mut editor = TextEditor::new(value);
    editor.set_cursor(5);
    assert!(perform_visual_vertical_nav(
        &mut editor,
        Action::MoveDown,
        &lines,
        None,
        8,
        &[]
    ));
    // Lands on row 1 (byte 11, its last cell), not on byte 12 (row 2's start).
    assert_eq!(editor.cursor(), 11);
    assert_eq!(text_area_visual_line_for_cursor(&lines, editor.cursor()), 1);

    // Moving back up returns to row 0 instead of getting stuck.
    assert!(perform_visual_vertical_nav(
        &mut editor,
        Action::MoveUp,
        &lines,
        None,
        8,
        &[]
    ));
    assert_eq!(text_area_visual_line_for_cursor(&lines, editor.cursor()), 0);
}

#[test]
fn visual_vertical_nav_clamps_offsets_inside_unicode_characters() {
    let value = "abc\n• OpenCode";
    let bullet = value.find('•').expect("test value contains bullet");
    let inside_bullet = bullet + 1;
    let lines = vec![
        TextAreaVisualLine {
            line_num: 2,
            continuation: false,
            start: bullet,
            end: inside_bullet,
            visual_start_col: 0,
            visual_end_col: 1,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 2,
            continuation: true,
            start: inside_bullet,
            end: value.len(),
            visual_start_col: 1,
            visual_end_col: 10,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
    ];

    let mut editor = TextEditor::new(value);
    editor.core.cursor = inside_bullet;

    assert!(perform_visual_vertical_nav(
        &mut editor,
        Action::MoveUp,
        &lines,
        None,
        8,
        &[]
    ));
    assert!(value.is_char_boundary(editor.cursor()));
}

#[test]
fn vim_vertical_nav_accounts_for_inline_virtual_text() {
    let value = "ab\ncde";
    let lines = vec![
        TextAreaVisualLine {
            line_num: 1,
            continuation: false,
            start: 0,
            end: 2,
            visual_start_col: 0,
            visual_end_col: 5,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
        TextAreaVisualLine {
            line_num: 2,
            continuation: false,
            start: 3,
            end: 6,
            visual_start_col: 0,
            visual_end_col: 3,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        },
    ];
    let virtual_texts = vec![TextAreaVirtualText::inline(1, vec![Span::new("xxx")])];
    let mut editor = TextEditor::new(value);
    editor.set_cursor(1);
    let mut state = TextAreaVimState::default();

    let outcome = super::vim::dispatch_text_area_vim_key(
        &mut editor,
        &mut state,
        key(KeyCode::Char('j')),
        Action::None,
        &super::vim::VimLayoutCtx {
            wrap: true,
            visual_lines: Some(&lines),
            sentinel: None,
            tab_stop: 8,
            virtual_texts: &virtual_texts,
        },
    );

    assert!(matches!(
        outcome,
        super::vim::VimKeyOutcome::EditorChanged { vertical: true, .. }
    ));
    assert_eq!(editor.cursor(), value.len());
}
