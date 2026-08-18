use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct AsciiSequenceDiagram;

impl Component for AsciiSequenceDiagram {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        SequenceDiagram::new()
            .participant("Client")
            .participant("API")
            .message(SequenceMessage::sync("Client", "API", "request"))
            .message(SequenceMessage::reply("API", "Client", "response"))
            .step(Step::note_over(["Client", "API"], "plain note"))
            .step(Step::fragment_begin(FragmentKind::Alt, "ok"))
            .message(SequenceMessage::lost("API", "Client", "timeout"))
            .step(Step::fragment_end())
            .theme(SequenceDiagramTheme::ascii())
            .autonumber(true)
            .padding(1)
            .into()
    }
}

#[test]
fn ascii_sequence_diagram_theme_renders_ascii_glyphs() {
    let mut backend = TestBackend::new(AsciiSequenceDiagram);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 64,
        h: 24,
    });
    backend.render();

    let plain = backend.capture_frame().plain_text();
    assert!(plain.is_ascii());
    assert!(plain.contains("+"));
    assert!(plain.contains(">"));
    assert!(plain.contains("x"));
}
