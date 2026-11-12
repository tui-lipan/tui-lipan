//! Runs the same PTY output through every implemented engine and compares what a screen reports.
//!
//! Alacritty is the reference. An engine joins [`ENGINES`] once it can be constructed; until then
//! the harness only checks that the reference agrees with itself and that the comparison is not
//! vacuous.

use std::ops::ControlFlow;

use super::super::{SemanticMark, TerminalScreen};
use super::TerminalEngineKind;
use crate::style::Span;

/// Engines compared against Alacritty.
const ENGINES: &[TerminalEngineKind] = &[
    TerminalEngineKind::Alacritty,
    #[cfg(feature = "terminal-ghostty")]
    TerminalEngineKind::Ghostty,
];

/// Everything a screen reports that an engine decides.
#[derive(Debug, PartialEq)]
struct Observation {
    text: Vec<String>,
    logical: Vec<String>,
    spans: Vec<Vec<Span>>,
    wrapped_rows: Vec<bool>,
    hyperlinks: Vec<(usize, std::ops::Range<usize>, String)>,
    cursor: (u16, u16, bool),
    cursor_style: (crate::style::CaretShape, bool),
    history: usize,
    alternate: bool,
    key_modes: super::super::super::TerminalKeyModes,
    mouse_mode: super::super::super::MouseModeState,
    title: Option<String>,
    bells: u64,
    responses: Vec<Vec<u8>>,
    clipboard: Vec<super::super::TerminalClipboardEvent>,
    semantic: super::super::super::TerminalSemanticState,
    marks: Vec<SemanticMark>,
    last_output: Option<String>,
}

fn observe(kind: TerminalEngineKind, chunks: &[&[u8]], size: (u16, u16, usize)) -> Observation {
    let (rows, cols, scrollback) = size;
    let mut screen = TerminalScreen::with_engine(kind, rows, cols, scrollback);
    let mut responses = Vec::new();
    for chunk in chunks {
        screen.process_bytes(chunk);
        responses.extend(screen.drain_responses());
    }
    let snapshot = screen.render_snapshot();
    let mut logical = Vec::new();
    let _ = screen.try_for_each_logical_text_line(0, usize::MAX, |line| {
        logical.push(line.text().to_owned());
        ControlFlow::Continue(())
    });
    Observation {
        text: screen.text_lines(0, screen.total_text_lines()),
        logical,
        spans: snapshot.color_lines.to_vec(),
        wrapped_rows: snapshot.wrapped_rows.to_vec(),
        hyperlinks: snapshot
            .hyperlinks
            .iter()
            .map(|link| {
                (
                    link.row,
                    link.start_col..link.end_col,
                    link.uri.as_ref().to_owned(),
                )
            })
            .collect(),
        cursor: (
            snapshot.cursor_row,
            snapshot.cursor_col,
            snapshot.cursor_visible,
        ),
        cursor_style: (snapshot.cursor_shape, snapshot.cursor_blinking),
        history: snapshot.total_scrollback_rows,
        alternate: screen.is_alternate_screen(),
        key_modes: screen.key_modes(),
        mouse_mode: screen.mouse_mode(),
        title: screen.title(),
        bells: screen.bell_count(),
        responses,
        clipboard: screen.drain_clipboard_events(),
        semantic: screen.semantic_state(),
        marks: screen.semantic_marks(),
        last_output: screen.export_last_command_output(),
    }
}

/// PTY output covering what the screen asks of an engine.
const CORPUS: &[(&str, &[&[u8]])] = &[
    (
        "styles",
        &[
            b"\x1b[1;31mbold red\x1b[0m \x1b[38;5;1midx\x1b[0m \x1b[4:3;58;2;1;2;3mcurl\x1b[0m\r\n\
            \x1b[2;3;7;9mdim it rev strike\x1b[0m \x1b[8mhidden\x1b[0m\r\n",
        ],
    ),
    (
        "wide-and-combining",
        &["東京 e\u{301} 🦀 wrap-around-the-edge-of-a-narrow-screen\r\n".as_bytes()],
    ),
    (
        "hyperlinks",
        &[b"\x1b]8;id=a;https://example.com\x1b\\link\x1b]8;;\x1b\\ text\r\n"],
    ),
    (
        "shell-integration",
        &[
            b"\x1b]7;file://host/tmp/work\x1b\\\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\ls\r\n",
            b"\x1b]133;C\x1b\\one\r\ntwo\r\n\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ",
        ],
    ),
    (
        "synchronized-update",
        &[
            b"\x1b[?2026h\x1b]133;A\x1b\\$ ls\r\n\x1b]133;C\x1b\\",
            b"one\r\n\x1b[?2026l\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ",
        ],
    ),
    (
        "alternate-screen",
        &[
            b"\x1b]133;C\x1b\\files\r\n\x1b]133;D;0\x1b\\",
            b"\x1b[?1049h\x1b[2;3Hvim-ui\x1b[?1000h\x1b[?1006h\x1b[?2004h",
        ],
    ),
    (
        "modes-and-queries",
        &[b"\x1b[?1h\x1b[>1u\x1b[?1016h\x1b[5 q\x1b]0;title\x07\x07\x1b[c\x1b[6n\x1b[>q"],
    ),
    ("clipboard", &[b"\x1b]52;c;aGVsbG8=\x07"]),
    (
        "unsupported-in-band-size-reports",
        &[b"\x1b[?2048$p\x1b[?2048h\x1b[?2048$p\x1b[6n"],
    ),
    (
        "scrollback-eviction",
        &[b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9\r\n10\r\n11\r\n12\r\n"],
    ),
    (
        // What the shell integration sends after every command, on the primary screen.
        "leave-alternate-on-primary",
        &[
            b"$ ls\r\none\r\n",
            b"\x1b]133;D;0\x1b\\\x1b[?1049l\x1b[?1000l\x1b[?2004l\x1b[?1l\x1b>",
            b"\x1b]133;A\x1b\\$ ",
            b"\x1b[?1049l",
            b"pwd",
        ],
    ),
    (
        "hard-reset",
        &[b"\x1b]133;A\x1b\\$ old\r\n\x1bc\x1b]133;A\x1b\\$ new"],
    ),
];

const SIZE: (u16, u16, usize) = (4, 24, 5);

#[test]
fn every_engine_reports_what_alacritty_reports() {
    let mut failures = Vec::new();
    for (name, chunks) in CORPUS {
        let reference = observe(TerminalEngineKind::Alacritty, chunks, SIZE);
        for &kind in ENGINES {
            let actual = observe(kind, chunks, SIZE);
            if actual != reference {
                failures.push(format!(
                    "{kind:?} diverges from Alacritty on {name}:\n  alacritty: {reference:?}\n  {kind:?}: {actual:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Negative control: the observation must see a difference an engine could plausibly get wrong,
/// or the comparison above proves nothing.
#[test]
fn observations_tell_corpus_cases_apart() {
    let observations: Vec<_> = CORPUS
        .iter()
        .map(|(_, chunks)| observe(TerminalEngineKind::Alacritty, chunks, SIZE))
        .collect();
    for (index, observation) in observations.iter().enumerate() {
        for other in &observations[index + 1..] {
            assert_ne!(observation, other);
        }
    }
    let shifted = observe(
        TerminalEngineKind::Alacritty,
        &[b"\x1b]133;A\x1b\\$ old\r\n\x1bc\x1b]133;A\x1b\\\r\n$ new"],
        SIZE,
    );
    assert_ne!(shifted, observations[CORPUS.len() - 1]);
}

/// Scrollback at scale: the history exposed, the lines evicted and the semantic marks that
/// survive must match Alacritty exactly, though libghostty prunes whole pages.
#[test]
fn scrollback_accounting_matches_alacritty_at_scale() {
    let mut output = Vec::new();
    for line in 0..20_000 {
        if line % 100 == 0 {
            output.extend_from_slice(b"\x1b]133;A\x1b\\$ \x1b]133;C\x1b\\");
        }
        output.extend_from_slice(format!("line {line}\r\n").as_bytes());
        if line == 12_345 {
            output.extend_from_slice(b"\x1b[3J");
        }
    }
    for scrollback in [0, 3, 1000, 5000] {
        let observe = |kind: TerminalEngineKind| {
            let mut screen = TerminalScreen::with_engine(kind, 24, 80, scrollback);
            for chunk in output.chunks(65_536) {
                screen.process_bytes(chunk);
            }
            let total = screen.total_text_lines();
            (
                screen.total_scrollback_rows(),
                screen.scrollback_lineage(),
                screen.text_lines(0, 3),
                screen.text_lines(total.saturating_sub(3), total),
                screen.semantic_marks(),
            )
        };
        let reference = observe(TerminalEngineKind::Alacritty);
        assert!(reference.0 <= scrollback);
        for &kind in ENGINES {
            assert_eq!(observe(kind), reference, "{kind:?} scrollback {scrollback}");
        }
    }
}

/// The hidden history margin is sized from the page size at the screen's width; eviction counts
/// must stay exact at narrow and wide screens, for bursts of bare line feeds, and for one long
/// line that wraps across far more rows than a piece of a write may push.
#[test]
fn scrollback_accounting_matches_alacritty_across_widths_and_bursts() {
    let long_line = "x".repeat(200_000);
    let bursts: [(&str, Vec<u8>); 3] = [
        (
            "numbered lines",
            (0..12_000)
                .flat_map(|line| format!("\x1b]133;A\x1b\\{line}\r\n").into_bytes())
                .collect(),
        ),
        (
            "bare line feeds",
            [b"mark".as_slice(), &[b'\n'; 30_000], b"end"].concat(),
        ),
        (
            "one long line",
            format!("start\r\n{long_line}\r\nend").into_bytes(),
        ),
    ];
    for cols in [20, 80, 200] {
        for (name, bytes) in &bursts {
            for scrollback in [0, 50, 3000] {
                let observe = |kind: TerminalEngineKind| {
                    let mut screen = TerminalScreen::with_engine(kind, 24, cols, scrollback);
                    for chunk in bytes.chunks(100_000) {
                        screen.process_bytes(chunk);
                    }
                    let total = screen.total_text_lines();
                    (
                        screen.total_scrollback_rows(),
                        screen.scrollback_lineage(),
                        screen.text_lines(0, 2),
                        screen.text_lines(total.saturating_sub(2), total),
                        screen.semantic_marks().len(),
                    )
                };
                let reference = observe(TerminalEngineKind::Alacritty);
                for &kind in ENGINES {
                    assert_eq!(
                        observe(kind),
                        reference,
                        "{kind:?} {name}, {cols} columns, scrollback {scrollback}"
                    );
                }
            }
        }
    }
}

/// Compressing idle history changes nothing a screen reports, and stops once done.
#[test]
fn idle_compression_keeps_contents() {
    for &kind in ENGINES {
        let mut screen = TerminalScreen::with_engine(kind, 24, 80, 5000);
        for line in 0..6000 {
            screen.process_bytes(format!("\x1b[3{}mline {line}\x1b[0m\r\n", line % 8).as_bytes());
        }
        let before = (
            screen.text_lines(0, screen.total_text_lines()),
            screen.render_snapshot().color_lines,
        );
        let mut steps = 0;
        while screen.compress_idle() {
            steps += 1;
            assert!(steps < 100_000, "{kind:?} compression never finishes");
        }
        assert!(
            !screen.compress_idle(),
            "{kind:?} has nothing left until new output"
        );
        screen.set_scrollback(1);
        screen.set_scrollback(0);
        let after = (
            screen.text_lines(0, screen.total_text_lines()),
            screen.render_snapshot().color_lines,
        );
        assert_eq!(after, before, "{kind:?}");
    }
}

/// Shrinking a full-screen application keeps the cursor's row and removes only the
/// necessary rows above it. Follow-up output also checks wrap, origin and saved state.
#[test]
fn alternate_resize_matches_alacritty() {
    let cases: &[(&str, &[u8], &[u8])] = &[
        ("top", b"\x1b[1;1H", b"X"),
        ("middle", b"\x1b[3;3H", b"X"),
        ("below cut", b"\x1b[5;3H", b"X"),
        ("bottom", b"\x1b[6;3H", b"X"),
        (
            "origin and margins",
            b"\x1b[2;5r\x1b[?6h\x1b[2;3H",
            b"X\x1b[1;1HY",
        ),
        ("pending wrap", b"\x1b[2;10HZ", b"X"),
        ("bottom pending wrap", b"\x1b[6;10HZ", b"X"),
        (
            "saved pending wrap",
            b"\x1b[5;10HZ\x1b7\x1b[1;1H",
            b"\x1b8X",
        ),
        (
            "saved style",
            b"\x1b[32m\x1b[2;4H\x1b7\x1b[31m\x1b[1;1H",
            b"\x1b8X",
        ),
        (
            "saved cursor above cut",
            b"\x1b[2;4H\x1b7\x1b[1;1H",
            b"\x1b8X",
        ),
        (
            "saved cursor below cut",
            b"\x1b[6;4H\x1b7\x1b[1;1H",
            b"\x1b8X",
        ),
        (
            "saved cursor with scrolling",
            b"\x1b[2;4H\x1b7\x1b[6;1H",
            b"\x1b8X",
        ),
        ("partial CSI", b"\x1b[1;1H\x1b[", b"2;2HX"),
        ("partial OSC", b"\x1b[1;1H\x1b]0;resize", b" title\x07X"),
        ("partial UTF-8", b"\x1b[1;1H\xe6", b"\x9d\xb1"),
    ];
    for (name, setup, after) in cases {
        let observe = |kind| {
            let mut screen = TerminalScreen::with_engine(kind, 6, 10, 20);
            screen.process_bytes(b"primary\x1b[?2048h\x1b[?1049h");
            for row in 0..6 {
                screen.process_bytes(format!("\x1b[{};1Hrow{row}", row + 1).as_bytes());
            }
            screen.process_bytes(setup);
            screen.resize(3, 10);
            let shrunk = screen.render_snapshot();
            screen.process_bytes(after);
            let written = screen.render_snapshot();
            screen.resize(6, 10);
            let grown = screen.render_snapshot();
            screen.process_bytes(b"\x1b[?1049l");
            let primary = screen.render_snapshot();
            assert!(
                screen.drain_responses().is_empty(),
                "{kind:?}: resize reports stay disabled"
            );
            [shrunk, written, grown, primary].map(|snapshot| {
                (
                    snapshot.color_lines,
                    snapshot.wrapped_rows,
                    snapshot.cursor_row,
                    snapshot.cursor_col,
                    snapshot.total_scrollback_rows,
                )
            })
        };
        let reference = observe(TerminalEngineKind::Alacritty);
        for &kind in ENGINES {
            assert_eq!(observe(kind), reference, "{kind:?}: {name}");
        }
    }
}

/// Cropping can retire whole native pages, including pages referenced by saved state.
#[test]
fn alternate_resize_across_pages_matches_alacritty() {
    for cursor in [0, 125, 250, 499] {
        let observe = |kind| {
            let mut screen = TerminalScreen::with_engine(kind, 500, 200, 0);
            screen.process_bytes(b"\x1b[?1049h");
            for row in 0..500 {
                screen.process_bytes(format!("\x1b[{};1Hrow{row}", row + 1).as_bytes());
            }
            screen.process_bytes(format!("\x1b[490;4H\x1b7\x1b[{};3H", cursor + 1).as_bytes());
            screen.resize(100, 200);
            let shrunk = screen.text_lines(0, screen.total_text_lines());
            screen.process_bytes(b"X\x1b8Y");
            screen.resize(500, 200);
            let grown = screen.text_lines(0, screen.total_text_lines());
            (shrunk, grown)
        };
        let reference = observe(TerminalEngineKind::Alacritty);
        for &kind in ENGINES {
            assert_eq!(observe(kind), reference, "{kind:?}: cursor row {cursor}");
        }
    }
}
