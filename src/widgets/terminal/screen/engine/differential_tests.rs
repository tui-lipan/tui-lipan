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
    /// Evicted lines and history epoch: what absolute line numbers mean.
    lineage: (u64, u64),
    scroll_offset: usize,
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
    /// What [`Step::ReadPrimary`] steps returned: the last command's output and the replay export.
    reads: Vec<(Option<String>, Vec<u8>)>,
}

fn observe(kind: TerminalEngineKind, chunks: &[&[u8]], size: (u16, u16, usize)) -> Observation {
    let (rows, cols, scrollback) = size;
    let mut screen = TerminalScreen::with_engine(kind, rows, cols, scrollback);
    let mut responses = Vec::new();
    for chunk in chunks {
        screen.process_bytes(chunk);
        responses.extend(screen.drain_responses());
    }
    observe_screen(&mut screen, responses)
}

/// What `screen` reports now, with the replies collected while it was fed.
fn observe_screen(screen: &mut TerminalScreen, responses: Vec<Vec<u8>>) -> Observation {
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
        lineage: (snapshot.evicted_lines, snapshot.history_epoch),
        scroll_offset: snapshot.scrollback_offset,
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
        reads: Vec::new(),
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

/// Each engine must report the same state however `process_bytes` splits `stream`: at every
/// single position, and in the given multi-split patterns (piece lengths, repeated). Returns one
/// failure per engine at most.
fn split_failures(
    name: &str,
    stream: &[u8],
    size: (u16, u16, usize),
    patterns: &[&[usize]],
) -> Vec<String> {
    let mut failures = Vec::new();
    for &kind in ENGINES {
        let whole = observe(kind, &[stream], size);
        let mut splits: Vec<Vec<&[u8]>> = (1..stream.len())
            .map(|cut| vec![&stream[..cut], &stream[cut..]])
            .collect();
        for pattern in patterns {
            let mut pieces = Vec::new();
            let mut start = 0;
            for &len in pattern.iter().cycle() {
                if start >= stream.len() {
                    break;
                }
                let end = (start + len.max(1)).min(stream.len());
                pieces.push(&stream[start..end]);
                start = end;
            }
            splits.push(pieces);
        }
        if let Some(pieces) = splits
            .iter()
            .find(|pieces| observe(kind, pieces, size) != whole)
        {
            let split = observe(kind, pieces, size);
            let lengths: Vec<usize> = pieces.iter().map(|piece| piece.len()).collect();
            failures.push(format!(
                "{kind:?} {name}: pieces {lengths:?}\n  whole: {whole:?}\n  split: {split:?}"
            ));
        }
    }
    failures
}

/// Shells send `?1049l` after every command; on the primary screen it must stay a no-op however
/// the PTY read splits it (libghostty would restore a saved cursor, (0, 0) if none).
#[test]
fn leaving_the_alternate_screen_on_the_primary_is_split_invariant() {
    let stream =
        b"$ ls\r\none\r\n\x1b]133;D;0\x1b\\\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b]133;A\x1b\\$ ";
    let failures = split_failures("redundant ?1049l", stream, SIZE, &[&[1], &[3, 1, 2]]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ED 3` while the viewport is scrolled into history: the clear, the eviction count and where
/// the viewport lands must not depend on how the sequence is split.
#[test]
fn clearing_history_while_scrolled_is_split_invariant() {
    let mut history = b"\x1b]133;A\x1b\\$ a\r\n\x1b]133;C\x1b\\".to_vec();
    for line in 0..20 {
        history.extend_from_slice(format!("{line}\r\n").as_bytes());
    }
    let rest: &[u8] = b"\x1b]133;D;0\x1b\\x\x1b[3Jy\r\n\x1b]133;A\x1b\\$ b";
    let size = (4, 24, 5);
    let run = |kind, pieces: &[&[u8]]| {
        let mut screen = TerminalScreen::with_engine(kind, size.0, size.1, size.2);
        screen.process_bytes(&history);
        screen.set_scrollback(3);
        let mut responses = screen.drain_responses();
        for piece in pieces {
            screen.process_bytes(piece);
            responses.extend(screen.drain_responses());
        }
        observe_screen(&mut screen, responses)
    };
    let reference = run(TerminalEngineKind::Alacritty, &[rest]);
    let mut failures = Vec::new();
    for &kind in ENGINES {
        let whole = run(kind, &[rest]);
        if whole != reference {
            failures.push(format!(
                "{kind:?} whole differs from Alacritty:\n  {reference:?}\n  {whole:?}"
            ));
        }
        for cut in 1..rest.len() {
            let split = run(kind, &[&rest[..cut], &rest[cut..]]);
            if split != whole {
                failures.push(format!(
                    "{kind:?} split at {cut}:\n  whole: {whole:?}\n  split: {split:?}"
                ));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A host resize while a prompt is active must not disturb a sequence the child left unfinished:
/// the continuation completes it exactly as it would have without the resize.
#[test]
fn prompt_resize_keeps_unfinished_sequences() {
    let before: &[u8] = b"out\r\n\x1b]133;A\x1b\\$ make && run ";
    let cases: [(&str, &[u8], &[u8]); 7] = [
        ("nothing pending (control)", b"", b"X"),
        ("saved cursor", b"\x1b7", b"\x1b8X"),
        ("origin mode", b"\x1b[2;4r\x1b[?6h", b"X\x1b[?6l\x1b[r"),
        // `before` leaves the cursor at column 14; ten more cells fill the row to its last column.
        ("pending wrap", b"tencharsxx", b"Y"),
        ("CSI", b"\x1b[1;", b"31mX\x1b[0m"),
        ("OSC", b"\x1b]0;ti", b"tle\x07X"),
        ("UTF-8", b"\xe6\x9d", b"\xb1X"),
    ];
    let mut failures = Vec::new();
    for (name, head, tail) in cases {
        for (rows, cols) in [(6, 24), (4, 16), (5, 30)] {
            let run = |kind| {
                let mut screen = TerminalScreen::with_engine(kind, 4, 24, 5);
                screen.process_bytes(before);
                screen.process_bytes(head);
                let mut responses = screen.drain_responses();
                screen.resize(rows, cols);
                screen.process_bytes(tail);
                responses.extend(screen.drain_responses());
                observe_screen(&mut screen, responses)
            };
            let reference = run(TerminalEngineKind::Alacritty);
            for &kind in ENGINES {
                let actual = run(kind);
                if actual != reference {
                    failures.push(format!(
                        "{kind:?} {name} resized to {rows}x{cols}:\n  alacritty: {reference:?}\n  {kind:?}: {actual:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
/// Reading the primary screen under a running alternate-screen application (the last command's
/// output, a replay export) must not change what the application sees next: its saved cursor, a
/// pending wrap, and a sequence it left unfinished.
#[test]
fn primary_reads_leave_the_alternate_screen_application_untouched() {
    let before: &[u8] =
        b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\ls\r\n\x1b]133;C\x1b\\\x1b[32mout\x1b[0m\r\n\
        \x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ vim\r\n\
        \x1b[?1049h\x1b[2;3Hab\x1b7\x1b[1;21Hedge\x1b[3";
    let tail: &[u8] = b"1mZ\x1b[0m\x1b8Y";
    let replay_of = |kind| {
        let mut screen = TerminalScreen::with_engine(kind, 4, 24, 5);
        screen.process_bytes(before);
        let mut replay = Vec::new();
        screen.write_replay_bytes(&mut replay).unwrap();
        replay
    };
    // The replay export repaints the primary screen from the engine's reads with shared code, so
    // it is byte for byte the same on every engine when the reads are.
    let reference_replay = replay_of(TerminalEngineKind::Alacritty);
    for &kind in ENGINES {
        assert_eq!(
            String::from_utf8_lossy(&replay_of(kind)),
            String::from_utf8_lossy(&reference_replay),
            "{kind:?} replay export"
        );
    }
    let run = |kind, read: bool| {
        let mut screen = TerminalScreen::with_engine(kind, 4, 24, 5);
        screen.process_bytes(before);
        let mut responses = screen.drain_responses();
        if read {
            assert_eq!(screen.export_last_command_output().as_deref(), Some("out"));
            let mut replay = Vec::new();
            screen.write_replay_bytes(&mut replay).unwrap();
            assert!(!replay.is_empty());
        }
        screen.process_bytes(tail);
        responses.extend(screen.drain_responses());
        observe_screen(&mut screen, responses)
    };
    let reference = run(TerminalEngineKind::Alacritty, false);
    let mut failures = Vec::new();
    for &kind in ENGINES {
        for read in [false, true] {
            let actual = run(kind, read);
            if actual != reference {
                failures.push(format!(
                    "{kind:?} (read: {read}):\n  alacritty: {reference:?}\n  {kind:?}: {actual:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Past the continuation limit a snapshot cannot carry the unfinished sequence, so the Ghostty
/// engine reads an empty primary screen instead; the running application is still untouched.
#[cfg(feature = "terminal-ghostty")]
#[test]
fn primary_reads_past_the_continuation_limit_are_empty_and_harmless() {
    let mut screen = TerminalScreen::with_engine(TerminalEngineKind::Ghostty, 4, 24, 5);
    screen.process_bytes(b"\x1b]133;A\x1b\\$ \x1b]133;C\x1b\\out\r\n\x1b]133;D;0\x1b\\");
    screen.process_bytes(b"\x1b[?1049h\x1b[2;3Hab\x1b]2;");
    let title = vec![b'x'; 5 << 20];
    screen.process_bytes(&title);
    // The marks are still there; the stand-in primary screen they point into is blank.
    assert_eq!(screen.export_last_command_output().as_deref(), Some(""));
    screen.process_bytes(b"\x07Z");
    assert_eq!(screen.render_snapshot().cursor_col, 5);
}

/// One step of a chunk-invariance scenario.
#[derive(Clone, Copy)]
enum Step<'a> {
    /// Child output, split as the run asks.
    Write(&'a [u8]),
    /// A host resize to (rows, cols).
    Resize(u16, u16),
    /// The host reads the primary screen: the last command's output and a replay export.
    ReadPrimary,
}

/// A named chunk-invariance scenario: screen size (rows, cols, scrollback) and its steps.
type Scenario<'a> = (&'static str, (u16, u16, usize), Vec<Step<'a>>);

/// How a run splits its writes.
#[derive(Clone, Copy, Debug)]
enum Split<'a> {
    Whole,
    /// Write step `step` in two at byte `at`; the others whole.
    At {
        step: usize,
        at: usize,
    },
    /// Every write in pieces of these lengths, repeated.
    Pattern(&'a [usize]),
}

fn run_steps(
    kind: TerminalEngineKind,
    size: (u16, u16, usize),
    steps: &[Step<'_>],
    split: Split<'_>,
) -> Observation {
    let mut screen = TerminalScreen::with_engine(kind, size.0, size.1, size.2);
    let mut responses = Vec::new();
    let mut reads = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        match *step {
            Step::Write(bytes) => {
                let pieces: Vec<&[u8]> = match split {
                    Split::At { step, at } if step == index => vec![&bytes[..at], &bytes[at..]],
                    Split::Pattern(lengths) => {
                        let mut pieces = Vec::new();
                        let mut start = 0;
                        for &len in lengths.iter().cycle() {
                            if start >= bytes.len() {
                                break;
                            }
                            let end = (start + len.max(1)).min(bytes.len());
                            pieces.push(&bytes[start..end]);
                            start = end;
                        }
                        pieces
                    }
                    _ => vec![bytes],
                };
                for piece in pieces {
                    screen.process_bytes(piece);
                }
            }
            Step::Resize(rows, cols) => screen.resize(rows, cols),
            Step::ReadPrimary => {
                let mut replay = Vec::new();
                screen.write_replay_bytes(&mut replay).unwrap();
                reads.push((screen.export_last_command_output(), replay));
            }
        }
        responses.extend(screen.drain_responses());
    }
    let mut observation = observe_screen(&mut screen, responses);
    observation.reads = reads;
    observation
}

/// The state a screen reaches must depend only on the bytes and the host's operations, never on
/// how `process_bytes` happened to split the bytes, and never on whether the host read the
/// screen. Every write of every scenario is split at every byte, and all writes together in a
/// few multi-split patterns. Each engine must match its own unsplit run, and every engine must
/// match Alacritty.
#[test]
fn every_engine_is_chunk_invariant() {
    let mut ed3 = b"\x1b]133;A\x1b\\$ a\r\n\x1b]133;C\x1b\\".to_vec();
    for line in 0..12 {
        ed3.extend_from_slice(format!("{line}\r\n").as_bytes());
    }
    ed3.extend_from_slice(b"\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ b\r\n\x1b[3J\x1b]133;C\x1b\\8\r\n");
    let graphemes =
        "e\u{301} 1\u{fe0f}\u{20e3} \u{1100}\u{1161}\u{11a8} 👨\u{200d}👩\u{200d}👧 \u{2764}\u{fe0f}x\r\n"
            .repeat(2);
    let scenarios: [Scenario<'_>; 5] = [
        (
            "redundant ?1049l on the primary screen",
            (4, 24, 5),
            vec![Step::Write(
                b"$ ls\r\none\r\n\x1b]133;D;0\x1b\\\x1b[?1049l\x1b[?1047l\x1b]133;A\x1b\\$ ",
            )],
        ),
        ("ED 3 with history and marks", (4, 24, 5), vec![Step::Write(&ed3)]),
        ("graphemes", (4, 40, 5), vec![Step::Write(graphemes.as_bytes())]),
        (
            "prompt resizes with unfinished CSI, OSC and UTF-8",
            (4, 24, 5),
            vec![
                Step::Write(b"out\r\n\x1b]133;A\x1b\\$ cmd \x1b[1;"),
                Step::Resize(6, 16),
                Step::Write(b"31mX\x1b[0m\x1b]0;ti"),
                Step::Resize(4, 24),
                Step::Write(b"tle\x07\xe6\x9d"),
                Step::Resize(5, 20),
                Step::Write(b"\xb1Y"),
            ],
        ),
        (
            "alternate-screen application read and continued",
            (4, 24, 5),
            vec![
                Step::Write(
                    b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\ls\r\n\x1b]133;C\x1b\\\x1b[32mout\x1b[0m\r\n\
                    \x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ vim\r\n\
                    \x1b[?1049h\x1b[2;3Hab\x1b7\x1b[1;21Hedge\x1b[3",
                ),
                Step::ReadPrimary,
                Step::Write(b"1mZ\x1b[0m\x1b8Y\x1b[4;23Hzz"),
                Step::ReadPrimary,
                Step::Write(b"w\x1b[?1049l"),
                Step::ReadPrimary,
            ],
        ),
    ];
    let patterns: [&[usize]; 3] = [&[1], &[2, 3, 1], &[7, 1, 4]];
    let mut failures = Vec::new();
    for (name, size, steps) in &scenarios {
        let reference = run_steps(TerminalEngineKind::Alacritty, *size, steps, Split::Whole);
        for &kind in ENGINES {
            let whole = run_steps(kind, *size, steps, Split::Whole);
            if whole != reference {
                failures.push(format!(
                    "{kind:?} differs from Alacritty on {name}:\n  alacritty: {reference:?}\n  {kind:?}: {whole:?}"
                ));
            }
            let mut splits: Vec<Split<'_>> = patterns.iter().map(|p| Split::Pattern(p)).collect();
            for (step, write) in steps.iter().enumerate() {
                if let Step::Write(bytes) = write {
                    splits.extend((1..bytes.len()).map(|at| Split::At { step, at }));
                }
            }
            if let Some((split, observed)) = splits
                .into_iter()
                .map(|split| (split, run_steps(kind, *size, steps, split)))
                .find(|(_, observed)| *observed != whole)
            {
                failures.push(format!(
                    "{kind:?} on {name} depends on the split {split:?}:\n  whole: {whole:?}\n  split: {observed:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
