//! Braille paint demo.
//!
//! Run with: cargo run --example paint

use tui_lipan::prelude::*;
use tui_lipan::utils::braille::{braille_char, clear_pixel, line_pixels, set_pixel};

const CANVAS_W: u16 = 128;
const CANVAS_H: u16 = 36;
const BRUSH_RADIUS: i32 = 1;

struct PaintDemo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
}

struct State {
    buffer: AsciiCanvasBuffer,
    pixel_mask: Vec<u8>,
    pixel_color: Vec<Option<Color>>,
    tool: Tool,
    color: Color,
    last_pixel: Option<(i32, i32)>,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    StrokeStart(MouseDragEvent),
    StrokeMove(MouseDragEvent),
    StrokeEnd,
    PickTool(Tool),
    PickColor(Color),
    Clear,
}

impl Default for State {
    fn default() -> Self {
        let len = CANVAS_W as usize * CANVAS_H as usize;
        Self {
            buffer: AsciiCanvasBuffer::new(CANVAS_W, CANVAS_H),
            pixel_mask: vec![0; len],
            pixel_color: vec![None; len],
            tool: Tool::Pencil,
            color: Color::indexed(51),
            last_pixel: None,
            status: "Drag to draw; strokes mirror braille edge dots across neighboring cells."
                .into(),
        }
    }
}

impl Component for PaintDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::StrokeStart(event) => {
                let current = cell_center(event.local_x, event.local_y);
                let previous = previous_cell_center(&event).unwrap_or(current);
                ctx.state.last_pixel = Some(previous);
                paint_line(&mut ctx.state, current);
            }
            Msg::StrokeMove(event) => {
                if let Some(current) = cell_center_in_canvas(event.local_x, event.local_y) {
                    paint_line(&mut ctx.state, current);
                } else {
                    ctx.state.last_pixel = None;
                }
            }
            Msg::StrokeEnd => {
                ctx.state.last_pixel = None;
            }
            Msg::PickTool(tool) => {
                ctx.state.tool = tool;
                ctx.state.status = match tool {
                    Tool::Pencil => "Pencil selected.".into(),
                    Tool::Eraser => "Eraser selected.".into(),
                };
            }
            Msg::PickColor(color) => {
                ctx.state.color = color;
                ctx.state.tool = Tool::Pencil;
                ctx.state.status = "Color selected; pencil active.".into();
            }
            Msg::Clear => {
                ctx.state.buffer = AsciiCanvasBuffer::new(CANVAS_W, CANVAS_H);
                ctx.state.pixel_mask.fill(0);
                ctx.state.pixel_color.fill(None);
                ctx.state.last_pixel = None;
                ctx.state.status = "Canvas cleared.".into();
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('e') => KeyUpdate::handled(self.update(Msg::PickTool(Tool::Eraser), ctx)),
            KeyCode::Char('p') => KeyUpdate::handled(self.update(Msg::PickTool(Tool::Pencil), ctx)),
            KeyCode::Char('c') => KeyUpdate::handled(self.update(Msg::Clear, ctx)),
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Paint")
            .status(format!(
                "{} | p: pencil e: eraser c: clear q/esc: quit",
                ctx.state.status
            ))
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(toolbar(ctx))
                    .child(
                        Frame::new()
                            .title("Braille canvas")
                            .border_style(BorderStyle::Rounded)
                            .padding(0)
                            .child(
                                MouseRegion::new()
                                    .on_drag_start(
                                        ctx.link().callback(|event: MouseDragEvent| {
                                            Msg::StrokeStart(event)
                                        }),
                                    )
                                    .on_drag(
                                        ctx.link().callback(|event: MouseDragEvent| {
                                            Msg::StrokeMove(event)
                                        }),
                                    )
                                    .on_drag_end(
                                        ctx.link().callback(|_: MouseDragEvent| Msg::StrokeEnd),
                                    )
                                    .child(
                                        AsciiCanvas::from_cells(
                                            CANVAS_W,
                                            CANVAS_H,
                                            ctx.state.buffer.cells().to_vec(),
                                        )
                                        .width(Length::Px(CANVAS_W))
                                        .height(Length::Px(CANVAS_H))
                                        .background(Style::new().bg(Color::indexed(234))),
                                    ),
                            ),
                    )
                    .child(Text::new(tool_label(ctx.state.tool, ctx.state.color))),
            )
            .into()
    }
}

fn toolbar(ctx: &Context<PaintDemo>) -> Element {
    HStack::new()
        .gap(1)
        .height(Length::Auto)
        .child(Button::new("Pencil").on_click(ctx.link().callback(|_| Msg::PickTool(Tool::Pencil))))
        .child(Button::new("Eraser").on_click(ctx.link().callback(|_| Msg::PickTool(Tool::Eraser))))
        .child(Button::new("Clear").on_click(ctx.link().callback(|_| Msg::Clear)))
        .children([
            color_button(ctx, "Cyan", Color::indexed(51)),
            color_button(ctx, "Magenta", Color::indexed(201)),
            color_button(ctx, "Yellow", Color::indexed(226)),
            color_button(ctx, "Green", Color::indexed(46)),
            color_button(ctx, "White", Color::indexed(255)),
        ])
        .into()
}

fn color_button(ctx: &Context<PaintDemo>, label: &'static str, color: Color) -> Element {
    Button::new(label)
        .style(Style::new().fg(color))
        .on_click(ctx.link().callback(move |_| Msg::PickColor(color)))
        .into()
}

fn tool_label(tool: Tool, color: Color) -> String {
    format!("Tool: {tool:?} | color: {color:?} | edge-mirrored brush | single clicks ignored")
}

fn cell_center(local_x: u16, local_y: u16) -> (i32, i32) {
    (local_x as i32 * 2 + 1, local_y as i32 * 4 + 2)
}

fn cell_center_in_canvas(local_x: u16, local_y: u16) -> Option<(i32, i32)> {
    (local_x < CANVAS_W && local_y < CANVAS_H).then(|| cell_center(local_x, local_y))
}

fn previous_cell_center(event: &MouseDragEvent) -> Option<(i32, i32)> {
    let x = event.local_x as i32 - event.delta_x as i32;
    let y = event.local_y as i32 - event.delta_y as i32;
    (x >= 0 && y >= 0 && x < CANVAS_W as i32 && y < CANVAS_H as i32).then(|| (x * 2 + 1, y * 4 + 2))
}

fn paint_line(state: &mut State, current: (i32, i32)) {
    let previous = state.last_pixel.unwrap_or(current);
    let tool = state.tool;
    let color = state.color;
    line_pixels(previous, current, |px, py| {
        stamp_brush(state, px, py, tool, color);
    });
    state.last_pixel = Some(current);
}

fn stamp_brush(state: &mut State, px: i32, py: i32, tool: Tool, color: Color) {
    // Terminals report mouse positions at cell granularity. Stamp nearby braille
    // dots and mirror edge dots so boundaries light up on both neighboring cells.
    for dy in -BRUSH_RADIUS..=BRUSH_RADIUS {
        for dx in -BRUSH_RADIUS..=BRUSH_RADIUS {
            apply_edge_pixel(state, px + dx, py + dy, tool, color);
        }
    }
}

fn apply_edge_pixel(state: &mut State, px: i32, py: i32, tool: Tool, color: Color) {
    apply_pixel(state, px, py, tool, color);

    let edge_dx = if px.rem_euclid(2) == 0 { -1 } else { 1 };
    let edge_dy = match py.rem_euclid(4) {
        0 => Some(-1),
        3 => Some(1),
        _ => None,
    };

    apply_pixel(state, px + edge_dx, py, tool, color);
    if let Some(edge_dy) = edge_dy {
        apply_pixel(state, px, py + edge_dy, tool, color);
        apply_pixel(state, px + edge_dx, py + edge_dy, tool, color);
    }
}

fn apply_pixel(state: &mut State, px: i32, py: i32, tool: Tool, color: Color) {
    if px < 0 || py < 0 {
        return;
    }

    let cell_x = px / 2;
    let cell_y = py / 4;
    if cell_x >= CANVAS_W as i32 || cell_y >= CANVAS_H as i32 {
        return;
    }

    let sub_x = (px % 2) as u8;
    let sub_y = (py % 4) as u8;
    let idx = cell_y as usize * CANVAS_W as usize + cell_x as usize;
    let before_mask = state.pixel_mask[idx];
    let before_color = state.pixel_color[idx];
    let after_mask = match tool {
        Tool::Pencil => set_pixel(before_mask, sub_x, sub_y),
        Tool::Eraser => clear_pixel(before_mask, sub_x, sub_y),
    };
    let after_color = match tool {
        Tool::Pencil => Some(color),
        Tool::Eraser if after_mask == 0 => None,
        Tool::Eraser => before_color,
    };

    if after_mask == before_mask && after_color == before_color {
        return;
    }

    state.pixel_mask[idx] = after_mask;
    state.pixel_color[idx] = after_color;

    let style = after_color.map_or_else(Style::new, |fg| Style::new().fg(fg));
    state.buffer.set(
        cell_x as u16,
        cell_y as u16,
        AsciiCell::new(braille_char(after_mask)).style(style),
    );
}

fn main() -> Result<()> {
    App::new().mount(PaintDemo).run()
}
