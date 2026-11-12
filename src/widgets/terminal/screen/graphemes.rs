//! Mode 2027 (grapheme clustering) checks through the screen's public read path.

use super::*;

const CASES: &[(&str, &str)] = &[
    ("keycap-1", "1\u{fe0f}\u{20e3}"),
    ("keycap-hash", "#\u{fe0f}\u{20e3}"),
    ("vs16", "\u{2764}\u{fe0f}"),
    ("vs15", "\u{2764}\u{fe0e}"),
    ("flag", "🇯🇵"),
    ("lone-ri", "🇯"),
    ("family", "👨\u{200d}👩\u{200d}👧"),
    ("skin", "👍🏽"),
    ("accent", "e\u{301}"),
    ("hangul", "\u{1100}\u{1161}\u{11a8}"),
    ("cjk", "字"),
];

fn cells(screen: &mut TerminalScreen) -> Vec<(usize, usize, String, usize)> {
    let mut result = Vec::new();
    let _ = screen.try_for_each_logical_text_line(0, screen.total_text_lines(), |line| {
        for cell in line.cells {
            let start = line.start_position(cell.byte_start).unwrap();
            result.push((
                start.absolute_line,
                cell.column_start,
                line.text()[cell.byte_start..cell.byte_end].to_owned(),
                cell.column_end - cell.column_start,
            ));
        }
        ControlFlow::Continue(())
    });
    result
}

#[test]
fn grapheme_measurements() {
    let wrap_cursors = [
        [
            (0, 4),
            (0, 4),
            (0, 4),
            (0, 4),
            (1, 1),
            (0, 4),
            (2, 2),
            (1, 4),
            (0, 4),
            (1, 2),
            (1, 2),
        ],
        [
            (0, 4),
            (0, 4),
            (0, 4),
            (0, 4),
            (1, 4),
            (1, 2),
            (2, 2),
            (1, 4),
            (0, 4),
            (1, 2),
            (1, 2),
        ],
        [
            (1, 2),
            (1, 2),
            (1, 2),
            (0, 4),
            (1, 2),
            (1, 2),
            (1, 2),
            (1, 2),
            (0, 4),
            (1, 2),
            (1, 2),
        ],
    ];
    for (mode, (kind, enabled)) in [
        (TerminalEngineKind::Alacritty, false),
        (TerminalEngineKind::Ghostty, false),
        (TerminalEngineKind::Ghostty, true),
    ]
    .into_iter()
    .enumerate()
    {
        for (case, &wrap_cursor) in wrap_cursors[mode].iter().enumerate() {
            for cols in [40, 5] {
                check_case(kind, enabled, case, cols, wrap_cursor);
            }
        }
    }
}

#[test]
fn grapheme_default_survives_resets_and_child_can_toggle_it() {
    let mut screen = TerminalScreen::with_engine(TerminalEngineKind::Ghostty, 4, 40, 0);
    screen.process_bytes(b"\x1b[?2027$p");
    assert_eq!(screen.drain_responses(), [b"\x1b[?2027;2$y".to_vec()]);
    let mut screen = screen.with_grapheme_clustering(true);
    screen.process_bytes(b"\x1b[?2027l\x1b[?2027$p");
    assert_eq!(screen.drain_responses(), [b"\x1b[?2027;2$y".to_vec()]);
    // RIS and following text in one write must use the configured default.
    screen.process_bytes("\x1bc👨\u{200d}👩\u{200d}👧".as_bytes());
    assert_eq!(screen.render_snapshot().cursor_col, 2);
    screen.reset();
    screen.process_bytes("1\u{fe0f}\u{20e3}".as_bytes());
    assert_eq!(cells(&mut screen), [(0, 0, "1\u{fe0f}\u{20e3}".into(), 2)]);
    screen.process_bytes(b"\x1b[?2027l\x1bc\x1b[?2027$p");
    assert_eq!(screen.drain_responses(), [b"\x1b[?2027;1$y".to_vec()]);
    let mut disabled = screen.with_grapheme_clustering(false);
    disabled.process_bytes(b"\x1b[?2027h\x1bc\x1b[?2027$p");
    assert_eq!(disabled.drain_responses(), [b"\x1b[?2027;2$y".to_vec()]);
}

#[test]
fn unicode_width_tui_placement() {
    use unicode_width::UnicodeWidthStr;
    for enabled in [false, true] {
        for &(name, text) in CASES {
            let width = text.width();
            let mut screen = TerminalScreen::with_engine(TerminalEngineKind::Ghostty, 4, 40, 0)
                .with_grapheme_clustering(enabled);
            // A TUI writes a label then positions its next column using unicode-width.
            screen.process_bytes(text.as_bytes());
            let printed = screen.render_snapshot().cursor_col;
            screen.process_bytes(format!("\x1b[{}GX", width + 1).as_bytes());
            let frame = screen.capture_frame();
            println!(
                "TUI {enabled} {name} unicode-width={width} printed={printed} cells={:?}",
                cells(&mut screen)
            );
            assert_eq!(frame.cell(width as u16, 0).symbol, "X");
            if enabled {
                // unicode-width 0.2 treats a lone RI as one column; Ghostty uses two
                // even with clustering on. Positioning X at column one erases that RI.
                assert_eq!(
                    usize::from(printed),
                    width + usize::from(name == "lone-ri"),
                    "{name}"
                );
            }
        }
    }
}

fn expected_cells(
    kind: TerminalEngineKind,
    enabled: bool,
    case: usize,
) -> Vec<(&'static str, usize)> {
    let text = CASES[case].1;
    let legacy: &[&[(&str, usize)]] = &[
        &[("1\u{20e3}", 1)],
        &[("#\u{20e3}", 1)],
        &[("❤\u{fe0f}", 1)],
        &[("❤\u{fe0e}", 1)],
        &[("🇯", 2), ("🇵", 2)],
        &[("🇯", 2)],
        &[("👨\u{200d}", 2), ("👩\u{200d}", 2), ("👧", 2)],
        &[("👍", 2), ("🏽", 2)],
        &[("e\u{301}", 1)],
        &[("각", 2)],
        &[("字", 2)],
    ];
    match (kind, enabled, case) {
        (_, true, 3 | 8) => vec![(text, 1)],
        (_, true, _) => vec![(text, 2)],
        (TerminalEngineKind::Alacritty, _, 0..=1) => vec![(text, 1)],
        (TerminalEngineKind::Alacritty, _, 4) => vec![("🇯", 1), ("🇵", 1)],
        (TerminalEngineKind::Alacritty, _, 5) => vec![(text, 1)],
        _ => legacy[case].to_vec(),
    }
}

fn check_case(
    kind: TerminalEngineKind,
    enabled: bool,
    case: usize,
    cols: u16,
    wrap_cursor: (u16, u16),
) {
    let (name, text) = CASES[case];
    let input = format!("{}{text}", if cols == 5 { "abcd" } else { "" });
    let mut screen =
        TerminalScreen::with_engine(kind, 5, cols, 0).with_grapheme_clustering(enabled);
    screen.process_bytes(input.as_bytes());
    let expected = expected_cells(kind, enabled, case);
    let snapshot = screen.render_snapshot();
    println!(
        "{kind:?}/{enabled} {name} cols={cols} cursor={},{} cells={:?}",
        snapshot.cursor_row,
        snapshot.cursor_col,
        cells(&mut screen)
    );

    if cols == 40 {
        let actual = cells(&mut screen);
        assert_eq!(
            actual
                .iter()
                .map(|(_, _, text, width)| (text.as_str(), *width))
                .collect::<Vec<_>>(),
            expected,
            "{kind:?}/{enabled} {name}"
        );
        assert_eq!(
            snapshot.cursor_col as usize,
            expected.iter().map(|(_, width)| width).sum::<usize>()
        );
    } else {
        assert_eq!(
            (snapshot.cursor_row, snapshot.cursor_col),
            wrap_cursor,
            "{kind:?}/{enabled} {name}"
        );
    }
    // Feed every byte boundary, including inside UTF-8 and VS/ZWJ sequences.
    // Pin the legacy dirty-row defect separately from the correct whole-write result.
    let mut split = TerminalScreen::with_engine(kind, 5, cols, 0).with_grapheme_clustering(enabled);
    for byte in input.as_bytes() {
        split.process_bytes(std::slice::from_ref(byte));
    }
    assert_eq!(
        cells(&mut split),
        expected_split_cells(&mut screen, kind, enabled, case),
        "{kind:?}/{enabled} {name} cols={cols}"
    );
    let split_snapshot = split.render_snapshot();
    assert_eq!(
        (split_snapshot.cursor_row, split_snapshot.cursor_col),
        (snapshot.cursor_row, snapshot.cursor_col)
    );
    // Reading the render snapshot and captured frame must preserve cluster text.
    let frame = screen.capture_frame();
    for (_, _, text, _) in cells(&mut screen) {
        assert!(frame.cells.iter().any(|cell| cell.symbol == text));
    }
}

/// libghostty-vt 1.0.1 legacy appendGrapheme does not dirty the row. The screen
/// updates RenderState after each write, so a final combining mark is absent
/// until another row mutation refreshes it. Mode 2027's print path dirties it.
fn expected_split_cells(
    whole: &mut TerminalScreen,
    kind: TerminalEngineKind,
    enabled: bool,
    case: usize,
) -> Vec<(usize, usize, String, usize)> {
    let mut expected = cells(whole);
    if kind == TerminalEngineKind::Ghostty && !enabled && [0, 1, 2, 3, 8, 9].contains(&case) {
        let last = expected.last_mut().unwrap();
        last.2 = last.2.chars().next().unwrap().to_string();
    }
    expected
}
