use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

#[derive(Clone)]
enum Msg {
    Seed,
    SetLive(&'static str),
}

struct TranscriptProbe;

impl Component for TranscriptProbe {
    type Message = Msg;
    type Properties = ();
    type State = String;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        "initial".to_string()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Seed => {
                ctx.append_transcript_lines(["line-a", "line-b"]);
                ctx.append_transcript_element(Text::new("elem-1"));
            }
            Msg::SetLive(label) => {
                ctx.state.clear();
                ctx.state.push_str(label);
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Text::new(format!("live:{}", ctx.state)).into()
    }
}

#[test]
fn transcript_replay_document_orders_history_before_live_viewport() {
    let mut backend = TestBackend::new_transcript(TranscriptProbe);
    backend
        .dispatch(Msg::Seed)
        .expect("seed transcript history");
    backend
        .dispatch(Msg::SetLive("after-update"))
        .expect("update live viewport state");

    assert_eq!(backend.transcript_history_len(), 2);
    assert_eq!(
        backend.transcript_replay_summary(false),
        vec!["lines:line-a|line-b", "element:elem-1"]
    );
    assert_eq!(
        backend.transcript_replay_summary(true),
        vec![
            "lines:line-a|line-b".to_string(),
            "element:elem-1".to_string(),
            "element:live:after-update".to_string(),
        ]
    );
}

#[test]
fn transcript_history_snapshot_is_stable_across_headless_viewport_changes() {
    let mut backend = TestBackend::new_transcript(TranscriptProbe);
    backend
        .dispatch(Msg::Seed)
        .expect("seed transcript history");

    let baseline = backend.transcript_replay_summary(false);
    for viewport in [
        tui_lipan::Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        },
        tui_lipan::Rect {
            x: 0,
            y: 0,
            w: 48,
            h: 12,
        },
        tui_lipan::Rect {
            x: 0,
            y: 0,
            w: 120,
            h: 32,
        },
    ] {
        backend.set_viewport(viewport);
        backend.render();
    }

    assert_eq!(backend.transcript_history_len(), 2);
    assert_eq!(backend.transcript_replay_summary(false), baseline);
}
