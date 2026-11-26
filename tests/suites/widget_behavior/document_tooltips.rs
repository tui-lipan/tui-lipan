//! `DocumentView::tooltips` names the language of an icon-only code-block header and shows the
//! target of a link whose text hides it.
use std::time::Duration;

use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{CodeBlockHeader, TestBackend};

const DOC: &str = "See [the guide](https://example.com/guide) or <https://example.com/raw> here.\n\n```rust\nlet answer = 42;\n```";

struct Doc {
    text: &'static str,
    delay: Duration,
    skip_delay: Duration,
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
        DocumentView::new(self.text)
            .markdown()
            .border(false)
            .scrollbar(false)
            .code_block_header(CodeBlockHeader::Icon)
            .tooltips(
                TooltipConfig::new()
                    .delay(self.delay)
                    .skip_delay(self.skip_delay)
                    .placement(PopoverPlacement::BelowStart),
            )
            .into()
    }
}

fn backend_for(text: &'static str, width: u16, delay: Duration) -> TestBackend<Doc> {
    let mut backend = TestBackend::new(Doc {
        text,
        delay,
        skip_delay: Duration::ZERO,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: width,
        h: 12,
    });
    backend.render();
    backend
}

fn backend(delay: Duration) -> TestBackend<Doc> {
    backend_for(DOC, 80, delay)
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

/// The cell of the header icon above the code line starting with `code`.
fn icon_above(backend: &TestBackend<Doc>, code: &str) -> (u16, u16) {
    let (_, code_y) = cell_of(backend, code);
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

fn wait(backend: &mut TestBackend<Doc>, dt: Duration) {
    backend.advance(dt);
    backend.pump().unwrap();
    backend.render();
}

#[test]
fn icon_only_header_names_its_language() {
    let mut backend = backend(Duration::ZERO);
    assert!(!screen(&backend).contains("rust"));

    let icon = icon_above(&backend, "let answer");
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
fn wrapped_autolink_shows_no_tooltip_on_any_row() {
    const AUTOLINK: &str = "<https://example.com/a/very/long/path/that/wraps/onto/rows>";
    let mut backend = backend_for(AUTOLINK, 20, Duration::ZERO);
    let before = screen(&backend);
    let rows: Vec<u16> = before
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(y, _)| y as u16)
        .collect();
    assert!(rows.len() > 1, "the URL must wrap: {before:?}");
    for y in rows {
        hover(&mut backend, (2, y));
        assert_eq!(screen(&backend), before, "row {y} showed a tooltip");
    }
}

#[test]
fn moving_between_parts_switches_the_tooltip() {
    let mut backend = backend(Duration::ZERO);
    let guide = cell_of(&backend, "the guide");
    hover(&mut backend, guide);
    let icon = icon_above(&backend, "let answer");
    hover(&mut backend, icon);
    let text = screen(&backend);
    assert!(text.contains("rust"));
    assert!(!text.contains("https://example.com/guide"));
}

/// Moving straight from one part to another with the same hint restarts the wait.
fn assert_twin_parts_wait_again(
    text: &'static str,
    first: fn(&TestBackend<Doc>) -> (u16, u16),
    second: fn(&TestBackend<Doc>) -> (u16, u16),
    hint: &str,
) {
    let delay = Duration::from_millis(500);
    let mut backend = backend_for(text, 80, delay);
    let (a, b) = (first(&backend), second(&backend));
    let shows = |backend: &TestBackend<Doc>| screen(backend).contains(hint);

    hover(&mut backend, a);
    wait(&mut backend, delay);
    assert!(shows(&backend));

    hover(&mut backend, b);
    assert!(
        !shows(&backend),
        "the second part reused the first one's tooltip"
    );
    wait(&mut backend, delay);
    assert!(shows(&backend));
}

#[test]
fn code_blocks_with_one_language_are_separate_parts() {
    assert_twin_parts_wait_again(
        "```rust\nlet first = 1;\n```\n\n```rust\nlet second = 2;\n```",
        |backend| icon_above(backend, "let first"),
        |backend| icon_above(backend, "let second"),
        "rust",
    );
}

#[test]
fn links_to_one_url_are_separate_parts() {
    assert_twin_parts_wait_again(
        "[one](https://example.com/same) and [two](https://example.com/same)",
        |backend| cell_of(backend, "one"),
        |backend| cell_of(backend, "two"),
        "https://example.com/same",
    );
}

#[test]
fn delayed_tooltip_waits_before_opening() {
    let mut backend = backend(Duration::from_millis(500));
    let icon = icon_above(&backend, "let answer");
    hover(&mut backend, icon);
    assert!(!screen(&backend).contains("rust"));

    wait(&mut backend, Duration::from_millis(500));
    assert!(screen(&backend).contains("rust"));
}
