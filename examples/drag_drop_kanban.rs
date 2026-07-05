use std::sync::Arc;

use tui_lipan::prelude::*;

const CARD_BLOCK_ROWS: u16 = 6;
const CARD_GAP_ROWS: u16 = 1;
const COLUMN_CARD_START_ROW: u16 = 3;
const COLUMN_CARD_END_PADDING_ROWS: u16 = 2;

#[derive(Clone, Debug)]
struct Card {
    id: u64,
    title: Arc<str>,
    accent: Color,
}

#[derive(Clone, Debug)]
struct Column {
    title: Arc<str>,
    tint: Color,
    cards: Vec<Card>,
}

#[derive(Clone, Debug)]
struct CardPayload {
    card_id: u64,
    from_column: usize,
    title: Arc<str>,
}

struct KanbanDemo;

struct State {
    columns: Vec<Column>,
    status: Arc<str>,
    hover_insert: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
enum Msg {
    SetHover {
        column: usize,
        insert_at: usize,
    },
    ClearHover,
    MoveCard {
        card_id: u64,
        from_column: usize,
        to_column: usize,
        insert_at: usize,
    },
    CancelDrag(Arc<str>),
}

impl Default for State {
    fn default() -> Self {
        Self {
            columns: vec![
                Column {
                    title: Arc::from("Backlog"),
                    tint: Color::indexed(60),
                    cards: vec![
                        Card {
                            id: 1,
                            title: Arc::from("Add drag payload protocol"),
                            accent: Color::indexed(111),
                        },
                        Card {
                            id: 2,
                            title: Arc::from("Wire hover transitions"),
                            accent: Color::indexed(153),
                        },
                    ],
                },
                Column {
                    title: Arc::from("In Flight"),
                    tint: Color::indexed(173),
                    cards: vec![
                        Card {
                            id: 3,
                            title: Arc::from("Render drag preview label"),
                            accent: Color::indexed(215),
                        },
                        Card {
                            id: 4,
                            title: Arc::from("Keep splitter drag untouched"),
                            accent: Color::indexed(209),
                        },
                    ],
                },
                Column {
                    title: Arc::from("Done"),
                    tint: Color::indexed(29),
                    cards: vec![Card {
                        id: 5,
                        title: Arc::from("Hook escape cancellation"),
                        accent: Color::indexed(43),
                    }],
                },
            ],
            status: Arc::from(
                "Snapshot follows the pointer; source row collapses after the first frame. Hover a column to see the drop slot.",
            ),
            hover_insert: None,
        }
    }
}

impl Component for KanbanDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SetHover { column, insert_at } => {
                ctx.state.hover_insert = Some((column, insert_at));
                let title = ctx.state.columns[column].title.clone();
                ctx.state.status = Arc::from(format!("Drop into {title} at index {insert_at}"));
            }
            Msg::ClearHover => {
                ctx.state.hover_insert = None;
                ctx.state.status =
                    Arc::from("Drag - hover a column to show where the card will land");
            }
            Msg::MoveCard {
                card_id,
                from_column,
                to_column,
                insert_at,
            } => {
                ctx.state.hover_insert = None;
                if from_column >= ctx.state.columns.len() || to_column >= ctx.state.columns.len() {
                    return Update::none();
                }

                let from_idx = {
                    let col = &ctx.state.columns[from_column];
                    col.cards.iter().position(|c| c.id == card_id)
                };
                let Some(from_idx) = from_idx else {
                    return Update::none();
                };

                let mut insert_at = insert_at.min(ctx.state.columns[to_column].cards.len());

                let card = ctx.state.columns[from_column].cards.remove(from_idx);
                let card_title = card.title.clone();

                if from_column == to_column && from_idx < insert_at {
                    insert_at = insert_at.saturating_sub(1);
                }
                insert_at = insert_at.min(ctx.state.columns[to_column].cards.len());

                ctx.state.columns[to_column].cards.insert(insert_at, card);

                let to_title = ctx.state.columns[to_column].title.clone();
                ctx.state.status = Arc::from(format!("Placed {card_title} into {to_title}"));
            }
            Msg::CancelDrag(title) => {
                ctx.state.hover_insert = None;
                ctx.state.status = Arc::from(format!("Canceled drag for {title}"));
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
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let columns = HStack::new().gap(1).children(
            ctx.state
                .columns
                .iter()
                .enumerate()
                .map(|(column_index, column)| kanban_column(ctx, column_index, column)),
        );

        Frame::new()
            .title("Generic Drag and Drop")
            .status(ctx.state.status.clone())
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "One bordered placeholder only while hovering a column; no gap strips.",
                        )
                        .style(Style::new().fg(Color::indexed(248))),
                    )
                    .child(columns),
            )
            .into()
    }
}

fn kanban_column(ctx: &Context<KanbanDemo>, column_index: usize, column: &Column) -> Element {
    let insert_hint = ctx.state.hover_insert;
    let len = column.cards.len();

    let mut elems: Vec<Element> = Vec::new();
    for (idx, card) in column.cards.iter().enumerate() {
        if insert_hint == Some((column_index, idx)) {
            elems.push(drop_slot_placeholder());
        }
        elems.push(card_tile(ctx, column_index, idx, card));
    }
    if insert_hint == Some((column_index, len)) {
        elems.push(drop_slot_placeholder());
    }

    let stack = VStack::new().gap(1).children(elems);

    let frame = Frame::new()
        .title(column.title.clone())
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().fg(column.tint))
        .focus_style(Style::new().fg(column.tint).bold())
        .padding(1)
        .width(Length::Flex(1))
        .child(stack);

    let target = DropTarget::new()
        .child(frame)
        .accept_group("kanban")
        .on_drag_over(ctx.link().callback(move |ev: DragOverEvent| Msg::SetHover {
            column: column_index,
            insert_at: insertion_slot(ev.local_y, ev.local_height, len),
        }))
        .on_drag_leave(ctx.link().callback(|_| Msg::ClearHover))
        .on_drop(ctx.link().callback(move |ev: DropEvent| {
            let payload = ev
                .payload
                .downcast_ref::<CardPayload>()
                .expect("kanban payload should downcast");
            Msg::MoveCard {
                card_id: payload.card_id,
                from_column: payload.from_column,
                to_column: column_index,
                insert_at: insertion_slot(ev.local_y, ev.local_height, len),
            }
        }));

    Element::from(target).key(format!("column-{column_index}"))
}

fn insertion_slot(local_y: u16, local_height: u16, card_count: usize) -> usize {
    if card_count == 0 {
        return 0;
    }

    let row = local_y.saturating_sub(COLUMN_CARD_START_ROW);
    let content_height = local_height
        .saturating_sub(COLUMN_CARD_START_ROW)
        .saturating_sub(COLUMN_CARD_END_PADDING_ROWS)
        .max(1);
    let gap_total = CARD_GAP_ROWS.saturating_mul(card_count.saturating_sub(1) as u16);
    let card_height = content_height
        .saturating_sub(gap_total)
        .checked_div(card_count as u16)
        .unwrap_or(1)
        .max(1);
    let stride = card_height.saturating_add(CARD_GAP_ROWS).max(1);

    for idx in 0..card_count {
        let center = (idx as u16)
            .saturating_mul(stride)
            .saturating_add(card_height / 2);
        if row < center {
            return idx;
        }
    }

    card_count
}

fn drop_slot_placeholder() -> Element {
    Frame::new()
        .height(Length::Px(CARD_BLOCK_ROWS))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().fg(Color::indexed(214)).bg(Color::indexed(236)))
        .padding(1)
        .child(Text::new(""))
        .into()
}

fn card_tile(
    ctx: &Context<KanbanDemo>,
    column_index: usize,
    card_index: usize,
    card: &Card,
) -> Element {
    let card_id = card.id;
    let title = card.title.clone();
    let accent = card.accent;
    let payload_title = title.clone();

    let source = DragSource::new()
        .child(
            Frame::new()
                .border(true)
                .border_style(BorderStyle::Rounded)
                .style(Style::new().fg(accent))
                .padding(1)
                .child(
                    VStack::new()
                        .gap(0)
                        .child(Text::new(title.clone()).style(Style::new().fg(Color::White).bold()))
                        .child(
                            Text::new("Drag to move").style(Style::new().fg(Color::indexed(244))),
                        ),
                ),
        )
        .drag_group("kanban")
        .threshold(3)
        .preview_snapshot()
        .on_drag_start(move |_| {
            Some(Box::new(CardPayload {
                card_id,
                from_column: column_index,
                title: payload_title.clone(),
            }) as Box<dyn DragPayload>)
        })
        .on_drag_cancel(ctx.link().callback(move |event: DragCancelEvent| {
            let payload = event
                .payload
                .downcast_ref::<CardPayload>()
                .expect("kanban payload should downcast");
            Msg::CancelDrag(payload.title.clone())
        }));

    Element::from(source).key(format!("card-{}-{}-{}", card.id, column_index, card_index))
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Drag and Drop Kanban")
        .mount(KanbanDemo)
        .run()
}
