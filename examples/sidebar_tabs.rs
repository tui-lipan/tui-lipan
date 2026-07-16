//! Rich vertical "sidebar tabs" composed entirely from primitives.
//!
//! Each item has a status glyph (or live spinner), a label, and a description
//! line — richer than a `DraggableTabBar` tab — and can be reordered by
//! dragging. The pattern:
//!
//! - `DragSource` with a snapshot preview wraps each item's content.
//! - A per-item `DropTarget` maps the pointer's top/bottom half to an
//!   insert-before/insert-after slot, so no stride math is needed.
//! - A constant-height indicator row above every item doubles as the
//!   insertion marker, so nothing shifts while hovering (no flicker).
//!
//! Controls: click or Up/Down to select, Space cycles the selected item's
//! status, drag an item to reorder, `q`/Esc quits.

use std::sync::Arc;

use tui_lipan::prelude::*;

const SIDEBAR_WIDTH: u16 = 34;
const ITEM_CONTENT_ROWS: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceStatus {
    Running,
    Ready,
    Error,
    Idle,
}

impl ServiceStatus {
    fn glyph(self) -> (&'static str, Color) {
        match self {
            // Running renders a Spinner instead; glyph is a fallback.
            Self::Running => ("~", Color::indexed(215)),
            Self::Ready => ("●", Color::indexed(114)),
            Self::Error => ("✗", Color::indexed(203)),
            Self::Idle => ("○", Color::indexed(244)),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Idle => "idle",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Running => Self::Ready,
            Self::Ready => Self::Error,
            Self::Error => Self::Idle,
            Self::Idle => Self::Running,
        }
    }
}

#[derive(Clone, Debug)]
struct Service {
    id: u64,
    name: Arc<str>,
    desc: Arc<str>,
    status: ServiceStatus,
}

/// Drag payload carried while a sidebar item is being moved.
#[derive(Clone, Debug)]
struct ServicePayload {
    id: u64,
    name: Arc<str>,
}

struct SidebarTabsDemo;

struct State {
    services: Vec<Service>,
    selected: u64,
    hover_insert: Option<usize>,
    status: Arc<str>,
}

#[derive(Clone, Debug)]
enum Msg {
    Select(u64),
    HoverInsert(usize),
    ClearHover,
    Drop { id: u64, insert_at: usize },
    CancelDrag(Arc<str>),
}

impl Default for State {
    fn default() -> Self {
        let services = vec![
            Service {
                id: 1,
                name: Arc::from("api-gateway"),
                desc: Arc::from("routes public traffic"),
                status: ServiceStatus::Running,
            },
            Service {
                id: 2,
                name: Arc::from("auth"),
                desc: Arc::from("tokens and sessions"),
                status: ServiceStatus::Ready,
            },
            Service {
                id: 3,
                name: Arc::from("billing"),
                desc: Arc::from("invoices, retries"),
                status: ServiceStatus::Error,
            },
            Service {
                id: 4,
                name: Arc::from("search-index"),
                desc: Arc::from("nightly rebuild"),
                status: ServiceStatus::Idle,
            },
            Service {
                id: 5,
                name: Arc::from("mailer"),
                desc: Arc::from("transactional email"),
                status: ServiceStatus::Ready,
            },
        ];
        Self {
            services,
            selected: 1,
            hover_insert: None,
            status: Arc::from("Click or Up/Down to select · drag to reorder · Space cycles status"),
        }
    }
}

impl State {
    fn selected_index(&self) -> Option<usize> {
        self.services.iter().position(|s| s.id == self.selected)
    }
}

impl Component for SidebarTabsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(id) => {
                ctx.state.selected = id;
                if let Some(svc) = ctx.state.services.iter().find(|s| s.id == id) {
                    ctx.state.status = Arc::from(format!("Selected {}", svc.name));
                }
            }
            Msg::HoverInsert(insert_at) => {
                ctx.state.hover_insert = Some(insert_at);
                ctx.state.status = Arc::from(format!("Drop to insert at position {insert_at}"));
            }
            Msg::ClearHover => {
                ctx.state.hover_insert = None;
            }
            Msg::Drop { id, mut insert_at } => {
                ctx.state.hover_insert = None;
                let Some(from) = ctx.state.services.iter().position(|s| s.id == id) else {
                    return Update::none();
                };
                let svc = ctx.state.services.remove(from);
                if from < insert_at {
                    insert_at -= 1;
                }
                insert_at = insert_at.min(ctx.state.services.len());
                let name = svc.name.clone();
                ctx.state.services.insert(insert_at, svc);
                ctx.state.status = Arc::from(format!("Moved {name} to position {insert_at}"));
            }
            Msg::CancelDrag(name) => {
                ctx.state.hover_insert = None;
                ctx.state.status = Arc::from(format!("Canceled drag for {name}"));
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
            KeyCode::Up | KeyCode::Down => {
                let len = ctx.state.services.len();
                if len == 0 {
                    return KeyUpdate::handled(Update::none());
                }
                let cur = ctx.state.selected_index().unwrap_or(0);
                let next = if key.code == KeyCode::Up {
                    cur.saturating_sub(1)
                } else {
                    (cur + 1).min(len - 1)
                };
                let svc = &ctx.state.services[next];
                ctx.state.selected = svc.id;
                ctx.state.status = Arc::from(format!("Selected {}", svc.name));
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char(' ') => {
                if let Some(idx) = ctx.state.selected_index() {
                    let svc = &mut ctx.state.services[idx];
                    svc.status = svc.status.next();
                    ctx.state.status =
                        Arc::from(format!("{} is now {}", svc.name, svc.status.label()));
                }
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Sidebar Tabs — composed from primitives")
            .status(ctx.state.status.clone())
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                HStack::new()
                    .gap(1)
                    .child(sidebar(ctx))
                    .child(details_pane(ctx)),
            )
            .into()
    }
}

fn sidebar(ctx: &Context<SidebarTabsDemo>) -> Element {
    let len = ctx.state.services.len();
    let mut rows: Vec<Element> = ctx
        .state
        .services
        .iter()
        .enumerate()
        .map(|(idx, svc)| sidebar_item(ctx, idx, svc))
        .collect();
    rows.push(end_drop_zone(ctx, len));

    Frame::new()
        .title("Services")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().fg(Color::indexed(60)))
        .padding(1)
        .width(Length::Px(SIDEBAR_WIDTH))
        .height(Length::Flex(1))
        .child(VStack::new().gap(0).children(rows))
        .into()
}

/// One sidebar item: an insertion-indicator row on top of the draggable,
/// clickable two-row content. The whole block is a drop target whose
/// top/bottom half maps to insert-before/insert-after.
fn sidebar_item(ctx: &Context<SidebarTabsDemo>, idx: usize, svc: &Service) -> Element {
    let id = svc.id;
    let selected = ctx.state.selected == id;
    let indicator = insert_indicator(ctx.state.hover_insert == Some(idx));

    let accent = if selected {
        Color::indexed(75)
    } else {
        Color::indexed(238)
    };
    let name_style = if selected {
        Style::new().fg(Color::indexed(231)).bold()
    } else {
        Style::new().fg(Color::indexed(250))
    };

    let leading: Element = match svc.status {
        ServiceStatus::Running => Spinner::new()
            .style(Style::new().fg(svc.status.glyph().1))
            .into(),
        status => {
            let (glyph, color) = status.glyph();
            Text::new(glyph).style(Style::new().fg(color)).into()
        }
    };

    let content = HStack::new()
        .gap(1)
        .height(Length::Px(ITEM_CONTENT_ROWS))
        .child(Text::new(if selected { "▎" } else { " " }).style(Style::new().fg(accent)))
        .child(leading)
        .child(
            VStack::new()
                .gap(0)
                .child(Text::new(svc.name.clone()).style(name_style))
                .child(Text::new(svc.desc.clone()).style(Style::new().fg(Color::indexed(244)))),
        );

    let region = MouseRegion::new()
        .child(content)
        .hover_style(Style::new().bg(Color::indexed(237)))
        .on_click(ctx.link().callback(move |_| Msg::Select(id)));

    let payload_name = svc.name.clone();
    let source = DragSource::new()
        .child(region)
        .drag_group("sidebar")
        .threshold(2)
        .preview_snapshot()
        .on_drag_start(move |_| {
            Some(Box::new(ServicePayload {
                id,
                name: payload_name.clone(),
            }) as Box<dyn DragPayload>)
        })
        .on_drag_cancel(ctx.link().callback(move |event: DragCancelEvent| {
            let payload = event
                .payload
                .downcast_ref::<ServicePayload>()
                .expect("sidebar payload should downcast");
            Msg::CancelDrag(payload.name.clone())
        }));

    let target = DropTarget::new()
        .child(
            VStack::new()
                .gap(0)
                // Pin the block height; stacks default to Flex(1) and would
                // otherwise spread the items across the sidebar.
                .height(Length::Px(ITEM_CONTENT_ROWS + 1))
                .child(indicator)
                .child(source),
        )
        .accept_group("sidebar")
        .on_drag_over(ctx.link().callback(move |ev: DragOverEvent| {
            Msg::HoverInsert(half_slot(ev.local_y, ev.local_height, idx))
        }))
        .on_drag_leave(ctx.link().callback(|_| Msg::ClearHover))
        .on_drop(ctx.link().callback(move |ev: DropEvent| {
            let payload = ev
                .payload
                .downcast_ref::<ServicePayload>()
                .expect("sidebar payload should downcast");
            Msg::Drop {
                id: payload.id,
                insert_at: half_slot(ev.local_y, ev.local_height, idx),
            }
        }));

    Element::from(target).key(format!("svc-{id}"))
}

/// Trailing drop zone: fills the leftover sidebar space so dropping below
/// the last item appends at the end.
fn end_drop_zone(ctx: &Context<SidebarTabsDemo>, len: usize) -> Element {
    let indicator = insert_indicator(ctx.state.hover_insert == Some(len));

    let target = DropTarget::new()
        .child(
            VStack::new()
                .gap(0)
                .height(Length::Flex(1))
                .child(indicator)
                .child(Spacer::new().height(Length::Flex(1))),
        )
        .accept_group("sidebar")
        .on_drag_over(ctx.link().callback(move |_| Msg::HoverInsert(len)))
        .on_drag_leave(ctx.link().callback(|_| Msg::ClearHover))
        .on_drop(ctx.link().callback(move |ev: DropEvent| {
            let payload = ev
                .payload
                .downcast_ref::<ServicePayload>()
                .expect("sidebar payload should downcast");
            Msg::Drop {
                id: payload.id,
                insert_at: len,
            }
        }));

    Element::from(target).key("end-zone")
}

/// Constant one-row slot that lights up as the insertion marker. Because the
/// row is always present, hovering never shifts the layout.
fn insert_indicator(on: bool) -> Element {
    if on {
        Divider::horizontal()
            .style(Style::new().fg(Color::indexed(214)))
            .into()
    } else {
        Spacer::new().height(Length::Px(1)).into()
    }
}

/// Map the pointer's position inside a drop target to an insertion slot:
/// top half inserts before the item, bottom half after it.
fn half_slot(local_y: u16, local_height: u16, index: usize) -> usize {
    if u32::from(local_y) * 2 < u32::from(local_height.max(1)) {
        index
    } else {
        index + 1
    }
}

fn details_pane(ctx: &Context<SidebarTabsDemo>) -> Element {
    let Some(idx) = ctx.state.selected_index() else {
        return Frame::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .child(Text::new("Nothing selected"))
            .into();
    };
    let svc = &ctx.state.services[idx];
    let (glyph, color) = svc.status.glyph();

    Frame::new()
        .title(svc.name.clone())
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().fg(Color::indexed(66)))
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(
            VStack::new()
                .gap(1)
                .child(
                    HStack::new()
                        .gap(1)
                        // Rows keep their natural height; stacks default to
                        // Flex(1) and would spread down the pane otherwise.
                        .height(Length::Px(1))
                        .child(Text::new(glyph).style(Style::new().fg(color)))
                        .child(
                            Text::new(format!("status: {}", svc.status.label()))
                                .style(Style::new().fg(Color::indexed(250))),
                        ),
                )
                .child(Text::new(svc.desc.clone()).style(Style::new().fg(Color::indexed(244))))
                .child(Divider::horizontal().style(Style::new().fg(Color::indexed(238))))
                .child(
                    VStack::new()
                        .gap(0)
                        .height(Length::Px(4))
                        .child(hint("click / Up/Down", "select item"))
                        .child(hint("drag", "reorder (snapshot preview)"))
                        .child(hint("Space", "cycle status"))
                        .child(hint("q / Esc", "quit")),
                )
                .child(Spacer::new().height(Length::Flex(1))),
        )
        .into()
}

fn hint(keys: &str, action: &str) -> Element {
    HStack::new()
        .gap(1)
        .height(Length::Px(1))
        .child(
            Text::new(keys.to_string())
                .style(Style::new().fg(Color::indexed(215)))
                .width(Length::Px(16)),
        )
        .child(Text::new(action.to_string()).style(Style::new().fg(Color::indexed(246))))
        .into()
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Sidebar Tabs from Primitives")
        .mount(SidebarTabsDemo)
        .run()
}
