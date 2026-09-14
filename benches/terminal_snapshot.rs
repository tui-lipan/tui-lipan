use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use tui_lipan::prelude::TerminalScreen;

const HISTORY: usize = 5_000;
const COLS: u16 = 253;
const ROWS: u16 = 64;

fn corpus(kind: &str, lines: usize) -> Vec<u8> {
    let line = match kind {
        "tails" => "short log record\r\n".to_owned(),
        "shell" => "東京 e\u{301} 🦀 log record\r\n".to_owned(),
        "alias_tail" => {
            let mut text = String::from("\x1b[31ma\x1b[38;5;1m");
            text.extend((1..COLS).map(|col| char::from(b'a' + (col % 26) as u8)));
            text.push_str("\r\n");
            text
        }
        "alternating" => {
            let mut text = String::new();
            for col in 0..COLS {
                if col % 2 == 0 {
                    text.push_str("\x1b[31m");
                } else {
                    text.push_str("\x1b[32m");
                }
                text.push(char::from(b'a' + (col % 26) as u8));
            }
            text.push_str("\r\n");
            text
        }
        _ => unreachable!(),
    };
    line.repeat(lines).into_bytes()
}

fn populated(kind: &str) -> TerminalScreen {
    let mut screen = TerminalScreen::new(ROWS, COLS, HISTORY);
    screen.process_bytes(&corpus(kind, HISTORY + usize::from(ROWS) + 1));
    screen
}

fn snapshots(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_snapshot");
    for kind in ["tails", "shell", "alias_tail", "alternating"] {
        group.bench_function(BenchmarkId::from_parameter(kind), |b| {
            b.iter_batched_ref(
                || populated(kind),
                |screen| black_box(screen.render_snapshot()),
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, snapshots);
criterion_main!(benches);
