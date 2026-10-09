use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

#[derive(Default)]
struct EmptyTransfer {
    body_targets: bool,
    separate_groups: bool,
    source_only_callback: bool,
    disabled_destination: bool,
    layered_body: bool,
    mode: DragReorderMode,
}

#[derive(Default)]
struct State {
    left: Vec<String>,
    right: Vec<String>,
    selected: Option<usize>,
    epoch: usize,
}

#[derive(Clone)]
enum Msg {
    Transfer(Option<usize>, DraggableTabTransferEvent),
    Select(TabsEvent),
}

impl Component for EmptyTransfer {
    type State = State;
    type Message = Msg;
    type Properties = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            left: vec!["Agents".to_string()],
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Transfer(epoch, event) => {
                if epoch.is_some_and(|epoch| epoch != ctx.state.epoch) {
                    return Update::none();
                }
                let state = &mut ctx.state;
                let (from, to) = if event.from_bar.as_ref() == "top" {
                    (&mut state.left, &mut state.right)
                } else {
                    (&mut state.right, &mut state.left)
                };
                let tab = from.remove(event.from);
                to.insert(event.to, tab);
                state.epoch += 1;
            }
            Msg::Select(event) => ctx.state.selected = Some(event.index),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let epoch = (!self.source_only_callback).then_some(ctx.state.epoch);
        let bar = |id: &'static str, tabs: &[String]| -> Element {
            let mut bar = DraggableTabBar::new()
                .tabs(tabs.iter().map(|tab| {
                    if tab == "+" {
                        DraggableTab::action(tab.as_str())
                    } else {
                        DraggableTab::new(tab.as_str())
                    }
                }))
                .disabled(self.disabled_destination && id == "bottom")
                .bar_id(id)
                .drag_group(if self.separate_groups && id == "bottom" {
                    "other"
                } else {
                    "sidebar"
                })
                .reorder_mode(self.mode)
                .height(Length::Px(1))
                .on_change(ctx.link().callback(Msg::Select));
            if !self.source_only_callback || id == "top" {
                bar = bar.on_transfer(
                    ctx.link()
                        .callback(move |event| Msg::Transfer(epoch, event)),
                );
            }
            if self.body_targets {
                bar = bar.drop_area(id);
                let body = Text::new("Body").height(Length::Flex(1));
                if self.layered_body {
                    VStack::new()
                        .height(Length::Px(5))
                        .child(
                            ZStack::new()
                                .passthrough(false)
                                .child(VStack::new().child(bar).child(body))
                                .key(id),
                        )
                        .into()
                } else {
                    VStack::new()
                        .height(Length::Px(5))
                        .child(bar)
                        .child(body)
                        .key(id)
                }
            } else {
                bar.into()
            }
        };
        VStack::new()
            .child(bar("top", &ctx.state.left))
            .child(Spacer::new().height(Length::Px(1)))
            .child(bar("bottom", &ctx.state.right))
            .into()
    }
}

fn mouse(x: u16, y: u16, kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind,
        mods: KeyMods::NONE,
    }
}

#[test]
fn live_drag_transfers_into_an_empty_grouped_bar() {
    let mut backend = TestBackend::new(EmptyTransfer::default());
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();

    assert!(
        backend
            .send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    assert!(
        backend
            .send_mouse(mouse(2, 2, MouseKind::Drag(MouseButton::Left)))
            .unwrap()
    );

    assert!(
        backend.state().left.is_empty(),
        "{}",
        backend.capture_ui_snapshot().to_markdown()
    );
    assert_eq!(backend.state().right, ["Agents"]);
    assert_eq!(backend.state().selected, Some(0));
}

#[test]
fn continuous_live_transfers_refresh_source_callbacks_after_every_render() {
    let mut b = TestBackend::new(EmptyTransfer::default());
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 3,
    });
    b.state_mut().left = vec!["Agents".into(), "Panes".into(), "Sessions".into()];
    b.render();
    let row = b.capture_frame().to_fixed_grid_lines().remove(0);
    let x = row.find("Panes").unwrap() as u16;
    b.send_mouse(mouse(x, 0, MouseKind::Down(MouseButton::Left)))
        .unwrap();
    for y in [2, 0, 2, 0] {
        b.send_mouse(mouse(2, y, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        b.render();
        let state = b.state();
        if y == 2 {
            assert_eq!(state.left, ["Agents", "Sessions"]);
            assert_eq!(state.right, ["Panes"]);
        } else {
            assert!(state.left.iter().any(|tab| tab == "Panes"));
            assert!(state.right.is_empty());
        }
    }
    assert_eq!(b.state().epoch, 4);
}

#[test]
fn panel_bodies_accept_transfers_in_both_reorder_modes() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        let mut b = TestBackend::new(EmptyTransfer {
            body_targets: true,
            mode,
            ..Default::default()
        });
        b.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 11,
        });
        b.state_mut().right = vec!["Existing".into()];
        b.render();
        b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap();
        b.send_mouse(mouse(1, 9, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        if mode == DragReorderMode::OnDrop {
            assert_eq!(b.state().left, ["Agents"]);
        }
        b.send_mouse(mouse(1, 9, MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert!(b.state().left.is_empty());
        assert_eq!(b.state().right, ["Existing", "Agents"]);
        assert_eq!(b.state().selected, Some(1));
    }
}

#[test]
fn body_transfer_can_return_to_the_now_empty_source_without_releasing() {
    let mut b = TestBackend::new(EmptyTransfer {
        body_targets: true,
        ..Default::default()
    });
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 11,
    });
    b.render();
    b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
        .unwrap();
    for y in [9, 3, 9, 3] {
        b.send_mouse(mouse(1, y, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        b.render();
        assert_eq!(b.state().left.is_empty(), y == 9);
        assert_eq!(b.state().right.is_empty(), y == 3);
    }
    assert_eq!(b.state().epoch, 4);
}

#[test]
fn body_targets_preserve_drag_groups_and_returning_home_cancels_on_drop() {
    for separate_groups in [false, true] {
        let mut b = TestBackend::new(EmptyTransfer {
            body_targets: true,
            separate_groups,
            mode: DragReorderMode::OnDrop,
            ..Default::default()
        });
        b.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 11,
        });
        b.render();
        b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap();
        b.send_mouse(mouse(1, 9, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        if !separate_groups {
            b.send_mouse(mouse(1, 3, MouseKind::Drag(MouseButton::Left)))
                .unwrap();
        }
        b.send_mouse(mouse(
            1,
            if separate_groups { 9 } else { 3 },
            MouseKind::Up(MouseButton::Left),
        ))
        .unwrap();
        assert_eq!(b.state().left, ["Agents"]);
        assert!(b.state().right.is_empty());
        assert_eq!(b.state().epoch, 0);
    }
}

#[test]
fn a_destination_without_a_transfer_callback_keeps_the_shared_source_handler() {
    let mut b = TestBackend::new(EmptyTransfer {
        source_only_callback: true,
        ..Default::default()
    });
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 3,
    });
    b.render();
    b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
        .unwrap();
    for y in [2, 0] {
        b.send_mouse(mouse(2, y, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        b.render();
    }
    assert_eq!(b.state().left, ["Agents"]);
    assert!(b.state().right.is_empty());
    assert_eq!(b.state().epoch, 2);
}

#[test]
fn disabled_destinations_reject_body_and_bar_transfers() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        for y in [6, 9] {
            let mut b = TestBackend::new(EmptyTransfer {
                body_targets: true,
                disabled_destination: true,
                mode,
                ..Default::default()
            });
            b.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 11,
            });
            b.state_mut().right = vec!["Existing".into()];
            b.render();
            b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
                .unwrap();
            b.send_mouse(mouse(1, y, MouseKind::Drag(MouseButton::Left)))
                .unwrap();
            b.send_mouse(mouse(1, y, MouseKind::Up(MouseButton::Left)))
                .unwrap();
            assert_eq!(b.state().left, ["Agents"]);
            assert_eq!(b.state().right, ["Existing"]);
            assert_eq!(b.state().epoch, 0);
        }
    }
}

#[test]
fn body_append_inserts_before_trailing_actions_including_action_only_bars() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        for (existing, insertion) in [
            (vec!["Editor".to_string(), "+".to_string()], 1),
            (vec!["+".to_string()], 0),
            (
                vec!["Editor".to_string(), "+".to_string(), "+".to_string()],
                1,
            ),
            (vec!["+".to_string(), "+".to_string()], 0),
        ] {
            let mut b = TestBackend::new(EmptyTransfer {
                body_targets: true,
                mode,
                ..Default::default()
            });
            b.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 11,
            });
            b.state_mut().right = existing.clone();
            b.render();
            b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
                .unwrap();
            b.send_mouse(mouse(1, 9, MouseKind::Drag(MouseButton::Left)))
                .unwrap();
            b.send_mouse(mouse(1, 9, MouseKind::Up(MouseButton::Left)))
                .unwrap();
            let mut expected = existing;
            expected.insert(insertion, "Agents".into());
            assert!(b.state().left.is_empty());
            assert_eq!(b.state().right, expected);
            assert_eq!(b.state().selected, Some(insertion));
        }
    }
}

#[test]
fn non_passthrough_layered_panel_accepts_body_transfers_over_paint_only_children() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        let mut b = TestBackend::new(EmptyTransfer {
            body_targets: true,
            layered_body: true,
            mode,
            ..Default::default()
        });
        b.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 11,
        });
        b.render();
        b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap();
        b.send_mouse(mouse(1, 9, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        b.send_mouse(mouse(1, 9, MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert!(b.state().left.is_empty());
        assert_eq!(b.state().right, ["Agents"]);
    }
}
