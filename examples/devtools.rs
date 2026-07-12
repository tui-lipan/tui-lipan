use std::time::Duration;

use tui_lipan::prelude::*;

struct DevtoolsDemo;

#[derive(Default)]
struct State {
    tick: u64,
}

#[derive(Clone)]
enum Msg {
    Tick,
    Quit,
}

impl Component for DevtoolsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        tui_lipan::debug_log!("[devtools] app init");
        Some(schedule_tick())
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.link().send(Msg::Quit);
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                tui_lipan::debug_log!("[devtools] manual log at tick={}", ctx.state.tick);
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Tick => {
                ctx.state.tick = ctx.state.tick.saturating_add(1);

                if ctx.state.tick.is_multiple_of(2) {
                    tui_lipan::debug_log!(
                        "[devtools] tick={} phase={}",
                        ctx.state.tick,
                        phase_label(ctx.state.tick)
                    );
                }

                Update::with_command(schedule_tick())
            }
            Msg::Quit => {
                tui_lipan::debug_log!("[devtools] quit requested");
                ctx.quit();
                Update::none()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let phase = phase_label(ctx.state.tick);
        let pulse = "*".repeat((ctx.state.tick % 20) as usize + 1);

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Frame::new()
                    .title("Devtools Demo")
                    .border(true)
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Press F12 to toggle DevTools overlay"))
                            .child(Text::new("Press L to emit a manual debug log"))
                            .child(Text::new("Press Q or Esc to quit")),
                    ),
            )
            .child(
                Frame::new()
                    .title("Live State")
                    .border(true)
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new(format!("Tick: {}", ctx.state.tick)))
                            .child(Text::new(format!("Phase: {phase}")))
                            .child(Text::new(format!("Pulse: {pulse}"))),
                    ),
            )
            .into()
    }
}

fn phase_label(tick: u64) -> &'static str {
    match tick % 4 {
        0 => "collect-input",
        1 => "run-update",
        2 => "reconcile-tree",
        _ => "render-frame",
    }
}

fn schedule_tick() -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(Duration::from_millis(350));
        link.send(Msg::Tick);
    })
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Devtools Demo")
        .mount(DevtoolsDemo)
        .run()
}
