//! A rich toast message styles each span over the toast's message style.
use std::time::Duration;
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const TEXT: Color = Color::Rgb(220, 80, 80);
const NAME: Color = Color::Rgb(80, 160, 220);

struct Host {
    toast: Toast,
}

impl Component for Host {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), ctx: &mut Context<Self>) -> Update {
        ctx.toast().push(self.toast.clone());
        Update::full()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        Text::new("Background").into()
    }
}

#[test]
fn each_span_keeps_the_message_style_it_does_not_override() {
    let toast = Toast::rich(vec![
        Span::new("editor "),
        Span::new("vim").style(Style::new().fg(NAME).bold()),
        Span::new(" is missing"),
    ])
    .message_style(Style::new().fg(TEXT))
    .duration(10.0);
    let mut backend = TestBackend::new(Host { toast });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();
    backend.dispatch(()).unwrap();
    backend.advance(Duration::from_millis(500));
    let frame = backend.capture_frame();

    let lines = frame.to_fixed_grid_lines();
    let y = lines
        .iter()
        .position(|line| line.contains("editor vim is missing"))
        .unwrap_or_else(|| panic!("the spans render as one line: {lines:#?}"));
    let runs = frame.row_runs(y as u16);
    let run = |text: &str| {
        runs.iter()
            .find(|run| run.text.contains(text))
            .unwrap_or_else(|| panic!("no run holds {text:?}: {runs:#?}"))
    };

    let name = run("vim");
    assert_eq!(name.fg, NAME);
    assert!(name.modifiers.bold);
    for plain in ["editor", "is missing"] {
        assert_eq!(run(plain).fg, TEXT, "{plain}");
        assert!(!run(plain).modifiers.bold, "{plain}");
    }
}
