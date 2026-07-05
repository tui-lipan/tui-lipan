use tui_lipan::TestBackend;
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, UiSnapshot, UiSnapshotOptions, UiWidgetDesc, UiWidgetKind};

#[cfg(feature = "ui-snapshot-png")]
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

struct Dashboard;

impl Component for Dashboard {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        HStack::new()
            .child(
                Frame::new()
                    .title("Sidebar")
                    .width(Length::Px(20))
                    .child(
                        List::new()
                            .items(["Home", "Settings"].map(ListItem::new))
                            .selected(1)
                            .key("nav"),
                    )
                    .key("sidebar"),
            )
            .child(
                Frame::new()
                    .title("Content")
                    .child(Text::new("Hello agent"))
                    .key("main"),
            )
            .into()
    }
}

struct MaskedInput;

impl Component for MaskedInput {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Input::new("secret")
            .mask(Some('*'))
            .width(Length::Px(16))
            .key("password")
    }
}

struct LongList;

impl Component for LongList {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .items((0..30).map(|i| ListItem::new(format!("Item {i}"))))
            .key("long-list")
    }
}

struct MiniText;

impl Component for MiniText {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("fit").into()
    }
}

#[test]
fn describe_includes_frame_list_and_selection() {
    let mut backend = TestBackend::new(Dashboard);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 50,
        h: 12,
    });
    backend.render();

    let snapshot = backend.capture_ui_snapshot();
    let list = snapshot
        .widgets
        .iter()
        .find(|w| w.kind == UiWidgetKind::List)
        .expect("list widget");

    assert_eq!(list.selected_index, Some(1));
    assert_eq!(
        list.item_labels.as_deref(),
        Some(["Home".to_string(), "Settings".to_string()].as_slice())
    );

    let frame = snapshot
        .widgets
        .iter()
        .find(|w| {
            w.kind == UiWidgetKind::Frame && w.key.as_ref().is_some_and(|k| k.as_ref() == "sidebar")
        })
        .expect("sidebar frame");
    assert_eq!(frame.title.as_deref(), Some("Sidebar"));
}

#[test]
fn masked_input_sets_value_masked_flag() {
    let mut backend = TestBackend::new(MaskedInput);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 3,
    });
    backend.render();

    let snapshot = backend.capture_ui_snapshot();
    let input = snapshot
        .widgets
        .iter()
        .find(|w| w.kind == UiWidgetKind::Input)
        .expect("input");

    assert!(input.value_masked);
    assert!(input.value.is_none());
}

#[test]
fn list_truncation_reports_total_items() {
    let mut backend = TestBackend::new(LongList);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 20,
    });
    backend.render();

    let options = UiSnapshotOptions {
        max_list_items: 5,
        ..UiSnapshotOptions::default()
    };
    let snapshot = backend.capture_ui_snapshot_with_options(&options);
    let list = snapshot
        .widgets
        .iter()
        .find(|w| w.kind == UiWidgetKind::List)
        .expect("list");

    assert_eq!(list.total_items, Some(30));
    assert_eq!(list.item_labels.as_ref().map(|v| v.len()), Some(5));
}

#[test]
fn markdown_includes_fixed_grid() {
    let mut backend = TestBackend::new(Dashboard);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();

    let markdown = backend.capture_ui_snapshot().to_markdown();
    assert!(markdown.contains("## Render"));
    assert!(markdown.contains("Hello agent"));
}

#[test]
fn diagnostic_options_include_hidden_nodes_and_mark_zero_area() {
    let options = UiSnapshotOptions::diagnostic();
    assert!(options.include_zero_area);
    assert!(options.include_chrome);
    assert_eq!(
        options.max_list_items,
        UiSnapshotOptions::default().max_list_items
    );

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
    let snapshot = UiSnapshot {
        viewport,
        frame: CapturedFrame {
            viewport,
            width: 0,
            height: 0,
            cells: Vec::new(),
            cursor: None,
        },
        widgets: vec![UiWidgetDesc {
            kind: UiWidgetKind::Text,
            key: None,
            rect: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 1,
            },
            focused: true,
            hovered: true,
            title: None,
            label: Some("hidden".to_string()),
            placeholder: None,
            value: None,
            value_masked: false,
            checkbox_state: None,
            selected_index: None,
            scroll_offset: None,
            item_labels: None,
            total_items: None,
            child_count: None,
        }],
        focus_key: None,
        hover_key: None,
    };

    let markdown = snapshot.to_markdown();

    assert!(markdown.contains("- **Text** @ (0,0) 0x1 [focused, hovered, zero-area]"));
}

#[test]
fn margin_frame_capture_uses_content_min_size_and_restores_viewport() {
    let mut backend = TestBackend::new(MiniText);
    let original = Rect {
        x: 2,
        y: 3,
        w: 40,
        h: 10,
    };
    backend.set_viewport(original);
    backend.render();

    let min_size = backend.content_min_size();
    assert_eq!(min_size, (3, 1));

    let frame = backend.capture_frame_with_margin(20, 8);

    assert_eq!(frame.width, min_size.0 + 20);
    assert_eq!(frame.height, min_size.1 + 8);
    assert_eq!(frame.viewport.x, 0);
    assert_eq!(frame.viewport.y, 0);
    assert_eq!(backend.viewport(), original);

    let restored_frame = backend.capture_frame();
    assert_eq!(restored_frame.width, original.w);
    assert_eq!(restored_frame.height, original.h);
    assert_eq!(restored_frame.viewport, original);
}

#[test]
fn margin_ui_snapshot_uses_content_min_size_plus_margin() {
    let mut backend = TestBackend::new(MiniText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();

    let (min_w, min_h) = backend.content_min_size();
    let snapshot = backend.capture_ui_snapshot_with_margin(4, 2, &UiSnapshotOptions::default());

    assert_eq!(snapshot.viewport.w, min_w + 4);
    assert_eq!(snapshot.viewport.h, min_h + 2);
    assert_eq!(snapshot.frame.width, min_w + 4);
    assert_eq!(snapshot.frame.height, min_h + 2);
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_default_starts_with_png_signature() {
    let mut backend = TestBackend::new(Dashboard);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 4,
    });
    backend.render();

    let png = backend.capture_ui_snapshot().to_png_default();

    assert!(png.starts_with(PNG_SIGNATURE));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_try_default_surfaces_successful_encoding() {
    let mut backend = TestBackend::new(Dashboard);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 4,
    });
    backend.render();

    let png = backend
        .capture_ui_snapshot()
        .try_to_png_default()
        .expect("png should encode");

    assert!(png.starts_with(PNG_SIGNATURE));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_default_uses_default_cell_and_scale_dimensions() {
    let mut backend = TestBackend::new(Dashboard);
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 4,
    };
    backend.set_viewport(viewport);
    backend.render();

    let png = backend.capture_ui_snapshot().to_png_default();
    let (width, height) = png_dimensions(&png);
    let options = tui_lipan::capture::PngOptions::default();

    assert_eq!(
        width,
        u32::from(viewport.w) * options.cell_width as u32 * options.scale as u32
    );
    assert_eq!(
        height,
        u32::from(viewport.h) * options.cell_height as u32 * options.scale as u32
    );
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn png_file_format_is_selectable() {
    let format = tui_lipan::UiSnapshotFileFormat::Png;
    assert!(matches!(format, tui_lipan::UiSnapshotFileFormat::Png));
}

#[cfg(feature = "ui-snapshot-png")]
fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(PNG_SIGNATURE));
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("png width bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("png height bytes"));
    (width, height)
}

#[cfg(feature = "ui-snapshot-json")]
#[test]
fn json_handles_special_characters() {
    struct SpecialText;

    impl Component for SpecialText {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("quote\" newline\n tab\t").into()
        }
    }

    let mut backend = TestBackend::new(SpecialText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();

    let json = backend.capture_ui_snapshot().to_json();
    assert!(json.contains("quote"));
    serde_json::from_str::<serde_json::Value>(&json).expect("valid json");
}

#[test]
fn markdown_escapes_backticks_and_newlines() {
    struct SpecialMarkdown;

    impl Component for SpecialMarkdown {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Frame::new()
                .title("tick`tick")
                .child(Text::new("line\nbreak"))
                .key("panel")
        }
    }

    let mut backend = TestBackend::new(SpecialMarkdown);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 6,
    });
    backend.render();

    let markdown = backend.capture_ui_snapshot().to_markdown();
    assert!(markdown.contains("``tick`tick``"));
    assert!(!markdown.contains("title: `tick`tick`"));
    assert!(markdown.contains("- label: `line\\nbreak`"));
}

#[test]
fn checkbox_uses_checkbox_state_not_selected_index() {
    struct TriCheckbox;

    impl Component for TriCheckbox {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Checkbox::new(false)
                .label("Maybe")
                .state(CheckboxState::Indeterminate)
                .key("maybe")
        }
    }

    let mut backend = TestBackend::new(TriCheckbox);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();

    let snapshot = backend.capture_ui_snapshot();
    let checkbox = snapshot
        .widgets
        .iter()
        .find(|w| w.kind == UiWidgetKind::Checkbox)
        .expect("checkbox");

    assert_eq!(checkbox.checkbox_state, Some(CheckboxState::Indeterminate));
    assert!(checkbox.selected_index.is_none());
}

#[test]
fn input_placeholder_is_separate_from_label() {
    struct PlaceholderInput;

    impl Component for PlaceholderInput {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Input::new("").placeholder("Enter name").key("name")
        }
    }

    let mut backend = TestBackend::new(PlaceholderInput);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 3,
    });
    backend.render();

    let input = backend
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == UiWidgetKind::Input)
        .expect("input");

    assert_eq!(input.placeholder.as_deref(), Some("Enter name"));
    assert!(input.label.is_none());
}

#[cfg(feature = "ui-snapshot-json")]
#[test]
fn json_uses_stable_color_wire_format() {
    struct StyledText;

    impl Component for StyledText {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("X")
                .style(Style::new().fg(Color::Rgb(1, 2, 3)).bg(Color::Indexed(42)))
                .into()
        }
    }

    let mut backend = TestBackend::new(StyledText);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    });
    backend.render();

    let options = UiSnapshotOptions::default();
    let format = tui_lipan::UiSnapshotFormatOptions {
        include_cells: true,
    };
    let json = backend
        .capture_ui_snapshot_with_options(&options)
        .to_json_with_options(&format);
    assert!(json.contains("\"fg\":\"rgb(1,2,3)\""));
    assert!(json.contains("\"bg\":\"indexed(42)\""));
    assert!(!json.contains("Rgb(1, 2, 3)"));
}
