#![cfg(feature = "markdown")]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

fn build_plain_document(paragraphs: usize) -> String {
    let mut out = String::new();
    out.push_str("System Runbook: API Gateway Rollout\n\n");
    for i in 0..paragraphs {
        out.push_str(&format!(
            "Section {:02}: During canary deployment, monitor p95 latency, retry spikes, and \
error budget burn. If alert thresholds trigger, pause rollout and capture timeline notes for \
incident review. This paragraph is intentionally long to exercise soft-wrap and cache reflow.\n\n",
            i + 1
        ));
    }
    out
}

fn build_markdown_document(sections: usize) -> String {
    let mut out = String::new();
    out.push_str("# Release Operations Guide\n\n");
    out.push_str(
        "This document models realistic release notes with mixed markdown structures for wrapping.\n\n",
    );
    for i in 0..sections {
        out.push_str(&format!("## Service Group {}\n\n", i + 1));
        out.push_str("- Validate deployment manifests against the target cluster baseline.\n");
        out.push_str("- Confirm health probes and autoscaling windows are aligned.\n");
        out.push_str("- Record post-deploy smoke test output in the incident log.\n\n");
        out.push_str(
            "> Operators should prefer short rollback windows when dependency drift is detected.\n\n",
        );
    }
    out
}

fn build_table_markdown(rows: usize) -> String {
    let mut out = String::new();
    out.push_str("# Fleet Status\n\n");
    out.push_str("| Service | Region | Version | Uptime | SLO | Notes |\n");
    out.push_str("|:--------|:-------|:--------|-------:|----:|:------|\n");
    for i in 0..rows {
        let region = match i % 5 {
            0 => "us-east-1",
            1 => "us-west-2",
            2 => "eu-central-1",
            3 => "ap-southeast-1",
            _ => "sa-east-1",
        };
        out.push_str(&format!(
            "| gateway-{i:03} | {region} | v2.{}.{} | {}d | {}.{}% | Rolling window tracks retries, tail latency, and saturation under burst traffic. |\n",
            i % 7,
            i % 10,
            30 + (i % 360),
            99,
            (i * 3) % 10
        ));
    }
    out
}

struct PlainBenchDoc {
    value: Arc<str>,
}

struct MarkdownBenchDoc {
    value: Arc<str>,
}

struct TableBenchDoc {
    value: Arc<str>,
}

impl Component for PlainBenchDoc {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        DocumentView::new(self.value.clone())
            .wrap(true)
            .line_numbers(true)
            .scrollbar(true)
            .into()
    }
}

impl Component for MarkdownBenchDoc {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        DocumentView::new(self.value.clone())
            .markdown()
            .wrap(true)
            .line_numbers(true)
            .scrollbar(true)
            .into()
    }
}

impl Component for TableBenchDoc {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        DocumentView::new(self.value.clone())
            .markdown()
            .wrap(true)
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

fn bench_document_view_wrap(c: &mut Criterion) {
    let plain_doc: Arc<str> = Arc::from(build_plain_document(96));
    let markdown_doc: Arc<str> = Arc::from(build_markdown_document(48));
    let table_doc: Arc<str> = Arc::from(build_table_markdown(220));

    let mut plain_backend = TestBackend::new(PlainBenchDoc {
        value: plain_doc.clone(),
    });
    let mut markdown_backend = TestBackend::new(MarkdownBenchDoc {
        value: markdown_doc.clone(),
    });
    let mut table_backend = TestBackend::new(TableBenchDoc {
        value: table_doc.clone(),
    });
    let mut warm_backend = TestBackend::new(MarkdownBenchDoc {
        value: markdown_doc,
    });

    plain_backend.render();
    markdown_backend.render();
    table_backend.render();
    warm_backend.render();

    let mut group = c.benchmark_group("document_view_wrap");
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(70);
    group.sampling_mode(SamplingMode::Flat);

    let mut plain_wide = false;
    group.bench_function("plain_wrap_reflow_on_width_change", |b| {
        b.iter(|| {
            plain_wide = !plain_wide;
            let width = if plain_wide { 134 } else { 86 };
            plain_backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 34,
            });
            plain_backend.render();
            black_box(plain_backend.element());
        });
    });

    let mut markdown_wide = false;
    group.bench_function("markdown_wrap_reflow_on_width_change", |b| {
        b.iter(|| {
            markdown_wide = !markdown_wide;
            let width = if markdown_wide { 130 } else { 82 };
            markdown_backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 34,
            });
            markdown_backend.render();
            black_box(markdown_backend.element());
        });
    });

    let mut table_wide = false;
    group.bench_function("table_wrap_reflow_on_width_change", |b| {
        b.iter(|| {
            table_wide = !table_wide;
            let width = if table_wide { 144 } else { 96 };
            table_backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 34,
            });
            table_backend.render();
            black_box(table_backend.element());
        });
    });

    group.bench_function("warm_cache_render", |b| {
        b.iter(|| {
            warm_backend.render();
            black_box(warm_backend.element());
        });
    });

    group.finish();
}

criterion_group!(document_view_wrap_benches, bench_document_view_wrap);
criterion_main!(document_view_wrap_benches);
