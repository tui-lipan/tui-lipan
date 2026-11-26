//! `DocumentView::tooltips` names the language of an icon-only code-block header and shows the
//! target of a link whose text hides it.
use std::time::Duration;

use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{CodeBlockHeader, TestBackend};

const DOC: &str = "See [the guide](https://example.com/guide) or https://example.com/raw here.\n\n```rust\nlet answer = 42;\n```";

struct Doc {
    delay: Duration,
}

impl Component for Doc {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        DocumentView::new(DOC)
            .markdown()
            .border(false)
            .scrollbar(false)
            .code_block_header(CodeBlockHeader::Icon)
            .tooltips(
                TooltipConfig::new()
                    .delay(self.delay)
                    .placement(PopoverPlacement::BelowStart),
            )
            .into()
    }
}

fn backend(delay: Duration) -> TestBackend<Doc> {
    let mut backend = TestBackend::new(Doc { delay });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 12,
    });
    backend.render();
    backend
}

fn screen(backend: &TestBackend<Doc>) -> String {
    backend.capture_frame().plain_text()
}

/// The cell where `needle` starts on screen.
fn cell_of(backend: &TestBackend<Doc>, needle: &str) -> (u16, u16) {
    screen(backend)
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            let byte = line.find(needle)?;
            Some((line[..byte].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("`{needle}` must be on screen"))
}

/// The cell of the header icon: the inset column on the row above the code.
fn icon_cell(backend: &TestBackend<Doc>) -> (u16, u16) {
    let (_, code_y) = cell_of(backend, "let answer");
    (1, code_y - 1)
}

fn hover(backend: &mut TestBackend<Doc>, (x, y): (u16, u16)) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.pump().unwrap();
    backend.render();
}

#[test]
fn icon_only_header_names_its_language() {
    let mut backend = backend(Duration::ZERO);
    assert!(!screen(&backend).contains("rust"));

    let icon = icon_cell(&backend);
    hover(&mut backend, icon);
    assert!(screen(&backend).contains("rust"));

    hover(&mut backend, (40, 11));
    assert!(!screen(&backend).contains("rust"));
}

#[test]
fn link_shows_its_target_only_when_its_text_hides_it() {
    let mut backend = backend(Duration::ZERO);
    let guide = cell_of(&backend, "the guide");
    hover(&mut backend, (guide.0 + 2, guide.1));
    assert!(screen(&backend).contains("https://example.com/guide"));

    let raw = cell_of(&backend, "https://example.com/raw");
    hover(&mut backend, (raw.0 + 2, raw.1));
    let text = screen(&backend);
    assert!(!text.contains("https://example.com/guide"));
    assert_eq!(text.matches("https://example.com/raw").count(), 1);
}

#[test]
fn moving_between_parts_switches_the_tooltip() {
    let mut backend = backend(Duration::ZERO);
    let guide = cell_of(&backend, "the guide");
    hover(&mut backend, guide);
    let icon = icon_cell(&backend);
    hover(&mut backend, icon);
    let text = screen(&backend);
    assert!(text.contains("rust"));
    assert!(!text.contains("https://example.com/guide"));
}

#[test]
fn delayed_tooltip_waits_before_opening() {
    let mut backend = backend(Duration::from_millis(500));
    let icon = icon_cell(&backend);
    hover(&mut backend, icon);
    assert!(!screen(&backend).contains("rust"));

    backend.advance(Duration::from_millis(500));
    backend.pump().unwrap();
    backend.render();
    assert!(screen(&backend).contains("rust"));
}
