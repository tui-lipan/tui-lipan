#![cfg(feature = "markdown")]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{
    BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

fn build_markdown(base_rows: usize) -> String {
    let mut out = String::new();
    out.push_str("# Render Benchmark\n\n");
    out.push_str("| Service | Region | Build | Duration | Status | Owner |\n");
    out.push_str("|:--------|:-------|------:|---------:|:-------|:------|\n");
    for i in 0..base_rows {
        let status = if i % 7 == 0 { "warn" } else { "ok" };
        let region = match i % 5 {
            0 => "eu-central-1",
            1 => "us-east-1",
            2 => "eu-west-1",
            3 => "ap-southeast-1",
            _ => "us-west-2",
        };
        out.push_str(&format!(
            "| svc-{i} | {region} | {} | 00:{:02}:{:02} | {status} | team-{} |\n",
            1000 + i,
            (i * 3) % 60,
            (i * 7) % 60,
            i % 9
        ));
    }
    out.push_str("\n## Notes\n\n");
    for i in 0..(base_rows / 4).max(4) {
        out.push_str(&format!(
            "- Entry {i}: long markdown paragraph for wrapping, layout and style application.\n"
        ));
    }
    out
}

struct BenchDoc;

#[derive(Default)]
struct BenchState {
    value: Arc<str>,
}

#[derive(Clone)]
enum BenchMsg {
    Set(Arc<str>),
}

impl Component for BenchDoc {
    type Message = BenchMsg;
    type Properties = ();
    type State = BenchState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        BenchState {
            value: Arc::from(build_markdown(64)),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            BenchMsg::Set(value) => {
                ctx.state.value = value;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        DocumentView::new(ctx.state.value.clone())
            .markdown()
            .wrap(false)
            .table_wrap(true)
            .table_width_mode(DocumentTableWidthMode::Fill)
            .table_outer_frame(true)
            .table_column_separators(true)
            .table_cell_padding(1)
            .line_numbers(true)
            .scrollbar(true)
            .h_scrollbar(true)
            .into()
    }
}

fn bench_markdown_formatter(c: &mut Criterion) {
    let formatter = MarkdownFormatter::default();
    let docs = [
        ("small", build_markdown(32)),
        ("medium", build_markdown(256)),
        ("large", build_markdown(1024)),
    ];

    let mut group = c.benchmark_group("document_view_markdown_formatter");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(8));
    group.sample_size(80);
    for (name, doc) in &docs {
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), doc, |b, input| {
            b.iter(|| {
                black_box(formatter.format(FormatInput {
                    value: black_box(input.as_str()),
                    content_type: Some("markdown"),
                    document_styles: None,
                }))
            });
        });
    }
    group.finish();
}

fn bench_document_view_reconcile(c: &mut Criterion) {
    let doc_a: Arc<str> = Arc::from(build_markdown(256));
    let doc_b: Arc<str> = Arc::from(format!("{}\n\nextra tail", doc_a));

    let mut backend = TestBackend::new(BenchDoc);
    backend.state_mut().value = doc_a.clone();
    backend.render();

    let mut group = c.benchmark_group("document_view_markdown_reconcile");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(70);
    group.sampling_mode(SamplingMode::Flat);

    group.bench_function("warm_cache_render", |b| {
        b.iter(|| {
            backend.render();
            black_box(backend.element());
        });
    });

    let mut flip = false;
    group.bench_function("cache_miss_value_toggle", |b| {
        b.iter(|| {
            flip = !flip;
            let value = if flip { doc_a.clone() } else { doc_b.clone() };
            backend
                .dispatch(BenchMsg::Set(value))
                .expect("bench dispatch should succeed");
            black_box(backend.element());
        });
    });

    let mut wide = false;
    group.bench_function("reflow_on_width_change", |b| {
        b.iter(|| {
            wide = !wide;
            let width = if wide { 140 } else { 88 };
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 36,
            });
            backend.render();
            black_box(backend.element());
        });
    });

    group.finish();
}

criterion_group!(
    document_view_benches,
    bench_markdown_formatter,
    bench_document_view_reconcile
);
criterion_main!(document_view_benches);
