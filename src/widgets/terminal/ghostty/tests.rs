use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// Collects every reply the terminal sends to the program.
fn capture_replies(terminal: &mut Terminal) -> Rc<RefCell<Vec<u8>>> {
    let replies = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&replies);
    terminal
        .on_write_pty(Some(Box::new(move |bytes| {
            sink.borrow_mut().extend_from_slice(bytes)
        })))
        .unwrap();
    replies
}

/// The text of every captured row, trailing blanks trimmed.
fn row_texts(state: &mut RenderState) -> Vec<String> {
    let mut texts = Vec::new();
    let mut rows = state.rows();
    while let Some(mut row) = rows.next() {
        let mut text = String::new();
        let mut cells = row.cells();
        while let Some(cell) = cells.next() {
            if cell.wide() == CellWide::SpacerTail {
                continue;
            }
            if cell.has_text() {
                cell.push_text(&mut text);
            } else {
                text.push(' ');
            }
        }
        texts.push(text.trim_end().to_owned());
    }
    texts
}

fn rendered(terminal: &mut Terminal) -> (RenderState, Vec<String>) {
    let mut state = RenderState::new().unwrap();
    state.update(terminal).unwrap();
    let texts = row_texts(&mut state);
    (state, texts)
}

#[test]
fn renders_text_styles_graphemes_and_cursor() {
    let mut terminal = Terminal::new(20, 4).unwrap();
    terminal.write("Hi \x1b[1;31mred\x1b[0m e\u{301} 日本\r\n> ".as_bytes());

    let (mut state, texts) = rendered(&mut terminal);
    assert_eq!(texts[0], "Hi red e\u{301} 日本");
    assert_eq!(texts[1], ">");
    assert_eq!(state.dirty(), Dirty::Full);

    let mut rows = state.rows();
    let mut row = rows.next().unwrap();
    let mut cells = row.cells();
    let plain = cells.select(0).unwrap().style();
    assert_eq!(plain, Style::default());
    let red = cells.select(3).unwrap();
    let style = red.style();
    assert!(style.bold);
    assert_eq!(style.fg, StyleColor::Palette(1));
    assert!(red.fg().is_some(), "palette colors resolve to RGB");
    assert_eq!(cells.select(7).unwrap().text(), "e\u{301}");
    assert_eq!(cells.select(9).unwrap().wide(), CellWide::Wide);
    assert_eq!(cells.select(10).unwrap().wide(), CellWide::SpacerTail);

    let cursor = state.cursor();
    assert_eq!(cursor.position, Some((2, 1)));
    assert!(cursor.visible);
    assert_eq!(terminal.cursor_position(), (2, 1));

    state.clean();
    assert_eq!(state.dirty(), Dirty::Clean);
    terminal.write(b"x");
    assert_eq!(state.update(&mut terminal).unwrap(), Dirty::Partial);
    let mut rows = state.rows();
    let dirty_y = rows.next_dirty().map(|row| row.viewport_y());
    assert_eq!(dirty_y, Some(1));
}

#[test]
fn soft_wraps_and_reflows_on_resize() {
    let mut terminal = Terminal::new(10, 4).unwrap();
    terminal.write(b"abcdefghijKLMNO");
    assert_eq!(terminal.row_wraps(Point::Active { x: 0, y: 0 }), Some(true));

    let (mut state, texts) = rendered(&mut terminal);
    assert_eq!(&texts[..2], ["abcdefghij", "KLMNO"]);
    let mut rows = state.rows();
    let first = rows.next().unwrap();
    assert!(first.wraps());
    let second = rows.next().unwrap();
    assert!(second.is_wrap_continuation());

    terminal.resize(20, 4, 8, 16).unwrap();
    assert_eq!(terminal.cols(), 20);
    let (_, texts) = rendered(&mut terminal);
    assert_eq!(texts[0], "abcdefghijKLMNO");
    assert_eq!(texts[1], "");
    assert_eq!(
        terminal.row_wraps(Point::Active { x: 0, y: 0 }),
        Some(false)
    );
}

#[test]
fn answers_device_queries_through_write_pty() {
    let mut terminal = Terminal::new(20, 5).unwrap();
    let replies = capture_replies(&mut terminal);

    terminal.write(b"\x1b[c");
    assert_eq!(replies.borrow().as_slice(), b"\x1b[?62;22c");
    replies.borrow_mut().clear();

    terminal.write(b"\x1b[3;5H\x1b[6n");
    assert_eq!(replies.borrow().as_slice(), b"\x1b[3;5R");
    replies.borrow_mut().clear();

    // Mode query (DECRQM) for bracketed paste, then after enabling it.
    terminal.write(b"\x1b[?2004$p\x1b[?2004h\x1b[?2004$p");
    assert_eq!(replies.borrow().as_slice(), b"\x1b[?2004;2$y\x1b[?2004;1$y");
    assert_eq!(terminal.mode(Mode::BRACKETED_PASTE), Some(true));
}

#[test]
fn reports_program_status_and_answers_its_support_query() {
    let mut terminal = Terminal::new(20, 5).unwrap();
    let replies = capture_replies(&mut terminal);
    let reports = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reports);
    terminal
        .on_program_status(Some(Box::new(move |status: &ProgramStatus<'_>| {
            sink.borrow_mut().push((
                status.state,
                status.kind,
                status.progress,
                String::from_utf8_lossy(status.app).into_owned(),
                String::from_utf8_lossy(status.message).into_owned(),
            ));
        })))
        .unwrap();

    terminal.write(b"\x1b]7501;?\x1b\\");
    assert!(replies.borrow().starts_with(b"\x1b]7501;?:states="));

    terminal.write(b"\x1b]7501;state=blocked:kind=permission:app=terraform:msg=QXBwbHk/\x1b\\");
    terminal.write(b"\x1b]7501;state=working:progress=40\x07");
    assert_eq!(
        *reports.borrow(),
        [
            (
                ProgramStatusState::Blocked,
                ProgramStatusKind::Permission,
                None,
                "terraform".to_owned(),
                "Apply?".to_owned(),
            ),
            (
                ProgramStatusState::Working,
                ProgramStatusKind::None,
                Some(40),
                String::new(),
                String::new(),
            ),
        ]
    );
}

#[test]
fn reports_semantic_prompt_steps() {
    let mut terminal = Terminal::new(20, 5).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&events);
    terminal
        .on_semantic_prompt(Some(Box::new(move |event: &SemanticPrompt<'_>| {
            sink.borrow_mut().push((event.kind, event.exit_code));
        })))
        .unwrap();

    terminal.write(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;3\x07");
    assert_eq!(
        *events.borrow(),
        [
            (SemanticPromptKind::PromptStart(PromptKind::Primary), None),
            (SemanticPromptKind::InputStart, None),
            (SemanticPromptKind::OutputStart, None),
            (SemanticPromptKind::CommandEnd, Some(3)),
        ]
    );
}

#[test]
fn delivers_title_pwd_bell_clipboard_and_pointer_shape() {
    let mut terminal = Terminal::new(20, 5).unwrap();
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = Rc::clone(&log);
    terminal
        .on_title_changed(Some(Box::new(move |title| {
            sink.borrow_mut()
                .push(format!("title {}", String::from_utf8_lossy(title)));
        })))
        .unwrap();
    let sink = Rc::clone(&log);
    terminal
        .on_pwd_changed(Some(Box::new(move |pwd| {
            sink.borrow_mut()
                .push(format!("pwd {}", String::from_utf8_lossy(pwd)));
        })))
        .unwrap();
    let sink = Rc::clone(&log);
    terminal
        .on_bell(Some(Box::new(move || {
            sink.borrow_mut().push("bell".into())
        })))
        .unwrap();
    let sink = Rc::clone(&log);
    terminal
        .on_clipboard_write(Some(Box::new(move |write: &ClipboardWrite<'_>| {
            for content in &write.contents {
                sink.borrow_mut().push(format!(
                    "clip {:?} {} {}",
                    write.location,
                    String::from_utf8_lossy(content.mime),
                    String::from_utf8_lossy(content.data),
                ));
            }
            ClipboardWriteReply {
                result: ClipboardWriteResult::Success,
                remember: false,
            }
        })))
        .unwrap();

    terminal.write(b"\x1b]2;build\x07\x1b]7;file://host/tmp\x07\x07\x1b]52;c;aGVsbG8=\x07");
    terminal.write(b"\x1b]22;pointer\x07");
    assert_eq!(
        *log.borrow(),
        [
            "title build",
            "pwd file://host/tmp",
            "bell",
            "clip Standard text/plain hello",
        ]
    );
    assert_eq!(terminal.title(), b"build");
    assert_eq!(
        terminal.pointer_shape(),
        crate::style::PointerShape::Pointer
    );
}

#[test]
fn reads_hyperlinks_by_point() {
    let mut terminal = Terminal::new(20, 3).unwrap();
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain");
    let uri = terminal.hyperlink_uri(Point::Viewport { x: 1, y: 0 });
    assert_eq!(uri.as_deref(), Some(&b"https://example.com"[..]));
    assert_eq!(terminal.hyperlink_uri(Point::Viewport { x: 6, y: 0 }), None);

    let mut state = RenderState::new().unwrap();
    state.update(&mut terminal).unwrap();
    let mut rows = state.rows();
    let mut row = rows.next().unwrap();
    assert!(row.may_have_hyperlink());
    let mut cells = row.cells();
    assert!(cells.select(0).unwrap().has_hyperlink());
    assert!(!cells.select(6).unwrap().has_hyperlink());
}

#[test]
fn tracked_ref_follows_scrolling_and_dies_with_its_page() {
    let mut terminal = Terminal::new(10, 3).unwrap();
    terminal
        .set_scrollback_limits(ScrollbackLimits {
            max_lines: Some(0),
            max_bytes: Some(1),
        })
        .unwrap();
    terminal.write(b"first\r\n");
    let anchor = terminal.track(Point::Active { x: 0, y: 0 }).unwrap();
    assert_eq!(anchor.text().as_deref(), Some("f"));

    // Two more lines push "first" from the active area into history.
    terminal.write(b"second\r\nthird\r\nfourth");
    assert!(anchor.has_value());
    assert_eq!(anchor.point(Point::Active { x: 0, y: 0 }), None);
    assert_eq!(
        anchor.point(Point::Screen { x: 0, y: 0 }),
        Some(Point::Screen { x: 0, y: 0 })
    );
    assert_eq!(anchor.text().as_deref(), Some("f"));

    // Fill enough pages that the one holding "first" is pruned.
    let filler = "0123456789\r\n".repeat(20_000);
    terminal.write(filler.as_bytes());
    assert!(!anchor.has_value());
    assert_eq!(anchor.text(), None);
    drop(terminal);
    // Freeing the reference after its terminal is allowed.
    drop(anchor);
}

#[test]
fn scrollback_limits_round_trip_and_viewport_scrolls() {
    let mut terminal = Terminal::new(10, 3).unwrap();
    assert_eq!(terminal.scrollback_limits(), ScrollbackLimits::default());

    terminal.write("line\r\n".repeat(50).as_bytes());
    assert!(terminal.scrollback_rows() >= 47);
    assert!(terminal.viewport_active());
    terminal.scroll_viewport(ScrollViewport::Top).unwrap();
    assert!(!terminal.viewport_active());
    assert_eq!(terminal.scrollbar().offset, 0);
    terminal.scroll_viewport(ScrollViewport::Bottom).unwrap();
    assert!(terminal.viewport_active());

    let usage = terminal.memory_usage();
    assert!(usage.primary_pages > 0);
    assert!(usage.resident_bytes() > 0);
    terminal.compress(CompressionMode::Full).unwrap();
}

#[test]
fn callback_panic_resumes_after_the_write() {
    let mut terminal = Terminal::new(10, 3).unwrap();
    terminal
        .on_bell(Some(Box::new(|| panic!("bell handler failed"))))
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        terminal.write(b"a\x07b");
    }));
    let payload = result.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"bell handler failed"));
    // The write still completed; the terminal is usable.
    assert_eq!(
        terminal.cell_text(Point::Active { x: 1, y: 0 }).as_deref(),
        Some("b")
    );
}
