use std::time::Duration;

use tui_lipan::prelude::*;

struct PanViewExample;

enum Msg {
    Panned(PanEvent),
    NodeClicked(GraphNodeEvent),
}

#[derive(Default)]
struct State {
    pan_target: Option<(i32, i32)>,
    animate_pan: bool,
    last_pan: Option<PanEvent>,
    last_clicked: Option<GraphNodeEvent>,
}

impl Component for PanViewExample {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let pan_transition = TransitionConfig {
            duration: if ctx.state.animate_pan {
                Duration::from_millis(350)
            } else {
                Duration::ZERO
            },
            easing: Easing::EaseInOutCubic,
        };
        let controlled_offset = ctx.state.pan_target.map(|target| {
            (
                ctx.transition::<f32>("pan-x", target.0 as f32, pan_transition)
                    .round() as i32,
                ctx.transition::<f32>("pan-y", target.1 as f32, pan_transition)
                    .round() as i32,
            )
        });

        let graph = Graph::new()
            .root(sample_diagram())
            .direction(GraphDirection::TopDown)
            .gap_x(5)
            .gap_y(2)
            .node_border_style(BorderStyle::Rounded)
            .edge_border_style(BorderStyle::Rounded)
            .edge_style(Style::new().fg(Color::Gray))
            .node_style(Style::new().fg(Color::White))
            .node_hover_style(Style::new().fg(Color::LightCyan).bold())
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(ctx.link().callback(Msg::NodeClicked));
        let mut pan_view = PanView::new()
            .child(graph)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .clamp(false)
            .center_content(true)
            .free_pan_margin(2)
            .pan_state_key("pan-view-example-diagram")
            .on_pan(ctx.link().callback(Msg::Panned));
        if let Some(offset) = controlled_offset {
            pan_view = pan_view.offset(offset);
        }

        ui! {
            VStack::new().gap(1).padding(1) => {
                Frame::new()
                    .title("PanView diagram preview")
                    .border(true)
                    .padding(1)
                    .child(pan_view),
                Frame::new()
                    .title("Controls")
                    .border(true)
                    .height(Length::Auto)
                    .padding((0, 1))
                    .child(Text::new(status_line(ctx)).overflow(Overflow::Wrap)),
            }
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Panned(event) => {
                ctx.state.pan_target = Some((event.x, event.y));
                ctx.state.animate_pan = false;
                ctx.state.last_pan = Some(event);
            }
            Msg::NodeClicked(event) => ctx.state.last_clicked = Some(event),
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('r') => {
                if let Some(event) = ctx.state.last_pan {
                    ctx.state.pan_target = Some(center_offset(event.metrics));
                    ctx.state.animate_pan = true;
                    KeyUpdate::handled(Update::full())
                } else {
                    KeyUpdate::unhandled(Update::none())
                }
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn center_offset(metrics: PanMetrics) -> (i32, i32) {
    (
        (i32::from(metrics.content_w) - i32::from(metrics.viewport_w)) / 2,
        (i32::from(metrics.content_h) - i32::from(metrics.viewport_h)) / 2,
    )
}

fn status_line(ctx: &Context<PanViewExample>) -> String {
    let pan = match ctx.state.last_pan {
        Some(event) => format!(
            "offset=({}, {}) max=({}, {}) viewport={}x{} content={}x{}",
            event.x,
            event.y,
            event.metrics.max_x,
            event.metrics.max_y,
            event.metrics.viewport_w,
            event.metrics.viewport_h,
            event.metrics.content_w,
            event.metrics.content_h,
        ),
        None => "offset=centered".to_string(),
    };
    let clicked = match &ctx.state.last_clicked {
        Some(event) => format!("clicked='{}' path={:?}", event.label, event.path.segments()),
        None => "clicked=none".to_string(),
    };
    format!(
        "The graph starts centered in an unclamped, bounded PanView. Drag or use arrows/hjkl to pan; press r to animate back to center. {pan}. {clicked}."
    )
}

fn sample_diagram() -> GraphNode {
    GraphNode::new("workspace")
        .style(Style::new().fg(Color::LightMagenta).bold())
        .child(
            GraphNode::new("frontend")
                .child(deep_chain(
                    "home",
                    &["shell", "tabs", "session list", "active row"],
                ))
                .child(deep_chain(
                    "prompt",
                    &["composer", "slash menu", "filtered actions"],
                ))
                .child(GraphNode::new("theme")),
        )
        .child(
            GraphNode::new("runtime")
                .child(deep_chain(
                    "component tree",
                    &["diff", "reconcile", "node reuse", "layout cache", "paint"],
                ))
                .child(deep_chain(
                    "commands",
                    &["worker", "result", "message", "toast"],
                )),
        )
        .child(
            GraphNode::new("widgets")
                .child(
                    GraphNode::new("PanView")
                        .style(Style::new().fg(Color::LightGreen).bold())
                        .child(deep_chain(
                            "drag",
                            &["offset", "child rect", "hover hit-test", "on_pan"],
                        ))
                        .child(deep_chain("keyboard", &["keymap", "bounded step", "reset"])),
                )
                .child(GraphNode::new("Graph")),
        )
        .child(
            GraphNode::new("tooling")
                .child(deep_chain("format", &["ui-fmt", "rsx-fmt", "rustfmt"]))
                .child(GraphNode::new("docs")),
        )
}

fn deep_chain(root: &str, labels: &[&str]) -> GraphNode {
    labels
        .iter()
        .rev()
        .fold(GraphNode::new(root), |child, label| {
            GraphNode::new(*label).child(child)
        })
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - PanView Example")
        .mount(PanViewExample)
        .run()
}
