#![cfg(feature = "markdown")]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const CHILD_COUNT: usize = 320;

struct ScrollViewRichChildrenBench;

#[derive(Default)]
struct BenchState {
    offset: usize,
    messages: Vec<Arc<str>>,
}

#[derive(Clone)]
enum BenchMsg {
    SetOffset(usize),
}

impl Component for ScrollViewRichChildrenBench {
    type Message = BenchMsg;
    type Properties = ();
    type State = BenchState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        BenchState {
            offset: 0,
            messages: build_rich_markdown_messages(CHILD_COUNT),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            BenchMsg::SetOffset(offset) => {
                ctx.state.offset = offset;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let children = ctx
            .state
            .messages
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, body)| {
                Frame::new()
                    .height(Length::Auto)
                    .border(true)
                    .padding((1, 1, 1, 1))
                    .child(
                        DocumentView::new(body)
                            .markdown()
                            .height(Length::Auto)
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .wrap(true)
                            .table_wrap(true),
                    )
                    .key(format!("msg-{i}"))
            });

        ScrollView::new()
            .offset(ctx.state.offset)
            .scrollbar(true)
            .gap(1)
            .padding(1)
            .children(children)
            .into()
    }
}

fn build_rich_markdown_messages(count: usize) -> Vec<Arc<str>> {
    let templates = [
        "## Incident Timeline\n\nA deploy in `us-east-1` caused retries to spike from 0.2% to 3.8%. The mitigation toggled a feature gate and traffic normalized after two rollout steps.\n\n- service: gateway\n- cluster: prod-a\n- action: drained bad replicas\n",
        "### Build Report\n\n| Step | Duration | Result |\n|:-----|---------:|:-------|\n| lint | 00:01:23 | ok |\n| test | 00:04:41 | ok |\n| package | 00:00:49 | ok |\n\nArtifacts are retained for seven days and mirrored to cold storage.",
        "```rust\nfn reconcile_visible_rows(rows: usize, viewport: usize) -> usize {\n    rows.saturating_sub(viewport / 2)\n}\n```\n\nThis block intentionally includes longer prose around code to force line wrapping in narrower viewports.",
        "> Customer feedback indicates that scrolling remains smooth near the top but degrades when the view jumps to deeply nested content with mixed markdown structures.\n\nFollow-up: compare warm and cold layout passes.",
        "1. Validate scrollbar thumb updates\n2. Compare estimated vs measured child heights\n3. Profile markdown formatting cache hit rate\n\nParagraph: The renderer should gracefully handle long inline text like `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` without clipping.",
        "#### Notes\n\nCross-team handoff details:\n- owner: platform-observability\n- ticket: OPS-1429\n- status: monitoring\n\nAdditional context with **bold**, *italic*, and a [runbook](https://example.invalid/runbook) link to keep parser paths hot.",
    ];

    (0..count)
        .map(|i| {
            let body = templates[i % templates.len()];
            Arc::<str>::from(format!(
                "Message {i}\n\n{body}\n\nWrap probe: shard={} region={} attempt={} checksum={}\n",
                i % 16,
                i % 5,
                i % 3,
                10_000 + i
            ))
        })
        .collect()
}

fn make_backend(width: u16, height: u16) -> TestBackend<ScrollViewRichChildrenBench> {
    let mut backend = TestBackend::new(ScrollViewRichChildrenBench);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: width,
        h: height,
    });
    backend.render();
    backend
}

fn bench_scroll_view_rich_children(c: &mut Criterion) {
    let mut group = c.benchmark_group("scroll_view_rich_children");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(60);
    group.sampling_mode(SamplingMode::Flat);

    let mut warm_backend = make_backend(108, 40);
    group.bench_function("warm_render_large_list", |b| {
        b.iter(|| {
            warm_backend.render();
            black_box(warm_backend.element());
        });
    });

    let mut offset_backend = make_backend(108, 40);
    let mut offset = 0usize;
    group.bench_function("scroll_offset_change", |b| {
        b.iter(|| {
            offset = offset.saturating_add(7);
            offset_backend
                .dispatch(BenchMsg::SetOffset(offset))
                .expect("bench dispatch should succeed");
            black_box(offset_backend.element());
        });
    });

    let mut width_backend = make_backend(108, 40);
    let mut wide = false;
    group.bench_function("width_change_many_auto_height_children", |b| {
        b.iter(|| {
            wide = !wide;
            let width = if wide { 128 } else { 72 };
            width_backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            });
            width_backend.render();
            black_box(width_backend.element());
        });
    });

    let mut jump_backend = make_backend(108, 40);
    let mut far = false;
    group.bench_function("jump_to_far_offset", |b| {
        b.iter(|| {
            far = !far;
            let target = if far { 8_000 } else { 0 };
            jump_backend
                .dispatch(BenchMsg::SetOffset(target))
                .expect("bench dispatch should succeed");
            black_box(jump_backend.element());
        });
    });

    group.finish();
}

criterion_group!(
    scroll_view_rich_children_benches,
    bench_scroll_view_rich_children
);
criterion_main!(scroll_view_rich_children_benches);
