use tui_lipan::prelude::*;

const THEMES: &[&str] = &["Minimal", "Balanced", "Loud"];
const TARGETS: &[&str] = &[
    "Local sandbox",
    "Staging cluster",
    "Production canary",
    "Production full",
];

struct InlineChoices;

#[derive(Default)]
struct State {
    theme: usize,
    target: Option<usize>,
    target_expanded: bool,
    note: TextInput,
    applied: u32,
    recent: Vec<String>,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    ThemeChanged(usize),
    TargetToggle(bool),
    TargetSelected(usize),
    TargetChanged(usize),
    NoteChanged(InputEvent),
    Apply,
}

impl Component for InlineChoices {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            theme: 1,
            status: "Pick values, then apply to insert a summary above".to_string(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ThemeChanged(index) => {
                ctx.state.theme = index.min(THEMES.len().saturating_sub(1));
                Update::full()
            }
            Msg::TargetToggle(expanded) => {
                ctx.state.target_expanded = expanded;
                Update::full()
            }
            Msg::TargetSelected(index) | Msg::TargetChanged(index) => {
                ctx.state.target = Some(index.min(TARGETS.len().saturating_sub(1)));
                ctx.state.target_expanded = false;
                Update::full()
            }
            Msg::NoteChanged(event) => {
                ctx.state.note.set_text(event.value.as_ref());
                ctx.state.note.set_cursor_keep_anchor(event.cursor);
                ctx.state.note.set_anchor(event.anchor);
                Update::full()
            }
            Msg::Apply => {
                let Some(target_idx) = ctx.state.target else {
                    ctx.state.status = "Pick a deployment target first".to_string();
                    return Update::full();
                };

                let next = ctx.state.applied.saturating_add(1);
                let theme = THEMES.get(ctx.state.theme).copied().unwrap_or(THEMES[0]);
                let target = TARGETS.get(target_idx).copied().unwrap_or(TARGETS[0]);
                let note = ctx.state.note.text().trim();

                let note_text = if note.is_empty() { "(none)" } else { note };
                ctx.state.recent.push(format!(
                    "[{next:03}] deploy target={target} theme={theme} note={note_text}"
                ));

                ctx.state.applied = next;
                ctx.state.status = format!("Applied target '{target}' with theme '{theme}'");
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                ctx.toggle_mouse_capture();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Inline choices")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status(format!(
                "Enter applies from note input | m toggles mouse ({}) | q/Esc quits",
                if ctx.mouse_capture_enabled() {
                    "on"
                } else {
                    "off"
                }
            ))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Theme"))
                    .child(
                        Radio::new(THEMES.iter().copied())
                            .selected(Some(ctx.state.theme))
                            .layout(RadioLayout::Horizontal)
                            .gap(2)
                            .checked_style(Style::new().fg(Color::LightGreen).bold())
                            .unchecked_style(Style::new().fg(Color::DarkGray))
                            .on_change(ctx.link().callback(Msg::ThemeChanged)),
                    )
                    .child(Text::new("Deployment target"))
                    .child(
                        Select::new()
                            .options(TARGETS.iter().copied())
                            .selected(ctx.state.target)
                            .placeholder("Choose target")
                            .expanded(ctx.state.target_expanded)
                            .list_height(Length::Px(4))
                            .list_scrollbar(true)
                            .list_scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            )
                            .on_toggle(ctx.link().callback(Msg::TargetToggle))
                            .on_select(ctx.link().callback(Msg::TargetSelected))
                            .on_change(ctx.link().callback(Msg::TargetChanged))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Input::new(ctx.state.note.text().to_string())
                            .cursor(ctx.state.note.cursor())
                            .anchor(ctx.state.note.anchor())
                            .placeholder("Optional note")
                            .on_change(ctx.link().callback(Msg::NoteChanged))
                            .on_key(ctx.link().key_handler(|key| match key.code {
                                KeyCode::Enter => Some(Msg::Apply),
                                _ => None,
                            })),
                    )
                    .child(
                        Button::filled("Apply")
                            .on_click(ctx.link().callback(|_| Msg::Apply))
                            .focusable(true),
                    )
                    .child(Text::new(format!(
                        "Applied: {} | {}",
                        ctx.state.applied, ctx.state.status
                    )))
                    .child(
                        Text::new(
                            ctx.state
                                .recent
                                .iter()
                                .rev()
                                .take(4)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                        .overflow(Overflow::Wrap),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .inline_ephemeral(13)
        .mouse(true)
        .mount(InlineChoices)
        .run()
}
