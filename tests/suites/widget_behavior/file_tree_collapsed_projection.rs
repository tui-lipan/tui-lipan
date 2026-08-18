//! Collapsed `FileTree` directories are projected as a single placeholder child
//! instead of a fully materialized subtree. These tests pin the user-visible
//! contract that optimization must not break: a collapsed directory still
//! expands, and its contents still appear.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods};
use tui_lipan::prelude::*;

struct RemoteTree;

fn listings() -> Vec<FileTreeDirectoryListing> {
    vec![
        FileTreeDirectoryListing::new(
            ".",
            [
                FileTreeEntry::directory("src"),
                FileTreeEntry::file("README.md"),
            ],
        ),
        FileTreeDirectoryListing::new("src", [FileTreeEntry::file("main.rs")]),
        FileTreeDirectoryListing::new("empty", Vec::new()),
    ]
}

impl Component for RemoteTree {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::Provided(listings()))
            .show_icons(false)
            .key("tree")
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::NONE,
    }
}

fn visible_text(backend: &TestBackend<RemoteTree>) -> String {
    backend.capture_frame().plain_text()
}

#[test]
fn collapsed_directory_expands_and_reveals_its_children() {
    let mut backend = TestBackend::new(RemoteTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();

    let collapsed = visible_text(&backend);
    assert!(collapsed.contains("src"), "collapsed frame:\n{collapsed}");
    assert!(
        collapsed.contains("README.md"),
        "collapsed frame:\n{collapsed}"
    );
    assert!(
        !collapsed.contains("main.rs"),
        "a collapsed directory must not render its children:\n{collapsed}"
    );

    backend.focus_next();
    // Move the cursor off the root row onto `src`, then expand it.
    backend.send_key(key(KeyCode::Down)).unwrap();
    backend.send_key(key(KeyCode::Right)).unwrap();
    backend.render();

    let expanded = visible_text(&backend);
    assert!(
        expanded.contains("main.rs"),
        "expanding a collapsed directory must materialize its children:\n{expanded}"
    );

    // And collapsing again must hide them, proving the placeholder round-trips.
    backend.send_key(key(KeyCode::Left)).unwrap();
    backend.render();

    let recollapsed = visible_text(&backend);
    assert!(
        !recollapsed.contains("main.rs"),
        "collapsing must hide children again:\n{recollapsed}"
    );
    assert!(recollapsed.contains("src"), "frame:\n{recollapsed}");
}
