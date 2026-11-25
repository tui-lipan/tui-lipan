//! A selection step across a large list, as text rows and as composed rows. Both paths render the
//! same items; the composed one builds a row element per item.
use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const ROWS: usize = 1000;

struct Rows {
    composed: bool,
}

impl Component for Rows {
    type Message = ();
    type Properties = ();
    type State = usize;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        0
    }

    fn update(&mut self, _msg: (), ctx: &mut Context<Self>) -> Update {
        ctx.state = (ctx.state + 1) % ROWS;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let composed = self.composed;
        List::new()
            .items((0..ROWS).map(|index| {
                let label = format!("row-{index:04}");
                let item = ListItem::new(label.clone()).description("12m");
                if composed {
                    item.key(label.clone()).content(
                        HStack::new()
                            .child(Text::new(label).width(Length::Flex(1)))
                            .child(Text::new("12m")),
                    )
                } else {
                    item
                }
            }))
            .selected(ctx.state)
            .force_scroll_to_selected(true)
            .into()
    }
}

fn backend(composed: bool) -> TestBackend<Rows> {
    let mut backend = TestBackend::new(Rows { composed });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();
    backend
}

fn bench_list_rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_rows");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(40);
    group.sampling_mode(SamplingMode::Flat);

    for (name, composed) in [("text_select_1000", false), ("composed_select_1000", true)] {
        let mut backend = backend(composed);
        group.bench_function(name, |b| {
            b.iter(|| {
                backend.dispatch(()).expect("dispatch should succeed");
                black_box(backend.capture_frame());
            });
        });
    }

    group.finish();
}

criterion_group!(list_rows_benches, bench_list_rows);
criterion_main!(list_rows_benches);
