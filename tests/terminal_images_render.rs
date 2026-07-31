//! A Kitty graphics command from a child program ends up as pixels in the rendered frame.
//!
//! The host in a test is the halfblock encoder, which is the point: the same path serves a host
//! that speaks no graphics protocol at all, and it is the only encoder whose output can be read
//! back out of a cell buffer and asserted on.

#![cfg(feature = "terminal-images")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const CELL: TerminalCellSize = TerminalCellSize {
    width: 10,
    height: 20,
};

const RED: Color = Color::Rgb(255, 0, 0);

/// A transmit-and-display command for a solid image, sized in cells rather than pixels.
fn solid_image(id: u32, cols: u32, rows: u32, colour: [u8; 3]) -> Vec<u8> {
    let (width, height) = (cols * u32::from(CELL.width), rows * u32::from(CELL.height));
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&colour);
    }
    format!(
        "\x1b_Ga=T,f=24,s={width},v={height},t=d,i={id};{}\x1b\\",
        BASE64.encode(pixels)
    )
    .into_bytes()
}

/// A solid red image, the shape most of these tests want.
fn red_image(cols: u32, rows: u32) -> Vec<u8> {
    solid_image(1, cols, rows, [255, 0, 0])
}

struct Pane {
    screen: Rc<RefCell<TerminalScreen>>,
}

impl Component for Pane {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Terminal::new()
            .screen(TerminalScreenHandle::new(Rc::clone(&self.screen)))
            .scrollbar(false)
            .into()
    }
}

/// Render until `ready` accepts a frame, or give up and return the last one.
///
/// Encoding runs off the UI thread, so the first frame after a new image is expected to have no
/// pixels in it yet. Real apps repaint when the encode lands; a test just draws again.
fn render_until(
    backend: &mut TestBackend<Pane>,
    ready: impl Fn(&CapturedFrame) -> bool,
) -> CapturedFrame {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        backend.render();
        let frame = backend.capture_frame();
        if ready(&frame) || Instant::now() >= deadline {
            return frame;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether the image has landed in the top-left cell of the pane.
fn painted(frame: &CapturedFrame) -> bool {
    frame.cell(0, 0).fg == RED || frame.cell(0, 0).bg == RED
}

fn pane(output: &[u8], rows: u16, cols: u16) -> TestBackend<Pane> {
    pane_with_screen(output, rows, cols).0
}

/// A pane, plus the screen behind it for tests that assert on placements as well as pixels.
fn pane_with_screen(
    output: &[u8],
    rows: u16,
    cols: u16,
) -> (TestBackend<Pane>, Rc<RefCell<TerminalScreen>>) {
    let mut screen = TerminalScreen::new(rows, cols, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(output);

    let screen = Rc::new(RefCell::new(screen));
    let mut backend = TestBackend::new(Pane {
        screen: Rc::clone(&screen),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: cols,
        h: rows,
    });
    (backend, screen)
}

#[test]
fn a_transmitted_image_is_painted_into_the_pane() {
    let frame = render_until(&mut pane(&red_image(4, 2), 6, 20), painted);

    // Halfblocks paint two pixel rows per cell, so a solid image is a solid colored block.
    for y in 0..2u16 {
        for x in 0..4u16 {
            let cell = frame.cell(x, y);
            assert_eq!(
                cell.fg, RED,
                "cell ({x},{y}) should carry the image, got {cell:?}"
            );
        }
    }

    // Nothing outside the placement carries the image.
    assert_ne!(frame.cell(5, 0).fg, RED);
    assert_ne!(frame.cell(0, 3).fg, RED);
}

#[test]
fn an_image_taller_than_the_pane_is_cropped_rather_than_squashed() {
    // Ten rows of image in a four-row pane: the visible part must still be solid.
    let frame = render_until(&mut pane(&red_image(3, 10), 4, 20), painted);

    for y in 0..4u16 {
        for x in 0..3u16 {
            assert_eq!(
                frame.cell(x, y).fg,
                RED,
                "cell ({x},{y}) should be inside the cropped image"
            );
        }
    }
}

/// Several images stacked down one pane all reach the frame, not just the newest.
///
/// Worth pinning: each placement is encoded and drawn separately, and images that decode from the
/// same bytes share one cached encoding, so a bug here shows up as a pane that paints only the
/// last picture a program drew.
#[test]
fn stacked_images_all_paint() {
    const GREEN: Color = Color::Rgb(0, 255, 0);

    // Three 4x2-cell images down the pane, the outer two identical so they share an encoding.
    let mut output = Vec::new();
    for (id, colour) in [(1u32, [255, 0, 0]), (2, [0, 255, 0]), (3, [255, 0, 0])] {
        output.extend_from_slice(&solid_image(id, 4, 2, colour));
        output.extend_from_slice(b"\r\n");
    }
    let frame = render_until(&mut pane(&output, 10, 20), |frame| {
        painted(frame) && frame.cell(0, 2).fg == GREEN && frame.cell(0, 4).fg == RED
    });

    for (row, colour) in [(0u16, RED), (2, GREEN), (4, RED)] {
        for y in row..row + 2 {
            for x in 0..4u16 {
                assert_eq!(
                    frame.cell(x, y).fg,
                    colour,
                    "cell ({x},{y}) belongs to the image at row {row}"
                );
            }
        }
    }
}

/// Identical pictures stacked down a pane each keep their own encoding.
///
/// A host drawing through Kitty keys a placement by its encoding's id, so two placements sharing
/// one encoding are one placement to it: it draws one and silently drops the other. Half-blocks
/// cannot show that - they paint cells directly - so this asserts the property the emitter relies
/// on, that identical pixels under different image ids do not collide.
#[test]
fn identical_stacked_images_do_not_share_an_encoding() {
    let mut output = Vec::new();
    for id in 1..=3u32 {
        output.extend_from_slice(&solid_image(id, 4, 2, [255, 0, 0]));
        output.extend_from_slice(b"\r\n");
    }

    let (mut backend, screen) = pane_with_screen(&output, 12, 20);
    // Wait for every one of them, not just the first: each is encoded separately, so a predicate
    // that only looks at the top image can return before the others have landed.
    let frame = render_until(&mut backend, |frame| {
        [0u16, 2, 4].iter().all(|row| frame.cell(0, *row).fg == RED)
    });

    // All three are on screen, and all three paint.
    for row in [0u16, 2, 4] {
        for x in 0..4u16 {
            assert_eq!(
                frame.cell(x, row).fg,
                RED,
                "the image at row {row} should be painted"
            );
        }
    }

    let images = screen.borrow_mut().render_snapshot().images.to_vec();
    assert_eq!(images.len(), 3);
    let ids: Vec<u32> = images.iter().map(|image| image.image_id).collect();
    assert_eq!(
        ids,
        vec![1, 2, 3],
        "each placement keeps the id its transmit gave it"
    );
    let hashes: Vec<u64> = images
        .iter()
        .map(|image| image.image.source_hash())
        .collect();
    assert!(
        hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "the pixels are identical - the ids are what has to keep them apart"
    );
}

#[test]
fn a_pane_with_no_graphics_paints_no_images() {
    let frame = render_until(&mut pane(b"plain text", 6, 20), |frame| {
        frame.cell(0, 0).symbol == "p"
    });
    assert_eq!(frame.cell(0, 0).symbol, "p");
    assert_ne!(frame.cell(0, 0).fg, RED);
}
