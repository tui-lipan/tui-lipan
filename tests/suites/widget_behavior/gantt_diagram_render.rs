use tui_lipan::prelude::*;
use tui_lipan::{GanttDiagram, GanttSection, GanttTask, TestBackend};

struct SampleGanttDiagram;

impl Component for SampleGanttDiagram {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        GanttDiagram::new()
            .title("Sample Schedule")
            .section(
                GanttSection::new("Build")
                    .task(
                        GanttTask::new("Design")
                            .id("a1")
                            .start_date("2026-05-01")
                            .duration_days(3)
                            .done(),
                    )
                    .task(
                        GanttTask::new("Implement")
                            .id("a2")
                            .after("a1")
                            .duration_days(4)
                            .active(),
                    )
                    .task(GanttTask::new("Release").after("a2").milestone()),
            )
            .max_timeline_width(24)
            .padding(1)
            .into()
    }
}

#[test]
fn gantt_diagram_renders_labels_and_bars() {
    let mut backend = TestBackend::new(SampleGanttDiagram);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 16,
    });
    backend.render();

    let plain = backend.capture_frame().plain_text();
    assert!(plain.contains("Sample Schedule"));
    assert!(plain.contains("Build"));
    assert!(plain.contains("Design"));
    assert!(plain.contains("Implement"));
    assert!(plain.contains('█'));
    assert!(plain.contains('◆'));
}
