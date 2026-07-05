use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const ROWS: usize = 500;

#[derive(Clone)]
enum Msg {
    Toggle,
}

#[derive(Default)]
struct State {
    flipped: bool,
}

struct KeyedReorderBench;

impl Component for KeyedReorderBench {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle => {
                ctx.state.flipped = !ctx.state.flipped;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        ScrollView::new()
            .children(keyed_rows(ctx.state.flipped))
            .into()
    }
}

struct UnkeyedReorderBench;

impl Component for UnkeyedReorderBench {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle => {
                ctx.state.flipped = !ctx.state.flipped;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        ScrollView::new()
            .children(unkeyed_rows(ctx.state.flipped))
            .into()
    }
}

fn keyed_rows(flipped: bool) -> impl Iterator<Item = Element> {
    let iter: Box<dyn Iterator<Item = usize>> = if flipped {
        Box::new((0..ROWS).rev())
    } else {
        Box::new(0..ROWS)
    };

    iter.map(|i| Text::new(format!("row-{i:04}")).key(format!("id-{i}")))
}

fn unkeyed_rows(flipped: bool) -> impl Iterator<Item = Element> {
    let iter: Box<dyn Iterator<Item = usize>> = if flipped {
        Box::new((0..ROWS).rev())
    } else {
        Box::new(0..ROWS)
    };

    iter.map(|i| Text::new(format!("row-{i:04}")).into())
}

fn setup_keyed_backend() -> TestBackend<KeyedReorderBench> {
    let mut backend = TestBackend::new(KeyedReorderBench);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();
    backend
}

fn setup_unkeyed_backend() -> TestBackend<UnkeyedReorderBench> {
    let mut backend = TestBackend::new(UnkeyedReorderBench);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();
    backend
}

fn bench_scroll_reorder_reconcile(c: &mut Criterion) {
    let mut group = c.benchmark_group("scroll_reorder_reconcile");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(60);
    group.sampling_mode(SamplingMode::Flat);

    let mut keyed = setup_keyed_backend();
    group.bench_function("keyed_reorder_500", |b| {
        b.iter(|| {
            keyed
                .dispatch(Msg::Toggle)
                .expect("dispatch should succeed");
            black_box(keyed.element());
        });
    });

    let mut unkeyed = setup_unkeyed_backend();
    group.bench_function("unkeyed_reorder_500", |b| {
        b.iter(|| {
            unkeyed
                .dispatch(Msg::Toggle)
                .expect("dispatch should succeed");
            black_box(unkeyed.element());
        });
    });

    group.finish();
}

criterion_group!(
    scroll_reorder_reconcile_benches,
    bench_scroll_reorder_reconcile
);
criterion_main!(scroll_reorder_reconcile_benches);
