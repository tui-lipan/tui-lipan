use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tui_lipan::prelude::TerminalScreen;

fn plain_lines() -> Vec<u8> {
    const SIZE: usize = 256 * 1024;
    let line =
        b"2026-07-14T12:34:56.789Z INFO worker=17 request=8f31 elapsed_ms=12 path=/api/items\r\n";
    let mut bytes = Vec::with_capacity(SIZE);
    while bytes.len() < SIZE {
        let remaining = SIZE - bytes.len();
        bytes.extend_from_slice(&line[..remaining.min(line.len())]);
    }
    bytes
}

// Same inputs, viewport and timed loop as Rozi's original OSC 7501 ingestion measurement.
fn program_status_ingest(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_status_ingest");
    let progress = (0..1000)
        .map(|i| {
            format!(
                "\x1b]7501;state=working:app=cargo:progress={}:msg=QnVpbGRpbmc=\x1b\\",
                i % 101
            )
        })
        .collect::<String>()
        .into_bytes();
    for (name, corpus) in [("plain", plain_lines()), ("progress", progress)] {
        for mode in ["disabled_empty", "disabled_retained", "enabled"] {
            group.throughput(Throughput::Bytes(corpus.len() as u64));
            group.bench_with_input(BenchmarkId::new(name, mode), &corpus, |b, corpus| {
                b.iter_batched(
                    || {
                        let mut terminal = TerminalScreen::new(60, 200, 5000);
                        if mode == "disabled_retained" {
                            terminal.set_program_status_enabled(true);
                            terminal.process_bytes(b"\x1b]7501;state=working\x07");
                            terminal.drain_program_status_events();
                        }
                        terminal.set_program_status_enabled(mode == "enabled");
                        terminal
                    },
                    |mut terminal| {
                        terminal.process_bytes(black_box(corpus));
                        terminal.drain_program_status_events();
                        black_box(terminal);
                    },
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(benches, program_status_ingest);
criterion_main!(benches);
