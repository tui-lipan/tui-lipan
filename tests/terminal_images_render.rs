//! A Kitty graphics command from a child program ends up as pixels in the captured frame.
//!
//! A capture does not encode images for a host. It records their pixels in `CapturedFrame::images`,
//! marks which cells still show them after everything above has drawn, and paints those cells as
//! half blocks, which is what most of these tests read back out of the cell grid.

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
/// A capture records images synchronously, so the first frame normally has them; the retry only
/// keeps a test from depending on that.
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

/// A pane with a line of text drawn over its top row, as an overlay or a floating pane would be.
struct CoveredPane {
    screen: Rc<RefCell<TerminalScreen>>,
    label: &'static str,
}

impl Component for CoveredPane {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ZStack::new()
            .child(
                Terminal::new()
                    .screen(TerminalScreenHandle::new(Rc::clone(&self.screen)))
                    .scrollbar(false),
            )
            .child(Text::new(self.label))
            .into()
    }
}

fn covered_pane(output: &[u8]) -> TestBackend<CoveredPane> {
    labelled_pane(output, "OVER")
}

fn labelled_pane(output: &[u8], label: &'static str) -> TestBackend<CoveredPane> {
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(output);
    let mut backend = TestBackend::new(CoveredPane {
        screen: Rc::new(RefCell::new(screen)),
        label,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    });
    backend.render();
    backend
}

#[test]
fn a_capture_carries_the_image_pixels_beside_the_cells() {
    let frame = pane(&red_image(4, 2), 6, 20).capture_frame();

    assert_eq!(frame.images.len(), 1);
    let image = &frame.images[0];
    assert_eq!(
        image.area,
        Rect {
            x: 0,
            y: 0,
            w: 4,
            h: 2
        }
    );
    assert_eq!((image.width, image.height), (40, 40));
    assert!(
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [255, 0, 0, 255])
    );
    assert!(image.visible.iter().all(|&visible| visible));
    assert_eq!(frame.cell(0, 0).symbol, "\u{2580}");
}

#[test]
fn whatever_is_drawn_over_an_image_hides_it_in_those_cells() {
    let frame = covered_pane(&red_image(6, 2)).capture_frame();

    let image = &frame.images[0];
    let text: String = (0..4).map(|x| frame.cell(x, 0).symbol.clone()).collect();
    assert_eq!(text, "OVER", "the text above the image survives");
    for x in 0..4 {
        assert!(!image.shows(x, 0), "cell ({x},0) is under the text");
    }
    assert!(image.shows(4, 0) && image.shows(5, 0));
    assert!((0..6).all(|x| image.shows(x, 1)));
    assert_eq!(frame.cell(4, 0).fg, RED);
}

#[test]
fn a_screen_capture_includes_the_images_the_program_displayed() {
    let mut screen = TerminalScreen::new(4, 20, 100);
    screen.set_cell_size(CELL);
    // Ten rows of image in a four-row screen: cropped to what is visible, not squashed.
    screen.process_bytes(&red_image(3, 10));
    let frame = screen.capture_frame();

    assert_eq!(frame.images.len(), 1);
    let image = &frame.images[0];
    assert_eq!(
        image.area,
        Rect {
            x: 0,
            y: 0,
            w: 3,
            h: 4
        }
    );
    assert_eq!(
        (image.width, image.height),
        (30, 80),
        "the source rows below the screen are cropped away"
    );
    for y in 0..4u16 {
        for x in 0..3u16 {
            assert_eq!(frame.cell(x, y).fg, RED, "cell ({x},{y})");
        }
    }
    assert_ne!(frame.cell(3, 0).fg, RED);
}

/// A `cols` x `rows` cell image whose every cell-high band is green in its top quarter and blue
/// below: detail no single cell color can stand in for.
#[cfg(feature = "ui-snapshot-png")]
fn banded_image(cols: u32, rows: u32) -> Vec<u8> {
    let (width, height) = (cols * u32::from(CELL.width), rows * u32::from(CELL.height));
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        let colour = if y % u32::from(CELL.height) < u32::from(CELL.height) / 4 {
            [0, 255, 0]
        } else {
            [0, 0, 255]
        };
        for _ in 0..width {
            pixels.extend_from_slice(&colour);
        }
    }
    format!(
        "\x1b_Ga=T,f=24,s={width},v={height},t=d,i=1;{}\x1b\\",
        BASE64.encode(pixels)
    )
    .into_bytes()
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn a_png_draws_the_image_pixels_only_where_the_image_shows() {
    let frame = covered_pane(&banded_image(6, 2)).capture_frame();
    let options = tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        default_bg: Color::Rgb(0, 0, 0),
        ..tui_lipan::PngOptions::default()
    };
    let png = frame.to_png(&options).expect("encode");
    let decoded = image::load_from_memory(&png).expect("decode").to_rgb8();
    let pixel = |x: u32, y: u32| decoded.get_pixel(x, y).0;

    // Row 1 shows the image's own pixels, scaled to 16-pixel cells: green at the top of the band,
    // blue below. A cell color alone could only show one of the two.
    for x in 0..6 * 8 {
        assert_eq!(pixel(x, 17), [0, 255, 0], "pixel ({x},17)");
        assert_eq!(pixel(x, 28), [0, 0, 255], "pixel ({x},28)");
    }
    // The text over row 0 covers the image there; a cell it left alone still shows it.
    assert_ne!(pixel(4, 1), [0, 255, 0]);
    assert_eq!(pixel(4 * 8 + 4, 1), [0, 255, 0]);
    // Past the image, the background.
    assert_eq!(pixel(7 * 8, 20), [0, 0, 0]);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn transparent_image_pixels_show_what_is_behind_the_image_not_its_stand_in() {
    // Red, except the bottom eighth of each cell-high band, which is fully transparent. The lower
    // half of every cell is mostly red, so its half-block stand-in has a red background; the PNG
    // must still show the pane's own background through the transparent pixels.
    let (width, height) = (4 * u32::from(CELL.width), u32::from(CELL.height));
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let alpha = if y >= height * 7 / 8 { 0 } else { 255 };
        for _ in 0..width {
            pixels.extend_from_slice(&[255, 0, 0, alpha]);
        }
    }
    let command = format!(
        "\x1b_Ga=T,f=32,s={width},v={height},t=d,i=1;{}\x1b\\",
        BASE64.encode(pixels)
    );
    let frame = pane(command.as_bytes(), 4, 20).capture_frame();
    assert_eq!(frame.cell(0, 0).bg, RED, "the stand-in's lower half is red");

    let options = tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        default_bg: Color::Rgb(0, 0, 0),
        ..tui_lipan::PngOptions::default()
    };
    let png = frame.to_png(&options).expect("encode");
    let decoded = image::load_from_memory(&png).expect("decode").to_rgb8();
    assert_eq!(decoded.get_pixel(4, 2).0, [255, 0, 0]);
    assert_eq!(
        decoded.get_pixel(4, 15).0,
        [0, 0, 0],
        "the transparent bottom shows the pane background"
    );
}

#[test]
fn private_use_icons_survive_a_capture_that_holds_images() {
    // Nerd Font icons are private-use code points (U+F05B2, U+F06E4), and so were earlier forms of
    // the capture's image marks (U+F0000, U+F0001, U+10F000). None of them may be taken for a mark:
    // one drawn over the image covers it, and one drawn beside it stays text.
    // Over the image: the earlier marks for image 0 (U+10F000, U+F0000) and a Nerd Font icon.
    let label = "\u{10F000} \u{F05B2} \u{F0000} \u{F06E4} \u{F0001}";
    let frame = labelled_pane(&red_image(6, 2), label).capture_frame();
    let image = &frame.images[0];

    for (x, symbol) in [(0, "\u{10F000}"), (2, "\u{F05B2}"), (4, "\u{F0000}")] {
        assert_eq!(
            frame.cell(x, 0).symbol,
            symbol,
            "cell ({x},0) over the image"
        );
        assert!(!image.shows(x, 0), "the icon at ({x},0) covers the image");
    }
    // Outside the six-column image, while an image is in the frame.
    assert_eq!(frame.cell(6, 0).symbol, "\u{F06E4}");
    assert_eq!(frame.cell(8, 0).symbol, "\u{F0001}");
    assert!(
        (0..6).all(|x| image.shows(x, 1)),
        "the image still shows where no text covers it"
    );
}

/// What `clear` sends: home, erase the screen, erase the scrollback.
const CLEAR: &[u8] = b"\x1b[H\x1b[2J\x1b[3J";

#[test]
fn clearing_the_screen_and_scrollback_removes_the_images_on_it() {
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(&red_image(4, 2));
    screen.process_bytes(b"\r\n$ ");
    assert_eq!(screen.render_snapshot().images.len(), 1);

    screen.process_bytes(CLEAR);

    assert!(
        screen.render_snapshot().images.is_empty(),
        "the image went with the screen and scrollback it was on"
    );
    assert!(screen.capture_frame().images.is_empty());

    // A later image still lands where it is drawn.
    screen.process_bytes(&red_image(2, 1));
    let images = screen.render_snapshot().images.to_vec();
    assert_eq!(images.len(), 1);
    assert_eq!((images[0].row, images[0].col), (0, 0));
}

#[test]
fn erasing_the_screen_alone_scrolls_an_image_away_with_its_text() {
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(b"above\r\n");
    screen.process_bytes(&red_image(4, 2));
    screen.process_bytes(b"\r\n$ ");

    screen.process_bytes(b"\x1b[H\x1b[2J");

    assert!(
        screen.render_snapshot().images.is_empty(),
        "no longer on screen"
    );
}

/// `RIS` replaces both grids, so it has to take the images on screen too, not only those in
/// scrollback: with no history at all, nothing would be counted as gone.
#[test]
fn a_hard_reset_removes_images_from_a_screen_with_no_history() {
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(&red_image(4, 2));
    assert_eq!(screen.render_snapshot().images.len(), 1);

    screen.process_bytes(b"\x1bc");

    assert!(screen.render_snapshot().images.is_empty());
    assert!(screen.capture_frame().images.is_empty());
    screen.process_bytes(&red_image(2, 1));
    assert_eq!(
        screen.render_snapshot().images.len(),
        1,
        "later images still show"
    );
}

#[test]
fn a_hard_reset_on_the_alternate_screen_removes_the_images_of_both() {
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(&solid_image(1, 4, 2, [255, 0, 0]));
    screen.process_bytes(b"\x1b[?1049h");
    screen.process_bytes(&solid_image(2, 4, 2, [0, 255, 0]));
    assert_eq!(screen.render_snapshot().images.len(), 1);

    screen.process_bytes(b"\x1bc");

    assert!(
        screen.render_snapshot().images.is_empty(),
        "primary screen after the reset"
    );
    screen.process_bytes(b"\x1b[?1049h");
    assert!(
        screen.render_snapshot().images.is_empty(),
        "alternate screen after the reset"
    );
}

/// A pane with a modal that can be opened over it, dimming the pane through its backdrop.
struct ModalPane {
    screen: Rc<RefCell<TerminalScreen>>,
    backdrop: Style,
}

impl Component for ModalPane {
    type Message = bool;
    type Properties = ();
    type State = bool;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        false
    }

    fn update(&mut self, open: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state = open;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut root = ZStack::new().child(
            Terminal::new()
                .screen(TerminalScreenHandle::new(Rc::clone(&self.screen)))
                .scrollbar(false),
        );
        if ctx.state {
            root = root.child(
                Modal::new()
                    .width(Length::Px(6))
                    .height(Length::Px(2))
                    .border(false)
                    .padding(0)
                    .backdrop_style(self.backdrop)
                    .child(Text::new("modal")),
            );
        }
        root.into()
    }
}

/// The backdrop Rozi's dialogs use: recede toward a dark surface.
fn recede() -> Style {
    Style::new().tint_by(Color::Rgb(0, 0, 40), 0.5)
}

/// A red image in the top-left corner and, on the bottom row, text on a red background: the
/// cells an image under a backdrop has to match.
fn modal_pane(backdrop: Style) -> TestBackend<ModalPane> {
    let mut output = red_image(4, 2);
    output.extend_from_slice(b"\x1b[6;11H\x1b[48;2;255;0;0mtext\x1b[0m");
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(&output);
    let mut backend = TestBackend::new(ModalPane {
        screen: Rc::new(RefCell::new(screen)),
        backdrop,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    });
    backend.render();
    backend
}

fn set_modal(backend: &mut TestBackend<ModalPane>, open: bool) -> CapturedFrame {
    backend.dispatch(open).expect("dispatch");
    backend.advance(Duration::from_secs(1));
    backend.render();
    backend.capture_frame()
}

/// Every pixel of the frame's one image.
fn image_pixels(frame: &CapturedFrame) -> Vec<[u8; 4]> {
    assert_eq!(frame.images.len(), 1, "the image is in the frame");
    frame.images[0].rgba.as_chunks::<4>().0.to_vec()
}

fn rgb(color: Color) -> [u8; 4] {
    let Color::Rgb(r, g, b) = color else {
        panic!("expected a truecolor cell, got {color:?}");
    };
    [r, g, b, 255]
}

#[test]
fn a_backdrop_dims_image_pixels_like_the_cells_beside_them() {
    let mut backend = modal_pane(recede());
    let frame = set_modal(&mut backend, true);

    let text_bg = frame.cell(10, 5).bg;
    assert_ne!(text_bg, RED, "the backdrop dims the cells");
    let dimmed = rgb(text_bg);
    assert!(
        image_pixels(&frame).iter().all(|&pixel| pixel == dimmed),
        "every image pixel dims to the color of the red cells beside it, {dimmed:?}"
    );
    assert_eq!(
        frame.cell(0, 0).fg,
        text_bg,
        "the half-block stand-in is painted from the dimmed pixels"
    );
}

#[test]
fn image_pixels_are_untouched_without_a_backdrop() {
    let mut backend = modal_pane(Style::default());
    assert!(
        image_pixels(&backend.capture_frame())
            .iter()
            .all(|&pixel| pixel == [255, 0, 0, 255]),
        "no overlay, nothing dims"
    );

    let frame = set_modal(&mut backend, true);
    assert_eq!(
        frame.cell(10, 5).bg,
        RED,
        "a modal without a backdrop dims no cells"
    );
    assert!(
        image_pixels(&frame)
            .iter()
            .all(|&pixel| pixel == [255, 0, 0, 255]),
        "and no pixels"
    );
}

#[test]
fn closing_the_overlay_restores_the_original_pixels() {
    let mut backend = modal_pane(recede());
    let open = set_modal(&mut backend, true);
    assert!(
        image_pixels(&open)
            .iter()
            .all(|&pixel| pixel != [255, 0, 0, 255])
    );

    let closed = set_modal(&mut backend, false);
    assert_eq!(closed.cell(10, 5).bg, RED);
    assert!(
        image_pixels(&closed)
            .iter()
            .all(|&pixel| pixel == [255, 0, 0, 255]),
        "the image returns to full brightness with the cells"
    );
    assert_eq!(closed.cell(0, 0).fg, RED);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn a_png_of_an_open_backdrop_draws_the_dimmed_pixels() {
    let mut backend = modal_pane(recede());
    let frame = set_modal(&mut backend, true);
    let dimmed = rgb(frame.cell(10, 5).bg);
    let options = tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        default_bg: Color::Rgb(0, 0, 0),
        ..tui_lipan::PngOptions::default()
    };
    let png = frame.to_png(&options).expect("encode");
    let decoded = image::load_from_memory(&png).expect("decode").to_rgb8();
    for (x, y) in [(0, 0), (16, 8), (31, 31)] {
        assert_eq!(
            decoded.get_pixel(x, y).0,
            [dimmed[0], dimmed[1], dimmed[2]],
            "pixel ({x},{y})"
        );
    }
}

/// What sits around or over the pane in [`LayeredPane`].
#[derive(Clone)]
enum Layer {
    /// An `EffectScope` wrapping the pane.
    Scope(VisualEffect),
    /// An `Animated` wrapping the pane, faded toward a color.
    Fade(f32, Color),
    /// A `Local`-scope modal over the pane.
    LocalModal(Style),
    /// A `Center` the pane draws inside, painting its style first.
    Surface(Style),
    /// An `EffectScope` stacked beneath the pane, so it has run by the time the pane draws.
    Beneath(VisualEffect),
}

struct LayeredPane {
    screen: Rc<RefCell<TerminalScreen>>,
    layer: Layer,
}

impl Component for LayeredPane {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let pane = Terminal::new()
            .screen(TerminalScreenHandle::new(Rc::clone(&self.screen)))
            .scrollbar(false);
        match &self.layer {
            Layer::Scope(effect) => EffectScope::new().effect(effect.clone()).child(pane).into(),
            Layer::Fade(opacity, target) => Animated::new(pane)
                .opacity(*opacity)
                .opacity_target(*target)
                .into(),
            Layer::LocalModal(backdrop) => ZStack::new()
                .child(pane)
                .child(
                    Modal::new()
                        .scope(OverlayScope::Local)
                        .width(Length::Px(6))
                        .height(Length::Px(2))
                        .border(false)
                        .padding(0)
                        .backdrop_style(*backdrop)
                        .child(Text::new("modal")),
                )
                .into(),
            Layer::Surface(style) => Center::new()
                .width(Size::Percent(100))
                .height(Size::Percent(100))
                .style(*style)
                .child(pane)
                .into(),
            Layer::Beneath(effect) => ZStack::new()
                .child(
                    EffectScope::new().effect(effect.clone()).child(
                        Center::new()
                            .width(Size::Percent(100))
                            .height(Size::Percent(100)),
                    ),
                )
                .child(pane)
                .into(),
        }
    }
}

/// [`modal_pane`]'s screen, a red image and red text cells, under `layer`.
fn layered_pane(layer: Layer) -> CapturedFrame {
    let mut output = red_image(4, 2);
    output.extend_from_slice(b"\x1b[6;11H\x1b[48;2;255;0;0mtext\x1b[0m");
    let mut screen = TerminalScreen::new(6, 20, 100);
    screen.set_cell_size(CELL);
    screen.process_bytes(&output);
    let mut backend = TestBackend::new(LayeredPane {
        screen: Rc::new(RefCell::new(screen)),
        layer,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    });
    backend.render();
    backend.advance(Duration::from_secs(1));
    backend.render();
    backend.capture_frame()
}

/// Every image pixel took the color the red text cells did, and that color is not red.
fn assert_pixels_match_the_cells(frame: &CapturedFrame, what: &str) {
    let text_bg = frame.cell(10, 5).bg;
    assert_ne!(text_bg, RED, "{what} recolors the cells");
    let recolored = rgb(text_bg);
    assert!(
        image_pixels(frame).iter().all(|&pixel| pixel == recolored),
        "{what}: every image pixel is {recolored:?}, like the red cells beside it"
    );
}

#[test]
fn an_effect_scope_dims_image_pixels_like_the_cells_it_dims() {
    let frame = layered_pane(Layer::Scope(VisualEffect::dim(0.5)));
    assert_pixels_match_the_cells(&frame, "a dimming scope");
}

#[test]
fn an_effect_that_mixes_channels_recolors_image_pixels_exactly() {
    let frame = layered_pane(Layer::Scope(VisualEffect::Monochrome { strength: 1.0 }));
    assert_pixels_match_the_cells(&frame, "a monochrome scope");
}

#[test]
fn an_animated_fade_toward_a_color_reaches_image_pixels() {
    let frame = layered_pane(Layer::Fade(0.4, Color::Rgb(0, 0, 40)));
    assert_pixels_match_the_cells(&frame, "a fade");
}

#[test]
fn a_local_modal_backdrop_dims_the_image_it_covers() {
    let frame = layered_pane(Layer::LocalModal(recede()));
    assert_pixels_match_the_cells(&frame, "a local backdrop");
}

#[test]
fn passes_that_leave_cell_backgrounds_alone_leave_the_pixels_alone() {
    for layer in [
        Layer::Scope(VisualEffect::dim(0.5).foreground_only()),
        Layer::Surface(Style::new().dim_by(0.5)),
        Layer::Beneath(VisualEffect::dim(0.5)),
    ] {
        let frame = layered_pane(layer);
        assert_eq!(frame.cell(10, 5).bg, RED, "the cells keep their color");
        assert!(
            image_pixels(&frame)
                .iter()
                .all(|&pixel| pixel == [255, 0, 0, 255]),
            "and so do the pixels"
        );
    }
}
