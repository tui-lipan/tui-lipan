use std::io::IsTerminal;

use crossterm::terminal::is_raw_mode_enabled;
use tui_lipan::terminal_handoff::{resume_after_external_process, suspend_for_external_process};
use tui_lipan::{App, InlineHeight, InlineStartupPolicy, SurfaceMode};

fn mount_smoke_app(mode: SurfaceMode) {
    struct Smoke;

    impl tui_lipan::Component for Smoke {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(
            &mut self,
            _msg: Self::Message,
            _ctx: &mut tui_lipan::Context<Self>,
        ) -> tui_lipan::Update {
            tui_lipan::Update::none()
        }

        fn view(&self, _ctx: &tui_lipan::Context<Self>) -> tui_lipan::Element {
            tui_lipan::prelude::Text::new("smoke").into()
        }
    }

    let _runner = App::new().surface(mode).mount(Smoke);
}

#[test]
fn suspend_resume_restores_surface_state() {
    let _suspend_sig: fn(SurfaceMode) -> std::io::Result<()> = suspend_for_external_process;
    let _resume_sig: fn(SurfaceMode, bool) -> std::io::Result<()> = resume_after_external_process;

    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        mount_smoke_app(SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(4) });
        mount_smoke_app(SurfaceMode::Fullscreen);
        return;
    }

    let baseline_raw = is_raw_mode_enabled().unwrap_or(false);

    suspend_for_external_process(SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(4) })
        .expect("suspend inline surface");
    assert!(!is_raw_mode_enabled().unwrap_or(false));

    resume_after_external_process(SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(4) }, true)
        .expect("resume inline surface");
    assert!(is_raw_mode_enabled().unwrap_or(false));

    suspend_for_external_process(SurfaceMode::Fullscreen).expect("suspend fullscreen surface");
    assert!(!is_raw_mode_enabled().unwrap_or(false));

    resume_after_external_process(SurfaceMode::Fullscreen, true)
        .expect("resume fullscreen surface");
    assert!(is_raw_mode_enabled().unwrap_or(false));

    if !baseline_raw {
        crossterm::terminal::disable_raw_mode().expect("restore baseline raw mode");
    }
}

#[test]
fn transcript_startup_policy_defaults_off() {
    assert_eq!(
        InlineStartupPolicy::default(),
        InlineStartupPolicy::PreserveHost
    );

    let transcript_mode = SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(6),
        startup: InlineStartupPolicy::default(),
    };
    assert_eq!(
        transcript_mode,
        SurfaceMode::InlineTranscript {
            height: InlineHeight::Fixed(6),
            startup: InlineStartupPolicy::PreserveHost,
        }
    );

    mount_smoke_app(SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(6),
        startup: InlineStartupPolicy::PreserveHost,
    });
}
