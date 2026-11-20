use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

/// Registers a command whose description fuzzy-matches `theme` in a category that sorts first,
/// and one whose name matches in a later category.
struct Commands;

impl Component for Commands {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) -> Self::State {}

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        for (id, label, description, category) in [
            (
                "dismiss",
                "Dismiss overlay",
                "Close the top-most dismissible overlay",
                "Application",
            ),
            ("theme", "Choose theme", "", "View"),
        ] {
            let mut entry = CommandEntry::builder(id)
                .label(label)
                .category(category)
                .handler(Callback::new(|()| {}));
            if !description.is_empty() {
                entry = entry.description(description);
            }
            ctx.register_command(entry.build());
        }
        None
    }

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        CommandPalette::new().into()
    }
}

fn row_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not rendered in:\n{text}"))
}

#[test]
fn name_matches_rank_above_description_matches_across_categories() {
    let mut backend = TestBackend::new(Commands);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 20,
    });
    backend.render();
    for ch in "theme".chars() {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(ch),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    let text = backend.capture_frame().plain_text();
    assert!(
        row_of(&text, "Choose theme") < row_of(&text, "Dismiss overlay"),
        "the name match should lead:\n{text}"
    );
}
