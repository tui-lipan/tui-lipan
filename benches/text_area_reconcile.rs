use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use tui_lipan::prelude::*;
use tui_lipan::{
    IMAGE_SENTINEL_BASE, ImageContent, ImageFormat, SENTINEL_BASE, TestBackend, TextAreaImageMode,
    TextAreaSentinel,
};

#[derive(Clone)]
struct BenchDataset {
    value: Arc<str>,
    cursor_positions: Vec<usize>,
    sentinels: Vec<TextAreaSentinel>,
    images: Vec<ImageContent>,
}

fn dummy_image(seed: u8) -> ImageContent {
    let bytes = [seed; 32];
    ImageContent::from_bytes(&bytes, ImageFormat::Png)
}

fn build_dataset(blocks: usize) -> BenchDataset {
    let custom0 = char::from_u32(SENTINEL_BASE as u32).unwrap_or(SENTINEL_BASE);
    let custom1 = char::from_u32(SENTINEL_BASE as u32 + 1).unwrap_or(SENTINEL_BASE);
    let image0 = char::from_u32(IMAGE_SENTINEL_BASE as u32).unwrap_or(IMAGE_SENTINEL_BASE);
    let image1 = char::from_u32(IMAGE_SENTINEL_BASE as u32 + 1).unwrap_or(IMAGE_SENTINEL_BASE);

    let mut section = String::new();
    section.push_str(
        "ASCII prose: The quick brown fox jumps over lazy logs beside a warm terminal glow.\n",
    );
    section.push_str("Long URL: https://bench.example.com/superlongpathwithoutbreaks/superlongpathwithoutbreaks/superlongpathwithoutbreaks?query=abcdefghijklmnopqrstuvwxyz0123456789\n");
    section.push_str("Mixed CJK: Rust and UI \u{6DF7}\u{5408}\u{6587}\u{672C} with \u{65E5}\u{672C}\u{8A9E} and \u{D55C}\u{AE00} in one row.\n");
    section.push_str("Arabic sample: \u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0628}\u{0643}\u{0645} \u{0641}\u{064A} \u{0627}\u{062E}\u{062A}\u{0628}\u{0627}\u{0631} \u{0625}\u{0639}\u{0627}\u{062F}\u{0629} \u{0627}\u{0644}\u{062A}\u{062F}\u{0641}\u{0642}.\n");
    section.push_str(
        "Emoji-heavy: \u{1F600}\u{1F680}\u{2728}\u{1F9EA}\u{1F525}\u{1F4C8}\u{1F916}\u{1F9E0}\u{1F9F5}\u{1F6F0}\u{FE0F} repeated for cursor movement stability.\n",
    );
    section.push_str("Hard breaks + tabs:\tcol_a\tcol_b\nline_two\nline_three\tindented\n");
    section.push_str(&format!(
        "Sentinels: custom({custom0}) then custom({custom1}) and images({image0}{image1}) inline.\n"
    ));

    let mut text = String::with_capacity(section.len() * blocks.saturating_mul(2));
    for i in 0..blocks {
        text.push_str(&section);
        text.push_str(&format!("Block {i}: reconcile and wrap stress marker.\n\n"));
    }

    let value: Arc<str> = Arc::from(text);
    let mut boundaries: Vec<usize> = value.char_indices().map(|(idx, _)| idx).collect();
    boundaries.push(value.len());

    let stride = (boundaries.len() / 24).max(1);
    let mut cursor_positions: Vec<usize> = boundaries
        .iter()
        .step_by(stride)
        .copied()
        .chain(std::iter::once(value.len()))
        .collect();
    cursor_positions.sort_unstable();
    cursor_positions.dedup();

    BenchDataset {
        value,
        cursor_positions,
        sentinels: vec![
            TextAreaSentinel::new("tag:alpha"),
            TextAreaSentinel::new("tag:beta"),
        ],
        images: vec![dummy_image(17), dummy_image(193)],
    }
}

fn with_tail(base: &BenchDataset) -> Arc<str> {
    Arc::from(format!(
        "{}\nTail: value toggle forces cache miss and reconcile.\n",
        base.value
    ))
}

struct BenchTextArea;

#[derive(Clone)]
enum BenchMsg {
    Value(Arc<str>),
    Cursor(usize),
    Wrap(bool),
}

struct BenchState {
    value: Arc<str>,
    cursor: usize,
    wrap: bool,
    sentinels: Vec<TextAreaSentinel>,
    images: Vec<ImageContent>,
}

impl Default for BenchState {
    fn default() -> Self {
        let data = build_dataset(4);
        Self {
            value: data.value,
            cursor: 0,
            wrap: true,
            sentinels: data.sentinels,
            images: data.images,
        }
    }
}

impl Component for BenchTextArea {
    type Message = BenchMsg;
    type Properties = ();
    type State = BenchState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        BenchState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            BenchMsg::Value(value) => {
                ctx.state.value = value;
                ctx.state.cursor = ctx.state.cursor.min(ctx.state.value.len());
            }
            BenchMsg::Cursor(cursor) => {
                ctx.state.cursor = cursor.min(ctx.state.value.len());
            }
            BenchMsg::Wrap(wrap) => {
                ctx.state.wrap = wrap;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::new(ctx.state.value.clone())
            .cursor(ctx.state.cursor)
            .wrap(ctx.state.wrap)
            .line_numbers(true)
            .scrollbar(true)
            .h_scrollbar(true)
            .image_mode(TextAreaImageMode::Inline)
            .image_placeholder("[Img X]")
            .sentinels(ctx.state.sentinels.clone())
            .images(ctx.state.images.clone())
            .into()
    }
}

fn bench_text_area_reconcile(c: &mut Criterion) {
    let base = build_dataset(10);
    let miss_value = with_tail(&base);
    let large = build_dataset(80);

    let mut warm_backend = TestBackend::new(BenchTextArea);
    warm_backend.state_mut().value = base.value.clone();
    warm_backend.state_mut().sentinels = base.sentinels.clone();
    warm_backend.state_mut().images = base.images.clone();
    warm_backend.state_mut().cursor = 0;
    warm_backend.render();

    let mut toggle_backend = TestBackend::new(BenchTextArea);
    toggle_backend.state_mut().value = base.value.clone();
    toggle_backend.state_mut().sentinels = base.sentinels.clone();
    toggle_backend.state_mut().images = base.images.clone();
    toggle_backend.state_mut().cursor = base.value.len() / 3;
    toggle_backend.render();

    let mut reflow_backend = TestBackend::new(BenchTextArea);
    reflow_backend.state_mut().value = base.value.clone();
    reflow_backend.state_mut().sentinels = base.sentinels.clone();
    reflow_backend.state_mut().images = base.images.clone();
    reflow_backend.state_mut().cursor = base.value.len() / 4;
    reflow_backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 132,
        h: 36,
    });
    reflow_backend.render();

    let mut cursor_backend = TestBackend::new(BenchTextArea);
    cursor_backend.state_mut().value = base.value.clone();
    cursor_backend.state_mut().sentinels = base.sentinels.clone();
    cursor_backend.state_mut().images = base.images.clone();
    cursor_backend.state_mut().cursor = 0;
    cursor_backend.render();
    let cursor_positions = base.cursor_positions.clone();

    let mut wrap_backend = TestBackend::new(BenchTextArea);
    wrap_backend.state_mut().value = large.value.clone();
    wrap_backend.state_mut().sentinels = large.sentinels.clone();
    wrap_backend.state_mut().images = large.images.clone();
    wrap_backend.state_mut().cursor = large.value.len() / 2;
    wrap_backend.state_mut().wrap = false;
    wrap_backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 84,
        h: 40,
    });
    wrap_backend.render();

    let mut group = c.benchmark_group("text_area_reconcile");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(70);
    group.sampling_mode(SamplingMode::Flat);

    group.bench_function("warm_cache_render", |b| {
        b.iter(|| {
            warm_backend.render();
            black_box(warm_backend.element());
        });
    });

    let mut flip_value = false;
    group.bench_function("cache_miss_value_toggle", |b| {
        b.iter(|| {
            flip_value = !flip_value;
            let value = if flip_value {
                base.value.clone()
            } else {
                miss_value.clone()
            };
            toggle_backend
                .dispatch(BenchMsg::Value(value))
                .expect("bench dispatch should succeed");
            black_box(toggle_backend.element());
        });
    });

    let mut wide = false;
    group.bench_function("reflow_on_width_change", |b| {
        b.iter(|| {
            wide = !wide;
            let width = if wide { 140 } else { 78 };
            reflow_backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 36,
            });
            reflow_backend.render();
            black_box(reflow_backend.element());
        });
    });

    let mut cursor_idx = 0usize;
    group.bench_function("cursor_move_same_text", |b| {
        b.iter(|| {
            cursor_idx = (cursor_idx + 1) % cursor_positions.len();
            cursor_backend
                .dispatch(BenchMsg::Cursor(cursor_positions[cursor_idx]))
                .expect("bench dispatch should succeed");
            black_box(cursor_backend.element());
        });
    });

    let mut wrap_enabled = false;
    group.bench_function("wrap_on_large_multiline_value", |b| {
        b.iter(|| {
            wrap_enabled = !wrap_enabled;
            wrap_backend
                .dispatch(BenchMsg::Wrap(wrap_enabled))
                .expect("bench dispatch should succeed");
            black_box(wrap_backend.element());
        });
    });

    group.finish();
}

criterion_group!(text_area_benches, bench_text_area_reconcile);
criterion_main!(text_area_benches);
