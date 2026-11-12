//! Cell-level comparison of the Ghostty reader with the Alacritty engine on the byte-stream
//! corpus: the same bytes into both, then every retained row compared field by field.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::super::super::{CellAttrs, CellColor, CellRead, LinkRead, RowRead, TerminalEngine};
use super::*;
use crate::widgets::terminal::ghostty::ScrollbackLimits;
use crate::widgets::terminal::screen::{TerminalEngineKind, TerminalScreen};

#[derive(Deserialize)]
struct Fixture {
    rows: u16,
    cols: u16,
    scrollback: usize,
    #[serde(default)]
    ops: Vec<Operation>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Operation {
    Write {
        bytes: [usize; 2],
    },
    Resize {
        rows: u16,
        cols: u16,
    },
    // Parsed so fixtures load; this differential test compares the live rows only.
    Scroll {
        #[allow(dead_code)]
        delta: i64,
    },
    Dump,
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/terminal-corpus");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
}

/// One cell's fields as both engines report them.
#[derive(Debug, PartialEq)]
struct CellView {
    ch: char,
    zerowidth: Vec<char>,
    fg: CellColor,
    bg: CellColor,
    underline_color: Option<CellColor>,
    attrs: CellAttrs,
    link: Option<String>,
    empty: bool,
}

fn view<C: CellRead>(cell: &C) -> CellView {
    CellView {
        ch: cell.ch(),
        zerowidth: cell.zerowidth().to_vec(),
        fg: cell.fg(),
        bg: cell.bg(),
        underline_color: cell.underline_color(),
        attrs: cell.attrs(),
        link: cell.hyperlink().map(|link| link.uri().to_owned()),
        empty: cell.is_empty(),
    }
}

/// Differences by class, with one example each.
#[derive(Default)]
struct Differences {
    classes: BTreeMap<String, (usize, String)>,
    /// Cells where Alacritty holds `Indexed(n < 16)` in a color Ghostty reads as named.
    low_indexed_cells: usize,
    cells: usize,
}

impl Differences {
    fn add(&mut self, class: &str, example: impl FnOnce() -> String) {
        let entry = self
            .classes
            .entry(class.to_owned())
            .or_insert_with(|| (0, example()));
        entry.0 += 1;
    }
}

fn low_indexed(color: CellColor) -> bool {
    matches!(color, CellColor::Indexed(n) if n < 16)
}

fn compare(path: &Path, differences: &mut Differences) {
    let fixture: Fixture = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let bytes = fs::read(path.with_extension("vt")).unwrap();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    compare_stream(&name, &fixture, &bytes, differences);
}

/// Feeds a stream into both engines and compares the rows they retain, aligned at the bottom:
/// Ghostty's history is page-granular and may hold more.
fn compare_stream(name: &str, fixture: &Fixture, bytes: &[u8], differences: &mut Differences) {
    let mut alacritty = TerminalScreen::with_engine(
        TerminalEngineKind::Alacritty,
        fixture.rows,
        fixture.cols,
        fixture.scrollback,
    );
    let mut ghostty = Terminal::new(fixture.cols, fixture.rows).unwrap();
    ghostty
        .set_scrollback_limits(ScrollbackLimits {
            max_lines: Some(fixture.scrollback),
            max_bytes: None,
        })
        .unwrap();
    let ops = if fixture.ops.is_empty() {
        vec![Operation::Write {
            bytes: [0, bytes.len()],
        }]
    } else {
        fixture.ops.clone()
    };
    for op in ops {
        match op {
            Operation::Write {
                bytes: [start, end],
            } => {
                alacritty.process_bytes(&bytes[start..end]);
                ghostty.write(&bytes[start..end]);
            }
            Operation::Resize { rows, cols } => {
                alacritty.resize(rows, cols);
                ghostty.resize(cols, rows, 8, 16).unwrap();
            }
            // Rows are read by absolute line; the viewport does not matter.
            Operation::Scroll { .. } | Operation::Dump => {}
        }
    }

    let engine = alacritty.engine.alacritty();
    let alacritty_lines = engine.history_size() + engine.screen_lines();
    let ghostty_lines = ghostty.total_rows();
    let lines = alacritty_lines.min(ghostty_lines);
    let mut scratch = ReadScratch::default();
    for back in 1..=lines {
        let a_row = engine.row(alacritty_lines - back);
        let g_row = row(
            &ghostty,
            (ghostty_lines - back) as u32,
            ghostty.cols(),
            &mut scratch,
        );
        let at = |x: usize| format!("{name} line -{back} col {x}");
        if a_row.wrapped() != g_row.wrapped() {
            differences.add("wrapped", || {
                format!("{}: {} vs {}", at(0), a_row.wrapped(), g_row.wrapped())
            });
        }
        for (x, (a, g)) in a_row.cells().iter().zip(g_row.cells()).enumerate() {
            differences.cells += 1;
            let (a, g) = (view(a), view(g));
            if low_indexed(a.fg) || low_indexed(a.bg) {
                differences.low_indexed_cells += 1;
            }
            if a == g {
                continue;
            }
            let detail = || format!("{}: {a:?}\n      vs {g:?}", at(x));
            if a.ch != g.ch {
                let class = if a.ch == '\t' && g.ch == ' ' {
                    "ch: tab stored in cell (Alacritty)"
                } else {
                    "ch"
                };
                differences.add(class, detail);
            }
            if a.zerowidth != g.zerowidth {
                differences.add("zerowidth", detail);
            }
            for (field, a_color, g_color) in [("fg", a.fg, g.fg), ("bg", a.bg, g.bg)] {
                if a_color != g_color {
                    let class = if low_indexed(a_color) {
                        format!("{field}: Indexed(<16) read as Named")
                    } else {
                        field.to_owned()
                    };
                    differences.add(&class, detail);
                }
            }
            if a.underline_color != g.underline_color {
                differences.add("underline_color", detail);
            }
            if a.attrs != g.attrs {
                differences.add(
                    &format!("attrs {:#06x} vs {:#06x}", a.attrs.bits(), g.attrs.bits()),
                    detail,
                );
            }
            if a.link != g.link {
                differences.add("link", detail);
            }
            if a.empty != g.empty {
                differences.add("is_empty", detail);
            }
        }
    }
}

/// A stream with what the corpus lacks: deep history, every underline style and color form,
/// background-only erases, wide characters wrapping at the edge, grapheme clusters, hidden and
/// decorated text, adjacent links with distinct ids, and `SGR 38;5;1`.
fn synthetic() -> Vec<u8> {
    let mut out = String::new();
    for i in 0..1500 {
        match i % 10 {
            0 => out.push_str(&format!("\x1b[38;5;{}m256 fg {i}\x1b[48;2;{};40;90m tc bg\x1b[0m", 16 + i % 240, i % 256)),
            1 => out.push_str(&format!("\x1b[4:{}m under {i}\x1b[58;5;{}m colored\x1b[58;2;1;2;3m rgb\x1b[0m", 1 + i % 5, i % 256)),
            2 => out.push_str(&format!("\x1b[4{}m\x1b[K erased bg {i}\x1b[0m", i % 8)),
            3 => out.push_str(&format!("{}日本語の行{i}", "x".repeat(70 + i % 8))),
            4 => out.push_str(&format!("e\u{301} n\u{303}\u{323} \u{1f469}\u{200d}\u{1f4bb} \u{2764}\u{fe0f} {i}")),
            5 => out.push_str(&format!("\x1b[8mhidden\x1b[0m \x1b[7minv\x1b[0m \x1b[2mdim\x1b[22m \x1b[9mstrike\x1b[0m \x1b[1;3mbi\x1b[0m {i}")),
            6 => out.push_str(&format!("\x1b]8;id=a;https://example.org/\x1b\\one\x1b]8;id=b;https://example.org/\x1b\\two\x1b]8;;\x1b\\ {i}")),
            7 => out.push_str(&format!("\x1b[38;5;1mindexed red\x1b[31m named red\x1b[0m {i}")),
            8 => out.push_str(&format!("abcdefghij\x1b[5D\x1b[2P\x1b[3@XYZ {i}")),
            _ => out.push_str(&format!("\x1b[{};{}r\x1b[S\x1b[r\x1b[24;1H plain {i}", 2 + i % 5, 10 + i % 7)),
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

#[test]
#[ignore = "report: prints every cell difference class on the corpus"]
fn read_corpus_report() {
    for path in fixtures() {
        let mut differences = Differences::default();
        compare(&path, &mut differences);
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if differences.classes.is_empty() && differences.low_indexed_cells == 0 {
            continue;
        }
        eprintln!(
            "== {name}: {} cells, {} low-indexed",
            differences.cells, differences.low_indexed_cells
        );
        for (class, (count, example)) in &differences.classes {
            eprintln!("  {class}: {count}\n    {example}");
        }
    }
    let mut differences = Differences::default();
    let fixture = Fixture {
        rows: 24,
        cols: 80,
        scrollback: 5000,
        ops: Vec::new(),
    };
    compare_stream("synthetic", &fixture, &synthetic(), &mut differences);
    eprintln!(
        "== synthetic: {} cells, {} low-indexed",
        differences.cells, differences.low_indexed_cells
    );
    for (class, (count, example)) in &differences.classes {
        eprintln!("  {class}: {count}\n    {example}");
    }
}

/// Reviewed emulator differences: `(fixture, class)`. Everything else must match cell for cell.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "osc",
        "*",
        "Ghostty starts a fresh line on OSC 133;A when the cursor is not at column 0",
    ),
    (
        "*",
        "ch: tab stored in cell (Alacritty)",
        "Alacritty stores a literal tab where one starts; Ghostty stores nothing",
    ),
    (
        "synthetic",
        "fg: Indexed(<16) read as Named",
        "Ghostty stores SGR 38;5;n (n < 16) like SGR 3x; the stream writes it on purpose",
    ),
];

fn unexpected(name: &str, differences: &Differences) -> Vec<String> {
    differences
        .classes
        .iter()
        .filter(|(class, _)| {
            !KNOWN.iter().any(|(fixture, known, _)| {
                (*fixture == "*" || *fixture == name) && (*known == "*" || known == class)
            })
        })
        .map(|(class, (count, example))| format!("{name}: {class} x{count}\n  {example}"))
        .collect()
}

#[test]
fn rows_match_alacritty_cell_for_cell() {
    let mut failures = Vec::new();
    for path in fixtures() {
        let mut differences = Differences::default();
        compare(&path, &mut differences);
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        failures.extend(unexpected(&name, &differences));
    }
    let mut differences = Differences::default();
    let fixture = Fixture {
        rows: 24,
        cols: 80,
        scrollback: 5000,
        ops: Vec::new(),
    };
    compare_stream("synthetic", &fixture, &synthetic(), &mut differences);
    // Deep history really was read, through screen coordinates.
    assert!(differences.cells > 1000 * 80, "{} cells", differences.cells);
    failures.extend(unexpected("synthetic", &differences));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
