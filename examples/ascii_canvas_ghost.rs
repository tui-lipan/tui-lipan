//! ASCII ghost demo using AsciiCanvas with frame sequences.
//!
//! Run with: cargo run --example ascii_canvas_ghost

use std::sync::Arc;

use tui_lipan::prelude::*;

const GHOST_JSON: &str = include_str!("assets/ghost.json");
const GHOST_BASE_FG: Color = Color::rgb(255, 255, 255);

struct GhostCanvas;

struct State {
    sequence: Arc<FrameSequence>,
    current_frame: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            sequence: Arc::new(parse_ghost_frames()),
            current_frame: 0,
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    Move(MouseMoveEvent),
}

impl Component for GhostCanvas {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Move(ev) => {
                let seq = &ctx.state.sequence;
                if seq.is_empty() {
                    return Update::none();
                }

                let ghost_w = seq.width();
                let ghost_h = seq.height();
                let dir = direction_from_event(ev, ghost_w, ghost_h);
                let tag_value = dir.as_tag();

                let next = seq.find_by_tag("direction", tag_value).unwrap_or(0);

                if next != ctx.state.current_frame {
                    ctx.state.current_frame = next;
                    Update::full()
                } else {
                    Update::none()
                }
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let canvas = AsciiCanvas::from_sequence(ctx.state.sequence.clone())
            .frame(ctx.state.current_frame)
            .style(Style::new().fg(GHOST_BASE_FG));

        Frame::new()
            .title("Ghost (AsciiCanvas)")
            .status(format!(
                "frame {}/{}",
                ctx.state.current_frame + 1,
                ctx.state.sequence.len()
            ))
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                MouseRegion::new()
                    .on_mouse_move(ctx.link().callback(Msg::Move))
                    .child(Center::new().child(canvas)),
            )
            .into()
    }
}

/// Parse ghost.json using the built-in FrameSequence parser, then tag
/// each frame with its direction based on known frame order.
fn parse_ghost_frames() -> FrameSequence {
    let mut seq = FrameSequence::from_json(GHOST_JSON).expect("failed to parse ghost.json");

    // The ghost.json frames are ordered:
    // 0=center, 1=bottomRight, 2=right, 3=topRight, 4=top,
    // 5=topLeft, 6=left, 7=bottomLeft, 8=bottom
    let directions = [
        "center",
        "bottomRight",
        "right",
        "topRight",
        "top",
        "topLeft",
        "left",
        "bottomLeft",
        "bottom",
    ];

    for (i, dir) in directions.iter().enumerate() {
        if let Some(frame) = seq.get_mut(i) {
            frame.tags.insert("direction".to_string(), dir.to_string());
        }
    }

    seq
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Center,
    BottomRight,
    Right,
    TopRight,
    Top,
    TopLeft,
    Left,
    BottomLeft,
    Bottom,
}

impl Direction {
    fn as_tag(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::BottomRight => "bottomRight",
            Self::Right => "right",
            Self::TopRight => "topRight",
            Self::Top => "top",
            Self::TopLeft => "topLeft",
            Self::Left => "left",
            Self::BottomLeft => "bottomLeft",
            Self::Bottom => "bottom",
        }
    }
}

fn direction_from_event(ev: MouseMoveEvent, ghost_w: u16, ghost_h: u16) -> Direction {
    if ev.target_w == 0 || ev.target_h == 0 || ghost_w == 0 || ghost_h == 0 {
        return Direction::Center;
    }

    let ghost_center_x = ev.target_w / 2;
    let ghost_center_y = ev.target_h / 2;

    let dx = (ev.local_x as i32) - (ghost_center_x as i32);
    let dy = (ev.local_y as i32) - (ghost_center_y as i32);

    let threshold_x = (ghost_w as i32) / 3;
    let threshold_y = (ghost_h as i32) / 3;

    let left = dx < -threshold_x;
    let right = dx > threshold_x;
    let up = dy < -threshold_y;
    let down = dy > threshold_y - 2;

    match (left, right, up, down) {
        (true, false, true, false) => Direction::TopLeft,
        (false, false, true, false) => Direction::Top,
        (false, true, true, false) => Direction::TopRight,
        (true, false, false, false) => Direction::Left,
        (false, true, false, false) => Direction::Right,
        (true, false, false, true) => Direction::BottomLeft,
        (false, false, false, true) => Direction::Bottom,
        (false, true, false, true) => Direction::BottomRight,
        _ => Direction::Center,
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new().mount(GhostCanvas).run()
}
