//! `ScrollKeymap::VIM_JUMP` gives selection widgets `g` / `G` for first / last row. It is opt-in,
//! so a tree on the default keymap leaves both keys to the app.

use std::sync::Arc;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods};
use tui_lipan::prelude::*;

struct JumpTree {
    keys: ScrollKeymap,
}

#[derive(Default)]
struct Selections(Vec<Arc<str>>);

impl Component for JumpTree {
    type Message = FileTreeEvent;
    type Properties = ();
    type State = Selections;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Selections::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.0.push(msg.path);
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        FileTree::new("/repo")
            .entry_source(FileTreeEntrySource::Provided(vec![
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        FileTreeEntry::file("a.rs"),
                        FileTreeEntry::file("b.rs"),
                        FileTreeEntry::file("c.rs"),
                    ],
                ),
            ]))
            .show_icons(false)
            .scroll_keys(self.keys)
            .on_select(ctx.link().callback(|event| event))
            .key("tree")
    }
}

fn key(code: KeyCode, mods: KeyMods) -> KeyEvent {
    KeyEvent { code, mods }
}

/// The path the tree last reported selecting after `presses`, if it reported one at all.
fn last_selection(keys: ScrollKeymap, presses: &[KeyEvent]) -> Option<Arc<str>> {
    let mut backend = TestBackend::new(JumpTree { keys });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 8,
    });
    backend.render();
    backend.focus_next();
    for press in presses {
        backend.send_key(*press).expect("send key");
        backend.render();
    }
    backend.state().0.last().cloned()
}

#[test]
fn vim_jump_moves_the_selection_to_the_first_and_last_rows() {
    let keys = ScrollKeymap::DEFAULT | ScrollKeymap::VIM_JUMP;
    let shift_g = key(KeyCode::Char('G'), KeyMods::SHIFT);
    let g = key(KeyCode::Char('g'), KeyMods::NONE);
    let home = key(KeyCode::Home, KeyMods::NONE);
    let end = key(KeyCode::End, KeyMods::NONE);

    let last = last_selection(keys, &[shift_g]).expect("G selects a row");
    assert!(last.ends_with("c.rs"), "G selects the last row, got {last}");
    assert_eq!(
        Some(last),
        last_selection(keys, &[end]),
        "G lands where End does"
    );

    let first = last_selection(keys, &[shift_g, g]).expect("g selects a row");
    assert_eq!(
        Some(first),
        last_selection(keys, &[end, home]),
        "g lands where Home does"
    );
}

#[test]
fn the_default_keymap_leaves_g_alone() {
    let shift_g = key(KeyCode::Char('G'), KeyMods::SHIFT);
    assert_eq!(last_selection(ScrollKeymap::DEFAULT, &[shift_g]), None);
}
