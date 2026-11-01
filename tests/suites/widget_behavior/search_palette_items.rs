use std::cell::RefCell;
use std::rc::Rc;

use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

struct Palette(SearchPalette<usize>);

impl Component for Palette {
    type Message = Vec<SearchItem<usize>>;
    type Properties = ();
    type State = Option<Vec<SearchItem<usize>>>;

    fn create_state(&self, _: &()) -> Self::State {
        None
    }

    fn update(&mut self, items: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = Some(items);
        Update::layout()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut palette = self.0.clone();
        if let Some(items) = &ctx.state {
            palette = palette.items(items.clone());
        }
        palette.into()
    }
}

fn backend(palette: SearchPalette<usize>) -> TestBackend<Palette> {
    let mut backend = TestBackend::new(Palette(palette));
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 10,
    });
    backend.render();
    backend
}

fn key(backend: &mut TestBackend<Palette>, code: KeyCode) {
    backend
        .send_key(KeyEvent {
            code,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
}

fn find(frame: &CapturedFrame, symbol: &str) -> (u16, u16) {
    (0..10)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .find(|&(x, y)| frame.cell(x, y).symbol == symbol)
        .expect("visible symbol")
}

#[test]
fn item_colours_and_match_highlights_survive_selection_on_active_multiline_row() {
    let mut backend = backend(
        SearchPalette::new()
            .items([SearchItem::new("é猫\nsecond\n", 0)
                .active(true)
                .label_style(Style::new().fg(Color::Green))
                .right_style(Style::new().fg(Color::Cyan))
                .description(ItemDescription::new().right("hint"))])
            .query("s")
            .match_style(Style::new().fg(Color::Yellow).bold())
            .list_selection_style(Style::new().fg(Color::White).bg(Color::Blue))
            .list_active_style(Style::new().bg(Color::Red)),
    );
    backend.focus_next();
    backend.render();
    let frame = backend.capture_frame();
    let (x, y) = find(&frame, "é");
    assert_eq!(frame.cell(x, y).fg, Color::Green);
    assert_eq!(frame.cell(x, y).bg, Color::Blue);
    let (sx, sy) = find(&frame, "s");
    assert_eq!(sy, y + 1);
    assert_eq!(frame.cell(sx, sy).fg, Color::Yellow);
    assert!(frame.cell(sx, sy).modifiers.bold);
    let (hx, hy) = find(&frame, "h");
    assert_eq!(hy, y);
    assert_eq!(frame.cell(hx, hy).fg, Color::Cyan);
    assert_eq!(frame.cell(hx, hy).bg, Color::Blue);
}

#[test]
fn disabled_rows_allow_inspection_but_block_keyboard_and_mouse_activation() {
    let activations = Rc::new(RefCell::new(Vec::new()));
    let seen = activations.clone();
    let mut backend = backend(
        SearchPalette::new()
            .items([
                SearchItem::new("disabled", 0).disabled(true),
                SearchItem::new("enabled", 1),
            ])
            .query("")
            .on_activate(Callback::new(move |event: SearchEvent<usize>| {
                seen.borrow_mut().push(event.item.value)
            })),
    );
    backend.focus_next();
    key(&mut backend, KeyCode::Enter);
    assert!(activations.borrow().is_empty());
    let frame = backend.capture_frame();
    let (x, y) = find(&frame, "d");
    assert!(frame.cell(x, y).modifiers.dim);
    for _ in 0..2 {
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x,
                    y,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
    }
    assert!(activations.borrow().is_empty());
    key(&mut backend, KeyCode::Down);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(*activations.borrow(), [1]);
    let frame = backend.capture_frame();
    let (x, disabled_y) = find(&frame, "d");
    let y = disabled_y + 1;
    // Positive control: the same mouse sequence activates an enabled result.
    for _ in 0..2 {
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x,
                    y,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
    }
    assert_eq!(*activations.borrow(), [1, 1]);
}

#[test]
fn external_results_ignore_query_priority_and_cap_and_refresh_with_input_intact() {
    let activations = Rc::new(RefCell::new(Vec::new()));
    let queries = Rc::new(RefCell::new(Vec::new()));
    let seen = activations.clone();
    let query_seen = queries.clone();
    let mut backend = backend(
        SearchPalette::new()
            .items([
                SearchItem::new("first", 0),
                SearchItem::new("second", 1).priority(99),
            ])
            .match_mode(SearchMatchMode::External)
            .sync_match_limit(1)
            .initial_query("unrelated")
            .on_query_change(Callback::new(move |query| {
                query_seen.borrow_mut().push(query)
            }))
            .on_activate(Callback::new(move |event: SearchEvent<usize>| {
                seen.borrow_mut().push(event.item.value)
            })),
    );
    backend.focus_next();
    key(&mut backend, KeyCode::Enter);
    key(&mut backend, KeyCode::Down);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(*activations.borrow(), [0, 1]);
    key(&mut backend, KeyCode::Char('x'));
    assert_eq!(
        queries.borrow().last().map(|s| s.as_ref()),
        Some("unrelatedx")
    );
    backend
        .dispatch(vec![
            SearchItem::new("replacement", 2),
            SearchItem::new("second", 1),
        ])
        .unwrap();
    key(&mut backend, KeyCode::End);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(*activations.borrow(), [0, 1, 1]);
    assert!(backend.capture_frame().plain_text().contains("unrelatedx"));
    backend.dispatch(Vec::new()).unwrap();
    key(&mut backend, KeyCode::Enter);
    assert_eq!(*activations.borrow(), [0, 1, 1]);
}

#[test]
fn multiline_labels_keep_blank_lines_and_description_placement_at_narrow_width() {
    for placement in [
        DescriptionPlacement::Inline,
        DescriptionPlacement::Right,
        DescriptionPlacement::Above,
        DescriptionPlacement::Below,
    ] {
        let mut backend = backend(
            SearchPalette::new()
                .items([SearchItem::new("alpha\n\n猫 beta\n", 0).description("details")])
                .description_placement(placement)
                .query("")
                .width(Length::Px(12)),
        );
        backend.render();
        let frame = backend.capture_frame();
        let (_, first) = find(&frame, "p");
        let (_, second) = find(&frame, "猫");
        assert_eq!(second, first + 2, "{placement:?}");
        if placement == DescriptionPlacement::Below {
            let (_, description) = find(&frame, "d");
            assert_eq!(description, first + 4);
        }
        if placement == DescriptionPlacement::Above {
            let (_, description) = find(&frame, "d");
            assert_eq!(description + 1, first);
        }
        assert!(!frame.plain_text().contains("alphadetails"));
    }
}

#[test]
fn disabled_custom_renderer_is_muted_and_cannot_activate() {
    use std::sync::Arc;
    let activations = Rc::new(RefCell::new(Vec::new()));
    let seen = activations.clone();
    let mut backend = backend(
        SearchPalette::new()
            .items([SearchItem::new("label", 0).disabled(true)])
            .query("")
            .render_item(Arc::new(|_, _| {
                Some(ListItem::new("custom").style(Style::new().fg(Color::Red)))
            }))
            .on_activate(Callback::new(move |event: SearchEvent<usize>| {
                seen.borrow_mut().push(event.item.value)
            })),
    );
    backend.focus_next();
    key(&mut backend, KeyCode::Enter);
    let frame = backend.capture_frame();
    let (x, y) = find(&frame, "c");
    assert!(frame.cell(x, y).modifiers.dim);
    assert_ne!(frame.cell(x, y).fg, Color::Red);
    assert!(activations.borrow().is_empty());
}

fn coloured_spinner() -> Spinner {
    Spinner::new()
        .spinner_style(SpinnerStyle::Line)
        .frame(0)
        .label("S")
        .style(Style::new().fg(Color::Red))
        .label_style(Style::new().fg(Color::Red))
}

#[test]
fn disabled_rows_mute_explicit_status_gutter_and_spinner_styles() {
    use std::sync::Arc;
    for selected in [false, true] {
        for spinner_slots in [false, true] {
            for placement in [
                DescriptionPlacement::Inline,
                DescriptionPlacement::Right,
                DescriptionPlacement::Above,
                DescriptionPlacement::Below,
            ] {
                let mut palette = SearchPalette::new()
                    .items([
                        SearchItem::new("label", 0).disabled(true).description(
                            ItemDescription::new()
                                .left("details")
                                .spinner(coloured_spinner()),
                        ),
                        SearchItem::new("other", 1),
                    ])
                    .initial_selected_item_index(Some(usize::from(!selected)))
                    .query("")
                    .description_placement(placement)
                    .list_selection_style(Style::new().fg(Color::Yellow).bg(Color::Blue));
                if spinner_slots {
                    palette = palette
                        .item_status(Arc::new(|item, _| {
                            item.disabled
                                .then(|| ListItemStatus::spinner(coloured_spinner()))
                        }))
                        .item_gutter(Arc::new(|item, _| {
                            item.disabled
                                .then(|| ListItemGutter::spinner(coloured_spinner()))
                        }));
                } else {
                    palette = palette
                        .item_status(Arc::new(|item, _| {
                            item.disabled.then(|| {
                                ListItemStatus::from_spans([Span::new("●").fg(Color::Red)])
                            })
                        }))
                        .item_gutter(Arc::new(|item, _| {
                            item.disabled.then(|| {
                                ListItemGutter::from_spans([Span::new("G").fg(Color::Red)])
                            })
                        }));
                }
                let mut backend = backend(palette);
                backend.focus_next();
                backend.render();
                let frame = backend.capture_frame();
                let (x, y) = find(&frame, "l");
                let muted = frame.cell(x, y).fg;
                for symbol in if spinner_slots {
                    vec!["|", "S"]
                } else {
                    vec!["●", "G", "|", "S"]
                } {
                    let mut count = 0;
                    for y in 0..10 {
                        for x in 0..32 {
                            let cell = frame.cell(x, y);
                            if cell.symbol == symbol {
                                assert_eq!(cell.fg, muted, "{placement:?} {symbol}");
                                assert!(cell.modifiers.dim, "{placement:?} {symbol}");
                                count += 1;
                            }
                        }
                    }
                    assert!(count > 0, "{placement:?} missing {symbol}");
                }
            }
        }
    }
}

#[test]
fn disabled_custom_renderer_mutes_primary_and_extra_line_spinners() {
    use std::sync::Arc;
    let mut backend = backend(
        SearchPalette::new()
            .items([SearchItem::new("label", 0).disabled(true)])
            .query("")
            .list_selection_style(Style::new().fg(Color::Yellow).bg(Color::Blue))
            .render_item(Arc::new(|_, _| {
                Some(
                    ListItem::new("row")
                        .description("hint")
                        .label_spinner(coloured_spinner())
                        .description_spinner(coloured_spinner())
                        .line(
                            ListItemLine::new("extra")
                                .description("info")
                                .label_spinner(coloured_spinner())
                                .description_spinner(coloured_spinner()),
                        ),
                )
            })),
    );
    backend.focus_next();
    backend.render();
    let frame = backend.capture_frame();
    let (x, y) = find(&frame, "r");
    let muted = frame.cell(x, y).fg;
    for symbol in ["|", "S"] {
        let mut count = 0;
        for y in 0..10 {
            for x in 0..32 {
                let cell = frame.cell(x, y);
                if cell.symbol == symbol {
                    assert_eq!(cell.fg, muted);
                    assert!(cell.modifiers.dim);
                    count += 1;
                }
            }
        }
        assert_eq!(
            count, 4,
            "every primary/extra-line spinner must render {symbol}"
        );
    }
}
