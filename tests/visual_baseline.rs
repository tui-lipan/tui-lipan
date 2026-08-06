//! Committed visual baselines for core widget chrome.
//!
//! These guard the rendering the whole framework is judged on - frame borders and
//! headers, focus chrome, input placeholders and masking, button and list
//! selection - against unintended pixel changes. A refactor that quietly moves a
//! border or drops a focus highlight fails here instead of shipping.
//!
//! The views are owned by this test rather than reused from `examples/sketches/`
//! so the coverage is deliberate: each one exists to pin specific chrome, not to
//! look good.
//!
//! # When this test fails
//!
//! Read the `*.diff.png` named in the failure - unchanged pixels are dimmed,
//! changed pixels are magenta - and decide whether the change was intended. If it
//! was, re-record and commit the updated images:
//!
//! ```sh
//! TUI_LIPAN_UPDATE_BASELINES=1 cargo test --all-features --test visual_baseline
//! ```
//!
//! Baselines are compared with the built-in bitmap font, never a system font, so
//! results are identical on CI and on any contributor's machine.

#![cfg(feature = "ui-snapshot-png")]

use tui_lipan::prelude::*;
use tui_lipan::{Result, Sketch};

/// Directory holding the committed reference images.
const BASELINE_DIR: &str = "tests/ui-baselines";

/// Panels, headers, and a divider: the chrome a `Frame` is responsible for.
fn frame_chrome() -> Element {
    HStack::new()
        .gap(1)
        .child(
            Frame::new()
                .header_left("Nav")
                .border(true)
                .width(Length::Px(16))
                .child(
                    List::new()
                        .items(["Overview", "Logs", "Settings"].map(ListItem::new))
                        .selected(1)
                        .key("routes"),
                )
                .key("sidebar"),
        )
        .child(
            Frame::new()
                .header_left("Detail")
                .border(true)
                .child(
                    VStack::new()
                        .gap(1)
                        .padding(1)
                        .child(Text::new("Body copy"))
                        .child(Divider::new(Orientation::Horizontal))
                        .child(Text::new("After divider")),
                )
                .key("detail"),
        )
        .into()
}

/// Input placeholder, masking, and button chrome.
fn form_chrome() -> Element {
    Frame::new()
        .header_left("Credentials")
        .border(true)
        .child(
            VStack::new()
                .gap(1)
                .padding(1)
                .child(Input::new("alice@example.com").key("email"))
                .child(Input::new("").placeholder("Password").key("blank"))
                .child(Input::new("hunter2").mask(Some('*')).key("secret"))
                .child(
                    HStack::new()
                        .gap(2)
                        .child(Button::new("Cancel"))
                        .child(Button::new("Submit")),
                ),
        )
        .into()
}

#[test]
fn frame_and_list_chrome_matches_baseline() -> Result<()> {
    Sketch::view("chrome-frame", frame_chrome)
        .viewport(48, 10)
        .dir(std::env::temp_dir().join("tui-lipan-baseline-artifacts"))
        .baseline(BASELINE_DIR)
        .quiet(true)
        .assert_baseline()
}

#[test]
fn form_chrome_matches_baseline() -> Result<()> {
    Sketch::view("chrome-form", form_chrome)
        .viewport(40, 12)
        .dir(std::env::temp_dir().join("tui-lipan-baseline-artifacts"))
        .baseline(BASELINE_DIR)
        .quiet(true)
        .assert_baseline()
}

#[test]
fn focused_form_chrome_matches_baseline() -> Result<()> {
    // Focus chrome is the easiest thing to break without noticing, since it only
    // appears in one state.
    Sketch::view("chrome-form-focused", form_chrome)
        .viewport(40, 12)
        .focus_next(1)
        .dir(std::env::temp_dir().join("tui-lipan-baseline-artifacts"))
        .baseline(BASELINE_DIR)
        .quiet(true)
        .assert_baseline()
}

#[test]
fn baseline_rendering_is_reproducible_within_a_run() {
    // Guards the determinism the whole baseline mechanism rests on: if two
    // captures of the same view in the same process differ, every baseline is
    // noise and the failures above would be meaningless.
    let dir = std::env::temp_dir().join(format!(
        "tui-lipan-baseline-determinism-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let first = Sketch::view("determinism", form_chrome)
        .viewport(32, 8)
        .dir(dir.join("a"))
        .quiet(true)
        .write()
        .expect("first capture");
    let second = Sketch::view("determinism", form_chrome)
        .viewport(32, 8)
        .dir(dir.join("b"))
        .quiet(true)
        .write()
        .expect("second capture");

    let png = |paths: &[std::path::PathBuf]| {
        let path = paths
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "png"))
            .expect("a png was written")
            .clone();
        std::fs::read(path).expect("read png")
    };

    assert_eq!(
        png(&first),
        png(&second),
        "the same view must render identically twice"
    );
    std::fs::remove_dir_all(&dir).ok();
}
