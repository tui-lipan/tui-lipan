//! Rendering a terminal pane whose child drew a full-pane picture, with and without a modal
//! backdrop over it or an `EffectScope` around it. Each iteration captures a whole frame, as a live
//! stream draws one. Under a backdrop or effect each frame recolors the picture's pixels, so the
//! difference between `closed` and the other cases is what that costs. The capture's own half-block
//! painting is common to all.

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const CELL: TerminalCellSize = TerminalCellSize {
    width: 10,
    height: 20,
};
const COLS: u16 = 192;
const ROWS: u16 = 54;

/// What recolors the pane.
#[derive(Clone)]
enum Layer {
    Closed,
    Backdrop(Style),
    Scope(VisualEffect),
}

struct Pane {
    screen: Rc<RefCell<TerminalScreen>>,
    layer: Layer,
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
        let terminal = Terminal::new()
            .screen(TerminalScreenHandle::new(Rc::clone(&self.screen)))
            .scrollbar(false);
        match &self.layer {
            Layer::Closed => terminal.into(),
            Layer::Backdrop(backdrop) => ZStack::new()
                .child(terminal)
                .child(
                    Modal::new()
                        .width(Length::Px(30))
                        .height(Length::Px(5))
                        .backdrop_style(*backdrop)
                        .child(Text::new("dialog")),
                )
                .into(),
            Layer::Scope(effect) => EffectScope::new()
                .effect(effect.clone())
                .child(terminal)
                .into(),
        }
    }
}

/// A web page: a few flat colors in bands and blocks.
fn page(x: u32, y: u32) -> [u8; 3] {
    if (y / 40).is_multiple_of(3) {
        [245, 245, 245]
    } else if (x / 200 + y / 90).is_multiple_of(5) {
        [30, 90, 200]
    } else {
        [255, 255, 255]
    }
}

/// A photo, at its worst: nearly every pixel a different color.
fn photo(x: u32, y: u32) -> [u8; 3] {
    let mut seed = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) | 1;
    seed ^= seed << 13;
    seed ^= seed >> 17;
    seed ^= seed << 5;
    [
        (x % 256) as u8 ^ (seed >> 24) as u8,
        (y % 256) as u8,
        ((x + y) % 256) as u8 ^ (seed >> 16) as u8,
    ]
}

fn pane(pixel: fn(u32, u32) -> [u8; 3], layer: Layer) -> TestBackend<Pane> {
    let (width, height) = (
        u32::from(COLS) * u32::from(CELL.width),
        u32::from(ROWS) * u32::from(CELL.height),
    );
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&pixel(x, y));
        }
    }
    let mut screen = TerminalScreen::new(ROWS, COLS, 0);
    screen.set_cell_size(CELL);
    screen.process_bytes(
        format!(
            "\x1b_Ga=T,f=24,s={width},v={height},t=d,i=1,C=1;{}\x1b\\",
            BASE64.encode(pixels)
        )
        .as_bytes(),
    );

    let mut backend = TestBackend::new(Pane {
        screen: Rc::new(RefCell::new(screen)),
        layer,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: COLS,
        h: ROWS,
    });
    backend.render();
    backend
}

fn backdrops(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_backdrop");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(5));
    let layers = [
        ("closed", Layer::Closed),
        (
            "tint",
            Layer::Backdrop(Style::new().tint_by(Color::Rgb(0, 0, 40), 0.5)),
        ),
        (
            "elevate",
            Layer::Backdrop(Style::new().transform_bg(ColorTransform::Elevate(0.5))),
        ),
        ("scope_dim", Layer::Scope(VisualEffect::dim(0.5))),
        (
            "scope_monochrome",
            Layer::Scope(VisualEffect::Monochrome { strength: 1.0 }),
        ),
    ];
    for (content, pixel) in [("page", page as fn(u32, u32) -> [u8; 3]), ("photo", photo)] {
        for (name, layer) in &layers {
            let backend = pane(pixel, layer.clone());
            group.bench_function(BenchmarkId::new(content, name), |b| {
                b.iter(|| black_box(backend.capture_frame()));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, backdrops);
criterion_main!(benches);
