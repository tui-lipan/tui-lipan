use tui_lipan::OverlayScope;
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct FrameAndText;

impl Component for FrameAndText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Panel")
            .child(Text::new("hello frame"))
            .into()
    }
}

struct StyledCellText;

impl Component for StyledCellText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("A")
            .style(
                Style::new()
                    .fg(Color::Rgb(12, 34, 56))
                    .bg(Color::Rgb(90, 80, 70)),
            )
            .into()
    }
}

struct StyledRunsText;

impl Component for StyledRunsText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::from_spans([
            Span::new("aa").style(Style::new().fg(Color::Rgb(200, 1, 1))),
            Span::new("BB").style(Style::new().fg(Color::Rgb(1, 200, 1))),
            Span::new("cc").style(Style::new().fg(Color::Rgb(1, 1, 200))),
        ])
        .into()
    }
}

struct FocusedInput;

impl Component for FocusedInput {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Input::new("abc").cursor(1).width(Length::Px(12)).into()
    }
}

struct OverlayWithModal;

impl Component for OverlayWithModal {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Text::new("base content"))
            .child(
                Modal::new()
                    .scope(OverlayScope::RootPortal)
                    .title("Dialog")
                    .child(Text::new("modal body")),
            )
            .into()
    }
}

#[cfg(feature = "diff-view")]
struct SplitDiffThemedFiller;

#[cfg(feature = "diff-view")]
impl Component for SplitDiffThemedFiller {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut theme = Theme::default();
        theme.diff.context = Style::new().bg(Color::Rgb(20, 30, 40));
        theme.diff.empty = Style::new().bg(Color::Rgb(30, 50, 70));
        theme.diff.added = Style::new().fg(Color::Green).bg(Color::Rgb(10, 60, 40));
        theme.diff.removed = Style::new().fg(Color::Red).bg(Color::Rgb(70, 20, 20));
        theme.diff.context_line_number = Style::new().fg(Color::DarkGray);

        let diff = DiffView::with_content(
            "same\nshort\nend\n",
            "same\nthis is a very long inserted line that wraps in the right pane\nend\n",
        )
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .width(Length::Percent(100))
        .wrap(true)
        .scrollbar(false)
        .h_scrollbar(false)
        .border(false)
        .panels_border(false)
        .highlight_full_width(true)
        .gutter_inset(1);

        ThemeProvider::new(theme)
            .child(
                Frame::new()
                    .border(false)
                    .style(Style::new().bg(Color::Rgb(120, 10, 10)))
                    .child(diff),
            )
            .into()
    }
}

#[test]
fn plain_text_includes_frame_title_and_text_content() {
    let mut backend = TestBackend::new(FrameAndText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 28,
        h: 6,
    });
    backend.render();

    let captured = backend.capture_frame();
    let plain = captured.plain_text();

    assert!(plain.contains("Panel"));
    assert!(plain.contains("hello frame"));
}

#[test]
fn cell_returns_expected_colors_for_styled_text() {
    let mut backend = TestBackend::new(StyledCellText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    let cell = captured.cell(0, 0);

    assert_eq!(cell.symbol, "A");
    assert_eq!(cell.fg, Color::Rgb(12, 34, 56));
    assert_eq!(cell.bg, Color::Rgb(90, 80, 70));
}

#[test]
fn row_length_matches_viewport_width() {
    let mut backend = TestBackend::new(StyledCellText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 17,
        h: 3,
    });
    backend.render();

    let captured = backend.capture_frame();
    assert_eq!(captured.width, 17);
    assert_eq!(captured.row(0).len(), usize::from(captured.width));
}

#[test]
fn styled_lines_groups_runs_by_style() {
    let mut backend = TestBackend::new(StyledRunsText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 6,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    let runs = &captured.styled_lines()[0];

    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].0, "aa");
    assert_eq!(runs[1].0, "BB");
    assert_eq!(runs[2].0, "cc");
    assert_eq!(
        runs[0].1.fg,
        Some(tui_lipan::Paint::Solid(Color::Rgb(200, 1, 1)))
    );
    assert_eq!(
        runs[1].1.fg,
        Some(tui_lipan::Paint::Solid(Color::Rgb(1, 200, 1)))
    );
    assert_eq!(
        runs[2].1.fg,
        Some(tui_lipan::Paint::Solid(Color::Rgb(1, 1, 200)))
    );
}

#[test]
fn focused_input_captures_cursor_position() {
    let mut backend = TestBackend::new(FocusedInput);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.focus_next();
    backend.render();

    let captured = backend.capture_frame();
    let cursor = captured
        .cursor
        .expect("cursor should be present when input is focused");

    assert!(cursor.visible);
    assert!(cursor.x < captured.width);
    assert!(cursor.y < captured.height);
}

#[cfg(feature = "diff-view")]
#[test]
fn split_document_diff_themed_empty_and_wrap_padding_rows_paint_background() {
    let mut backend = TestBackend::new(SplitDiffThemedFiller);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 42,
        h: 6,
    });
    backend.render();

    let captured = backend.capture_frame();
    let filler_bg = Color::Rgb(30, 50, 70);
    let removed_bg = Color::Rgb(70, 20, 20);

    // Row 1 is the real removed line and keeps its diff coloring.
    assert_eq!(captured.cell(0, 1).bg, removed_bg);
    assert_eq!(captured.cell(8, 1).bg, removed_bg);

    // Row 2 is synthetic split-wrap padding inserted to align with the
    // wrapped added line in the right pane. It should use the neutral empty
    // background, not the removed-line color from row 1.
    assert_eq!(captured.cell(0, 2).bg, filler_bg);
    assert_eq!(captured.cell(8, 2).bg, filler_bg);
}

#[test]
fn viewport_resize_changes_captured_dimensions() {
    let mut backend = TestBackend::new(FrameAndText);

    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 22,
        h: 5,
    });
    backend.render();
    let first = backend.capture_frame();

    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 31,
        h: 8,
    });
    backend.render();
    let second = backend.capture_frame();

    assert_eq!(first.width, 22);
    assert_eq!(first.height, 5);
    assert_eq!(second.width, 31);
    assert_eq!(second.height, 8);
}

#[test]
fn root_portal_modal_content_appears_in_captured_frame() {
    let mut backend = TestBackend::new(OverlayWithModal);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();

    let captured = backend.capture_frame();
    let plain = captured.plain_text();

    assert!(plain.contains("modal body"));
}

struct PaddedBorderText;

impl Component for PaddedBorderText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("X").width(Length::Px(10)).into()
    }
}

#[test]
fn fixed_grid_preserves_trailing_spaces() {
    let mut backend = TestBackend::new(PaddedBorderText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    let fixed = captured.to_fixed_grid();
    let trimmed = captured.plain_text();

    assert_eq!(fixed.len(), 10);
    assert!(fixed.ends_with(' '));
    assert_eq!(trimmed.trim_end(), "X");
}

#[test]
fn to_ansi_round_trips_styled_text_via_parse_ansi() {
    let mut backend = TestBackend::new(StyledCellText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    let ansi = captured.to_ansi();
    let spans = tui_lipan::style::parse_ansi(&ansi);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains('A'));
}

#[test]
fn to_ansi_diff_skips_unchanged_cells() {
    let mut backend = TestBackend::new(StyledCellText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    });
    backend.render();

    let first = backend.capture_frame();
    backend.render();
    let second = backend.capture_frame();

    let diff = second.to_ansi_diff(Some(&first));
    assert!(!diff.contains("\x1b[2J"));
}

struct WideCharText;

impl Component for WideCharText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("A中B").into()
    }
}

#[test]
fn fixed_grid_wide_char_row_matches_viewport_width() {
    let mut backend = TestBackend::new(WideCharText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 6,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    assert_eq!(captured.row(0).len(), usize::from(captured.width));
    assert!(captured.to_fixed_grid().contains('中'));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_bytes_signature_and_dimensions_match_options() {
    let mut backend = TestBackend::new(StyledCellText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 2,
    });
    backend.render();

    let captured = backend.capture_frame();
    let options = tui_lipan::PngOptions {
        cell_width: 3,
        cell_height: 5,
        scale: 2,
        ..Default::default()
    };
    let png = captured.to_png(&options);

    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let decoded = image::load_from_memory(&png).expect("png should decode");
    assert_eq!(decoded.width(), 4 * 3 * 2);
    assert_eq!(decoded.height(), 2 * 5 * 2);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_bitmap_renderer_forces_legacy_bitmap_output() {
    let frame = single_cell_frame("A", Color::White, Color::Black);
    let bitmap = frame.to_png(&tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        ..Default::default()
    });
    let bitmap_with_font_preferences = frame.to_png(&tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        font_family: Some(std::sync::Arc::from("Definitely Missing Font Family")),
        font_path: Some(std::path::PathBuf::from("/definitely/missing/font.ttf")),
        ..Default::default()
    });

    assert_eq!(bitmap, bitmap_with_font_preferences);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_font_renderer_uses_system_font_when_available() {
    let Some(font_family) = available_test_monospace_family() else {
        return;
    };
    let frame = single_cell_frame("A", Color::White, Color::Black);
    let options = tui_lipan::PngOptions {
        cell_width: 12,
        cell_height: 18,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Font,
        font_family: Some(std::sync::Arc::from(font_family.as_str())),
        ..Default::default()
    };
    let font_png = frame.to_png(&options);
    let bitmap_png = frame.to_png(&tui_lipan::PngOptions {
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        ..options.clone()
    });

    assert!(font_png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_ne!(font_png, bitmap_png);
    let decoded = image::load_from_memory(&font_png)
        .expect("png should decode")
        .to_rgb8();
    assert_eq!((decoded.width(), decoded.height()), (12, 18));
    assert!(
        decoded
            .pixels()
            .any(|pixel| pixel.0 != [0, 0, 0] && pixel.0 != [255, 255, 255]),
        "font rendering should include antialiased pixels"
    );
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_zero_options_are_clamped_and_scale_keeps_legacy_dimensions() {
    let captured = single_cell_frame("A", Color::White, Color::Black);
    let zero_options = tui_lipan::PngOptions {
        cell_width: 0,
        cell_height: 0,
        scale: 0,
        ..Default::default()
    };
    let scaled_options = tui_lipan::PngOptions {
        cell_width: 3,
        cell_height: 5,
        scale: 2,
        ..Default::default()
    };

    let zero_png = captured.to_png(&zero_options);
    let scaled_png = captured.to_png(&scaled_options);
    let zero_decoded = image::load_from_memory(&zero_png).expect("png should decode");
    let scaled_decoded = image::load_from_memory(&scaled_png).expect("png should decode");

    assert_eq!((zero_decoded.width(), zero_decoded.height()), (1, 1));
    assert_eq!((scaled_decoded.width(), scaled_decoded.height()), (6, 10));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_wide_character_rendering_does_not_panic() {
    let mut backend = TestBackend::new(WideCharText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 6,
        h: 1,
    });
    backend.render();

    let captured = backend.capture_frame();
    let png = captured.to_png(&tui_lipan::PngOptions::default());

    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_grapheme_and_multi_codepoint_symbols_do_not_panic() {
    let mut frame = single_cell_frame("👨‍👩‍👧‍👦", Color::White, Color::Black);
    frame.width = 2;
    frame.viewport.w = 2;
    frame.cells.push(tui_lipan::CapturedCell {
        symbol: " ".to_string(),
        fg: Color::White,
        bg: Color::Black,
        underline_color: Color::Reset,
        modifiers: tui_lipan::CellModifiers::default(),
    });

    let png = frame.to_png(&tui_lipan::PngOptions::default());
    let decoded = image::load_from_memory(&png).expect("png should decode");

    assert_eq!(decoded.width(), u32::from(frame.width) * 8 * 2);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_private_use_placeholder_differs_from_literal_question_mark() {
    let options = tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        ..Default::default()
    };
    let pua = single_cell_frame("\u{e000}", Color::White, Color::Black).to_png(&options);
    let question = single_cell_frame("?", Color::White, Color::Black).to_png(&options);
    let decoded = image::load_from_memory(&pua)
        .expect("png should decode")
        .to_rgb8();
    let placeholder_pixels = (0..decoded.width())
        .flat_map(|x| (0..decoded.height()).map(move |y| (x, y)))
        .filter(|&(x, y)| decoded.get_pixel(x, y).0 == [255, 255, 255])
        .count();

    assert_ne!(pua, question);
    assert!(placeholder_pixels > 0, "placeholder should be visible");
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_common_tui_symbol_fallbacks_are_visible() {
    let mut frame = single_cell_frame("▶", Color::Rgb(250, 10, 20), Color::Black);
    frame.width = 4;
    frame.viewport.w = 4;
    frame.cells.extend(
        ["✓", "×", "⠋"]
            .into_iter()
            .map(|symbol| tui_lipan::CapturedCell {
                symbol: symbol.to_string(),
                fg: Color::Rgb(250, 10, 20),
                bg: Color::Black,
                underline_color: Color::Reset,
                modifiers: tui_lipan::CellModifiers::default(),
            }),
    );

    let png = frame.to_png(&tui_lipan::PngOptions {
        cell_width: 8,
        cell_height: 16,
        scale: 1,
        text_renderer: tui_lipan::PngTextRenderer::Bitmap,
        ..Default::default()
    });
    let decoded = image::load_from_memory(&png)
        .expect("png should decode")
        .to_rgb8();

    for cell_x in 0..4 {
        let x0 = cell_x * 8;
        let visible_pixels = (x0..x0 + 8)
            .flat_map(|x| (0..16).map(move |y| (x, y)))
            .filter(|&(x, y)| decoded.get_pixel(x, y).0 == [250, 10, 20])
            .count();
        assert!(
            visible_pixels > 0,
            "fallback cell {cell_x} should draw pixels"
        );
    }
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_frame_border_cell_contains_foreground_pixels() {
    let frame = single_cell_frame("─", Color::Rgb(7, 200, 9), Color::Black);
    let png = frame.to_png(&tui_lipan::PngOptions {
        cell_width: 10,
        cell_height: 9,
        scale: 1,
        ..Default::default()
    });
    let decoded = image::load_from_memory(&png)
        .expect("png should decode")
        .to_rgb8();

    let border_pixels = (0..decoded.width())
        .flat_map(|x| (0..decoded.height()).map(move |y| (x, y)))
        .filter(|&(x, y)| decoded.get_pixel(x, y).0 == [7, 200, 9])
        .count();
    assert!(border_pixels > 0, "box drawing glyph should remain visible");
    for x in 0..decoded.width() {
        assert!(
            (0..decoded.height()).any(|y| decoded.get_pixel(x, y).0 == [7, 200, 9]),
            "box drawing glyph should cover non-8-multiple column {x}"
        );
    }
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_cursor_render_on_and_off_changes_output() {
    let mut backend = TestBackend::new(FocusedInput);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.focus_next();
    backend.render();

    let captured = backend.capture_frame();
    assert!(
        captured
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.visible)
    );

    let options_on = tui_lipan::PngOptions::default();
    let options_off = tui_lipan::PngOptions {
        render_cursor: false,
        ..Default::default()
    };

    assert_ne!(captured.to_png(&options_on), captured.to_png(&options_off));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_try_to_png_returns_bytes_and_cursor_uses_cell_foreground() {
    let frame = tui_lipan::CapturedFrame {
        viewport: Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
        width: 1,
        height: 1,
        cells: vec![tui_lipan::CapturedCell {
            symbol: "X".to_string(),
            fg: Color::Rgb(1, 2, 3),
            bg: Color::Rgb(4, 5, 6),
            underline_color: Color::Reset,
            modifiers: tui_lipan::CellModifiers::default(),
        }],
        cursor: Some(tui_lipan::CursorState {
            x: 0,
            y: 0,
            visible: true,
        }),
    };
    let options = tui_lipan::PngOptions {
        cell_width: 4,
        cell_height: 4,
        scale: 1,
        default_fg: Color::White,
        ..Default::default()
    };

    let png = frame.try_to_png(&options).expect("png should encode");
    let decoded = image::load_from_memory(&png)
        .expect("png should decode")
        .to_rgb8();

    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(decoded.get_pixel(0, 0).0, [1, 2, 3]);
}

#[cfg(feature = "ui-snapshot-png")]
fn available_test_monospace_family() -> Option<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    [
        "JetBrains Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
    ]
    .into_iter()
    .find(|family| {
        let families = [fontdb::Family::Name(family)];
        db.query(&fontdb::Query {
            families: &families,
            ..fontdb::Query::default()
        })
        .is_some()
    })
    .map(str::to_string)
    .or_else(|| {
        let families = [fontdb::Family::Monospace];
        db.query(&fontdb::Query {
            families: &families,
            ..fontdb::Query::default()
        })?;
        Some("monospace".to_string())
    })
}

#[cfg(feature = "ui-snapshot-png")]
fn single_cell_frame(symbol: &str, fg: Color, bg: Color) -> tui_lipan::CapturedFrame {
    tui_lipan::CapturedFrame {
        viewport: Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
        width: 1,
        height: 1,
        cells: vec![tui_lipan::CapturedCell {
            symbol: symbol.to_string(),
            fg,
            bg,
            underline_color: Color::Reset,
            modifiers: tui_lipan::CellModifiers::default(),
        }],
        cursor: None,
    }
}
