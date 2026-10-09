//! Drag-target cost with a large unrelated subtree, with and without panel-body opt-in.
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct DragTargets {
    unrelated_nodes: usize,
    body_target: bool,
}

impl Component for DragTargets {
    type State = ();
    type Message = ();
    type Properties = ();

    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let mut bar = DraggableTabBar::new()
            .tabs([DraggableTab::new("Editor")])
            .height(Length::Px(1))
            .show_close_buttons(false)
            .drag_preview(false)
            .bar_id("editor")
            .drag_group("editors")
            .on_transfer(Callback::new(|_| {}));
        if self.body_target {
            bar = bar.drop_area("panel");
        }
        let panel = VStack::new()
            .width(Length::Px(40))
            .child(bar)
            .child(Spacer::new())
            .key("panel");
        let mut unrelated = VStack::new().width(Length::Px(80));
        for _ in 0..self.unrelated_nodes {
            unrelated = unrelated.child(Text::new("Unrelated row").height(Length::Px(1)));
        }
        HStack::new().child(panel).child(unrelated).into()
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

fn bench_targets(c: &mut Criterion) {
    let mut group = c.benchmark_group("tab_drop_target");
    for body_target in [false, true] {
        for nodes in [0, 1_000, 10_000] {
            let mut b = TestBackend::new(DragTargets {
                unrelated_nodes: nodes,
                body_target,
            });
            b.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 120,
                h: 20,
            });
            b.render();
            b.send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
                .unwrap();
            b.send_mouse(mouse(2, 5, MouseKind::Drag(MouseButton::Left)))
                .unwrap();
            // A no-op drag exercises target lookup without rebuilding the unrelated subtree.
            assert!(
                !b.send_mouse(mouse(2, 5, MouseKind::Drag(MouseButton::Left)))
                    .unwrap()
            );
            group.bench_with_input(
                BenchmarkId::new(if body_target { "body" } else { "bar_only" }, nodes),
                &nodes,
                |bench, _| {
                    bench.iter(|| {
                        black_box(
                            b.send_mouse(mouse(2, 5, MouseKind::Drag(MouseButton::Left)))
                                .unwrap(),
                        );
                    })
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_targets);
criterion_main!(benches);
