use std::time::Duration;

use tui_lipan::prelude::*;

const MESSAGE_TARGETS: &[usize] = &[3, 11, 19, 27];
const DOCUMENT_TARGETS: &[usize] = &[4, 16, 29, 42];
const TEXT_AREA_TARGETS: &[usize] = &[6, 18, 31, 44];

struct SmoothScrollTargetsDemo;

#[derive(Default)]
struct State {
    message_target: usize,
    document_target: usize,
    text_area_target: usize,
}

#[derive(Clone, Debug)]
enum Msg {}

impl Component for SmoothScrollTargetsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('1') => {
                ctx.state.message_target = next_index(ctx.state.message_target, MESSAGE_TARGETS);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') => {
                ctx.state.document_target = next_index(ctx.state.document_target, DOCUMENT_TARGETS);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('3') => {
                ctx.state.text_area_target =
                    next_index(ctx.state.text_area_target, TEXT_AREA_TARGETS);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                ctx.state.message_target = next_index(ctx.state.message_target, MESSAGE_TARGETS);
                ctx.state.document_target = next_index(ctx.state.document_target, DOCUMENT_TARGETS);
                ctx.state.text_area_target =
                    next_index(ctx.state.text_area_target, TEXT_AREA_TARGETS);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                ctx.state.message_target = 0;
                ctx.state.document_target = 0;
                ctx.state.text_area_target = 0;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let message_id = MESSAGE_TARGETS[ctx.state.message_target];
        let document_line = DOCUMENT_TARGETS[ctx.state.document_target];
        let text_area_line = TEXT_AREA_TARGETS[ctx.state.text_area_target];

        Frame::new()
            .title("Smooth scroll targets")
            .status("1/2/3 jump panels · a jump all · r reset · wheel/drag cancels · q quits")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(summary(message_id, document_line, text_area_line))
                    .child(render_scroll_view_panel(message_id))
                    .child(render_document_panel(document_line))
                    .child(render_text_area_panel(text_area_line)),
            )
            .into()
    }
}

fn next_index(current: usize, targets: &[usize]) -> usize {
    (current + 1) % targets.len()
}

fn adaptive_smooth_scroll() -> ScrollBehavior {
    ScrollBehavior::smooth_distance(ScrollDistanceConfig::new(
        Duration::from_millis(120),
        Duration::from_millis(720),
        Duration::from_millis(12),
        Easing::EaseOutQuad,
    ))
}

fn summary(message_id: usize, document_line: usize, text_area_line: usize) -> Element {
    Frame::new()
        .title("Targets")
        .height(Length::Auto)
        .border(true)
        .border_style(BorderStyle::Plain)
        .padding(1)
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new(
                    "Each panel uses an explicit programmatic target with distance-adaptive smooth timing.",
                ))
                .child(Text::new(format!(
                    "ScrollView → message-{message_id} · DocumentView → source line {document_line} · TextArea → logical line {text_area_line}",
                )))
                .child(
                    Text::new(
                        "Try jumping, then interrupt with the mouse wheel or a scrollbar drag: user input snaps immediately.",
                    )
                    .style(Style::new().fg(Color::DarkGray)),
                ),
        )
        .into()
}

fn render_scroll_view_panel(target_id: usize) -> Element {
    let target_key = message_key(target_id);
    let timeline = ScrollView::new()
        .scrollbar(true)
        .show_scroll_indicators(false)
        .scroll_wheel(true)
        .scroll_to_key(target_key.clone())
        .scroll_behavior(adaptive_smooth_scroll())
        .padding(1)
        .gap(1)
        .children((0..32).map(move |id| render_message_card(id, target_id)));

    Frame::new()
        .title(format!("1 · ScrollView::scroll_to_key({target_key})"))
        .status("keyed top-level children")
        .height(Length::Flex(1))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .child(timeline)
        .into()
}

fn render_message_card(id: usize, target_id: usize) -> Element {
    let highlight = id == target_id;
    let title = if highlight {
        format!("▶ message-{id} target")
    } else {
        format!("message-{id}")
    };
    let body = format!(
        "Timeline entry #{id}. Smooth target scrolling resolves the key to this top-level child row before animating."
    );

    Element::from(
        Frame::new()
            .title(title)
            .border(true)
            .border_style(BorderStyle::Plain)
            .padding(1)
            .child(Text::new(body).style(if highlight {
                Style::new().fg(Color::LightGreen).bold()
            } else {
                Style::new().fg(Color::White)
            })),
    )
    .key(message_key(id))
}

fn render_document_panel(target_line: usize) -> Element {
    let document = DocumentView::new(document_source())
        .border(false)
        .focusable(false)
        .line_numbers(true)
        .scrollbar(true)
        .wrap(true)
        .scroll_to_source_line(target_line)
        .scroll_behavior(adaptive_smooth_scroll());

    Frame::new()
        .title(format!(
            "2 · DocumentView::scroll_to_source_line({target_line})"
        ))
        .status("zero-based source line")
        .height(Length::Flex(1))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .child(document)
        .into()
}

fn render_text_area_panel(target_line: usize) -> Element {
    let editor = TextArea::new(text_area_source())
        .read_only(true)
        .focusable(false)
        .border(false)
        .line_numbers(true)
        .scrollbar(true)
        .h_scrollbar(true)
        .wrap(true)
        .scroll_wheel(true)
        .scroll_to_line(target_line)
        .scroll_behavior(adaptive_smooth_scroll());

    Frame::new()
        .title(format!("3 · TextArea::scroll_to_line({target_line})"))
        .status("zero-based logical line")
        .height(Length::Flex(1))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .child(editor)
        .into()
}

fn message_key(id: usize) -> String {
    format!("message-{id}")
}

fn document_source() -> String {
    (0..54)
        .map(|line| {
            let marker = if DOCUMENT_TARGETS.contains(&line) {
                "◀ source target"
            } else {
                ""
            };
            format!(
                "source line {line:02}: render wrapped documentation, code comments, and diagnostics while preserving source-line targets. {marker}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_area_source() -> String {
    (0..56)
        .map(|line| {
            let marker = if TEXT_AREA_TARGETS.contains(&line) {
                "◀ logical target"
            } else {
                ""
            };
            format!(
                "text line {line:02}: read-only editor content can jump to a logical line without emitting scroll callbacks. {marker}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> Result<()> {
    App::new()
        .title("Smooth scroll targets example")
        .mount(SmoothScrollTargetsDemo)
        .run()
}
