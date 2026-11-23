//! Regression tests for squeezed/overflowed bordered widgets painting over siblings.

use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{PointerShape, TestBackend, UiSnapshotOptions};

fn rects_overlap(a: Rect, b: Rect) -> bool {
    !a.intersection(&b).is_empty()
}

fn find_rect(widgets: &[tui_lipan::UiWidgetDesc], key: &str) -> Rect {
    widgets
        .iter()
        .find(|w| w.key.as_ref().is_some_and(|k| k.as_ref() == key))
        .unwrap_or_else(|| panic!("missing widget with key '{key}'"))
        .rect
}

fn assert_no_visible_sibling_overlap(
    widgets: &[tui_lipan::UiWidgetDesc],
    keys: &[&str],
    frame_text: &str,
) {
    let rects: Vec<(&str, Rect)> = keys
        .iter()
        .map(|&key| (key, find_rect(widgets, key)))
        .collect();
    for i in 0..rects.len() {
        let (k1, r1) = rects[i];
        if r1.is_empty() {
            continue;
        }
        for &(k2, r2) in &rects[(i + 1)..] {
            if r2.is_empty() {
                continue;
            }
            assert!(
                !rects_overlap(r1, r2),
                "visible siblings '{k1}' ({r1:?}) and '{k2}' ({r2:?}) overlap!\n\
                 all sibling rects: {rects:?}\nrendered:\n{frame_text}"
            );
        }
    }
}

struct InlineLikeForm;

impl Component for InlineLikeForm {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Inline mode")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .footer_left("Enter records a line | q/Esc quits")
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Input::new("")
                            .placeholder("Type text and press Enter")
                            .key("input"),
                    )
                    .child(Button::filled("Insert").focusable(true).key("insert"))
                    .child(Text::new("").overflow(Overflow::Wrap).key("entries"))
                    .child(Text::new("Inserted lines: 0").key("counter"))
                    .key("stack"),
            )
            .into()
    }
}

#[test]
fn overflowed_inline_form_siblings_never_overlap() {
    let mut backend = TestBackend::new_with_viewport(
        InlineLikeForm,
        Rect {
            x: 0,
            y: 0,
            w: 79,
            h: 8,
        },
    );
    backend.render();

    let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
    let text = snapshot.frame.plain_text();
    let stack = find_rect(&snapshot.widgets, "stack");

    assert_no_visible_sibling_overlap(
        &snapshot.widgets,
        &["input", "insert", "entries", "counter"],
        &text,
    );

    // Leading overflowed siblings stay hidden (zero height) instead of inflating back.
    assert_eq!(find_rect(&snapshot.widgets, "input").h, 0);
    assert_eq!(find_rect(&snapshot.widgets, "insert").h, 0);
    let counter_rect = find_rect(&snapshot.widgets, "counter");
    assert_eq!(counter_rect.h, 1);

    for key in ["input", "insert", "entries", "counter"] {
        let rect = find_rect(&snapshot.widgets, key);
        if !rect.is_empty() {
            assert!(
                rect.y >= stack.y
                    && rect.y.saturating_add(rect.h as i16)
                        <= stack.y.saturating_add(stack.h as i16),
                "sibling '{key}' ({rect:?}) escaped stack bounds ({stack:?}):\n{text}"
            );
        }
    }

    let lines = snapshot.frame.to_fixed_grid_lines();
    assert!(
        lines[counter_rect.y as usize].contains("Inserted lines: 0"),
        "counter should render cleanly at its own row:\n{text}"
    );
    // No inner border glyph or dropped-sibling label may appear inside the frame body.
    for line in &lines[1..lines.len() - 1] {
        let inner = line
            .strip_prefix('│')
            .and_then(|s| s.strip_suffix('│'))
            .unwrap_or(line);
        assert!(
            !inner.contains('┌') && !inner.contains('└') && !inner.contains('─'),
            "stray input border glyph inside frame body on row: {line:?}\nfull:\n{text}"
        );
        assert!(
            !inner.contains("Type text") && !inner.contains("Insert "),
            "dropped top sibling leaked into frame body on row: {line:?}\nfull:\n{text}"
        );
    }
}

struct InlineChoicesLikeForm;

impl Component for InlineChoicesLikeForm {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        const THEMES: &[&str] = &["Minimal", "Balanced", "Loud"];
        const TARGETS: &[&str] = &[
            "Local sandbox",
            "Staging cluster",
            "Production canary",
            "Production full",
        ];

        Frame::new()
            .title("Inline choices")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .footer_left("Enter applies from note input | q/Esc quits")
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Theme").key("theme_label"))
                    .child(
                        Radio::new(THEMES.iter().copied())
                            .selected(1)
                            .layout(RadioLayout::Horizontal)
                            .gap(2)
                            .key("radio"),
                    )
                    .child(Text::new("Deployment target").key("target_label"))
                    .child(
                        Select::new()
                            .options(TARGETS.iter().copied())
                            .placeholder("Choose target")
                            .width(Length::Flex(1))
                            .key("select"),
                    )
                    .child(Input::new("").placeholder("Optional note").key("note"))
                    .child(Button::filled("Apply").focusable(true).key("apply"))
                    .child(
                        Text::new("Applied: 0 | Pick values, then apply to insert a summary above")
                            .key("status"),
                    )
                    .child(Text::new("").overflow(Overflow::Wrap).key("recent"))
                    .key("stack"),
            )
            .into()
    }
}

#[test]
fn overflowed_inline_choices_siblings_never_overlap() {
    let mut backend = TestBackend::new_with_viewport(
        InlineChoicesLikeForm,
        Rect {
            x: 0,
            y: 0,
            w: 79,
            h: 13,
        },
    );
    backend.render();

    let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
    let text = snapshot.frame.plain_text();
    let stack = find_rect(&snapshot.widgets, "stack");

    let keys = [
        "theme_label",
        "radio",
        "target_label",
        "select",
        "note",
        "apply",
        "status",
        "recent",
    ];
    assert_no_visible_sibling_overlap(&snapshot.widgets, &keys, &text);

    for key in keys {
        let rect = find_rect(&snapshot.widgets, key);
        if !rect.is_empty() {
            assert!(
                rect.y >= stack.y
                    && rect.y.saturating_add(rect.h as i16)
                        <= stack.y.saturating_add(stack.h as i16),
                "sibling '{key}' ({rect:?}) escaped stack bounds ({stack:?}):\n{text}"
            );
        }
    }

    let radio_y = find_rect(&snapshot.widgets, "radio").y as usize;
    let note_rect = find_rect(&snapshot.widgets, "note");
    let apply_y = find_rect(&snapshot.widgets, "apply").y as usize;
    let status_y = find_rect(&snapshot.widgets, "status").y as usize;
    assert_eq!(note_rect.h, 1, "note input should fit its 1-row slot");

    let lines = snapshot.frame.to_fixed_grid_lines();
    assert!(
        lines[radio_y].contains("Balanced"),
        "radio row mismatch:\n{text}"
    );
    assert!(
        lines[note_rect.y as usize].contains("Optional note"),
        "squeezed bordered input should keep placeholder legible on its row:\n{text}"
    );
    assert!(
        lines[apply_y].contains("Apply"),
        "apply row mismatch:\n{text}"
    );
    assert!(
        lines[status_y].contains("Applied: 0"),
        "status row mismatch:\n{text}"
    );

    for line in &lines[1..lines.len() - 1] {
        let inner = line
            .strip_prefix('│')
            .and_then(|s| s.strip_suffix('│'))
            .unwrap_or(line);
        assert!(
            !inner.contains('┌') && !inner.contains('└') && !inner.contains('─'),
            "squeezed border leaked into frame body on row: {line:?}\nfull:\n{text}"
        );
    }
}

struct FullscreenSqueezedBorderedWidgets {
    squeezed_height: u16,
}

impl Component for FullscreenSqueezedBorderedWidgets {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .gap(0)
            .child(Text::new("TOP_SENTINEL").height(Length::Px(1)).key("top"))
            .child(
                Element::from(
                    Input::new("hello")
                        .border(true)
                        .read_only(true)
                        .height(Length::Px(self.squeezed_height)),
                )
                .min_height(Length::Px(0))
                .key("input"),
            )
            .child(
                Element::from(
                    Frame::new()
                        .title("Box")
                        .border(true)
                        .height(Length::Px(self.squeezed_height))
                        .child(Text::new("inside")),
                )
                .min_height(Length::Px(0))
                .key("frame"),
            )
            .child(
                Element::from(Button::outlined("Act").height(Length::Px(self.squeezed_height)))
                    .min_height(Length::Px(0))
                    .key("btn"),
            )
            .child(
                Text::new("BOTTOM_SENTINEL")
                    .height(Length::Px(1))
                    .key("bottom"),
            )
            .into()
    }
}

#[test]
fn bordered_widgets_given_fewer_rows_than_chrome_never_paint_outside_rect() {
    for squeezed_height in [0u16, 1, 2] {
        let total_h = 2 + squeezed_height * 3;
        let mut backend = TestBackend::new_with_viewport(
            FullscreenSqueezedBorderedWidgets { squeezed_height },
            Rect {
                x: 0,
                y: 0,
                w: 30,
                h: total_h,
            },
        );
        backend.render();

        let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
        let text = snapshot.frame.plain_text();
        let lines = snapshot.frame.to_fixed_grid_lines();

        assert_no_visible_sibling_overlap(
            &snapshot.widgets,
            &["top", "input", "frame", "btn", "bottom"],
            &text,
        );

        assert_eq!(
            lines[0].trim_end(),
            "TOP_SENTINEL",
            "squeezed_height={squeezed_height}: top sentinel corrupted:\n{text}"
        );
        assert_eq!(
            lines[lines.len() - 1].trim_end(),
            "BOTTOM_SENTINEL",
            "squeezed_height={squeezed_height}: bottom sentinel corrupted:\n{text}"
        );

        if squeezed_height >= 1 {
            let input_rect = find_rect(&snapshot.widgets, "input");
            let btn_rect = find_rect(&snapshot.widgets, "btn");
            assert_eq!(input_rect.h, squeezed_height);
            assert_eq!(btn_rect.h, squeezed_height);
            assert!(
                lines[input_rect.y as usize].contains("hello"),
                "squeezed_height={squeezed_height}: input text should remain visible:\n{text}"
            );
            let btn_rows = &lines[btn_rect.y as usize..(btn_rect.y as usize + btn_rect.h as usize)];
            assert!(
                btn_rows.iter().any(|row| row.contains("Act")),
                "squeezed_height={squeezed_height}: outlined button label should remain visible:\n{text}"
            );
            assert_eq!(
                backend.pointer_shape_at((input_rect.x + 1) as u16, input_rect.y as u16),
                Some(PointerShape::Text),
                "squeezed_height={squeezed_height}: text pointer should match borderless input row"
            );
        }
    }
}

struct FixedView(Element);

impl Component for FixedView {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        self.0.clone()
    }
}

#[test]
fn narrow_bordered_controls_show_text_inside_their_slots() {
    for w in [1, 2] {
        let width = Length::Px(w);
        let element = HStack::new()
            .align(Align::Start)
            .child(Text::new("L"))
            .child(
                Element::from(
                    Input::new("hello")
                        .on_change(Callback::new(|_| {}))
                        .width(width)
                        .height(Length::Px(3)),
                )
                .min_width(Length::Px(0))
                .key("input"),
            )
            .child(Text::new("M"))
            .child(
                Element::from(
                    Input::new("")
                        .placeholder("Name")
                        .width(width)
                        .height(Length::Px(3)),
                )
                .min_width(Length::Px(0))
                .key("placeholder"),
            )
            .child(Text::new("N"))
            .child(
                Element::from(Button::outlined("Act").width(width).height(Length::Px(3)))
                    .min_width(Length::Px(0))
                    .key("button"),
            )
            .child(Text::new("R"));
        let mut backend = TestBackend::new_with_viewport(
            FixedView(element.into()),
            Rect {
                x: 0,
                y: 0,
                w: 3 * w + 4,
                h: 3,
            },
        );
        backend.render();

        let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
        let rows = snapshot.frame.to_fixed_grid_lines();
        for key in ["input", "placeholder", "button"] {
            let rect = find_rect(&snapshot.widgets, key);
            assert_eq!((rect.w, rect.h), (w, 3));
        }
        let n = usize::from(w);
        assert_eq!(
            rows[0],
            format!("L{}M{}N{}R", &"hello"[..n], &"Name"[..n], " ".repeat(n)),
            "{rows:?}"
        );
        assert_eq!(
            rows[1],
            format!("{}{} ", " ".repeat(2 * n + 3), &"Act"[..n])
        );
        assert_eq!(rows[2], " ".repeat(3 * n + 4));
        assert_eq!(backend.pointer_shape_at(w, 0), Some(PointerShape::Text));

        backend.focus_next();
        assert_eq!(backend.focused_key().map(|key| key.as_ref()), Some("input"));
        let focused_rows = backend.capture_ui_snapshot().frame.to_fixed_grid_lines();
        assert_eq!(&focused_rows[0][1..1 + n], &"hello"[5 - n..]);
    }
}

#[test]
fn squeezed_input_keeps_its_field_and_excludes_the_error_row_from_text_hits() {
    use std::cell::Cell;
    use std::rc::Rc;

    for (reserve_error_row, error) in [(true, None), (false, Some("Invalid value"))] {
        for h in [1, 2, 4] {
            let cursor = Rc::new(Cell::new(None));
            let context_menus = Rc::new(Cell::new(0));
            let input = Input::new("hello")
                .height(Length::Px(h))
                .focusable(false)
                .reserve_error_row(reserve_error_row)
                .error(error)
                .on_change(Callback::new({
                    let cursor = cursor.clone();
                    move |event: InputEvent| cursor.set(Some(event.cursor))
                }))
                .on_context_menu(Callback::new({
                    let context_menus = context_menus.clone();
                    move |_| context_menus.set(context_menus.get() + 1)
                }));
            let mut backend = TestBackend::new_with_viewport(
                FixedView(Element::from(input).min_height(Length::Px(0)).key("input")),
                Rect {
                    x: 0,
                    y: 0,
                    w: 24,
                    h,
                },
            );
            backend.render();

            let snapshot =
                backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
            assert_eq!(find_rect(&snapshot.widgets, "input").h, h);
            let rows = snapshot.frame.to_fixed_grid_lines();
            let (text_x, text_y) = if h == 4 { (2, 1) } else { (1, 0) };
            assert!(rows[text_y as usize].contains("hello"), "rows: {rows:?}");
            assert_eq!(
                rows.iter().any(|row| row.contains("Invalid value")),
                h > 1 && error.is_some(),
                "rows: {rows:?}"
            );
            assert_eq!(
                backend.pointer_shape_at(text_x, text_y),
                Some(PointerShape::Text)
            );

            let mut click = |y, button| {
                for kind in [MouseKind::Down(button), MouseKind::Up(button)] {
                    backend
                        .send_mouse(MouseEvent {
                            x: text_x + 1,
                            y,
                            kind,
                            mods: KeyMods::NONE,
                        })
                        .unwrap();
                }
            };
            click(text_y, MouseButton::Left);
            assert_eq!(cursor.take(), Some(1));
            click(text_y, MouseButton::Right);
            assert_eq!(context_menus.get(), 1);

            if h > 1 {
                click(h - 1, MouseButton::Left);
                assert_eq!(cursor.take(), None);
                click(h - 1, MouseButton::Right);
                assert_eq!(context_menus.get(), 1);
                assert_ne!(
                    backend.pointer_shape_at(text_x, h - 1),
                    Some(PointerShape::Text)
                );
            }
        }
    }
}

struct FullscreenOverflowVStack;

impl Component for FullscreenOverflowVStack {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .gap(1)
            .child(Input::new("first").border(true).key("input"))
            .child(
                TextArea::new("area")
                    .border(true)
                    .height(Length::Auto)
                    .key("area"),
            )
            .child(Button::outlined("Action").key("action"))
            .child(Text::new("Footer line").key("footer"))
            .into()
    }
}

#[test]
fn fullscreen_overflowed_vstack_never_overlaps_bordered_siblings() {
    for viewport_h in [2u16, 4, 6, 8] {
        let mut backend = TestBackend::new_with_viewport(
            FullscreenOverflowVStack,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: viewport_h,
            },
        );
        backend.render();

        let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
        let text = snapshot.frame.plain_text();
        assert_no_visible_sibling_overlap(
            &snapshot.widgets,
            &["input", "area", "action", "footer"],
            &text,
        );
        let footer = find_rect(&snapshot.widgets, "footer");
        assert!(
            footer.h == 1
                && snapshot.frame.to_fixed_grid_lines()[footer.y as usize].contains("Footer line"),
            "viewport_h={viewport_h}: footer should render cleanly without overlap:\n{text}"
        );
    }
}

struct FullscreenOverflowHStack;

impl Component for FullscreenOverflowHStack {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        HStack::new()
            .gap(1)
            .child(Button::filled("AlphaButton").key("b1"))
            .child(Button::filled("BetaButton").key("b2"))
            .child(Button::filled("Gamma").key("b3"))
            .key("row")
    }
}

#[test]
fn overflowed_hstack_siblings_never_overlap() {
    let mut backend = TestBackend::new_with_viewport(
        FullscreenOverflowHStack,
        Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 3,
        },
    );
    backend.render();

    let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::diagnostic());
    let text = snapshot.frame.plain_text();
    assert_no_visible_sibling_overlap(&snapshot.widgets, &["b1", "b2", "b3"], &text);
}
