//! A List with composed rows: hovering a thread swaps its age for Snooze and Settle buttons.
use tui_lipan::prelude::*;

const THREADS: [(&str, &str); 5] = [
    ("Review the focus restore path", "2m"),
    ("Release notes for v0.48", "14m"),
    ("Flaky snapshot on CI", "1h"),
    ("Pointer shapes over terminals", "3h"),
    ("Theme contrast audit", "1d"),
];

struct Inbox;

#[derive(Default)]
struct State {
    selected: usize,
    hovered: Option<usize>,
    snoozed: Vec<usize>,
    settled: Vec<usize>,
    opened: Option<usize>,
}

enum Msg {
    Select(usize),
    Hover(usize, bool),
    Open(usize),
    Snooze(usize),
    Settle(usize),
}

impl Component for Inbox {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State::default()
    }

    fn update(&mut self, message: Msg, ctx: &mut Context<Self>) -> Update {
        let state = &mut ctx.state;
        match message {
            Msg::Select(index) => state.selected = index,
            Msg::Hover(index, true) => state.hovered = Some(index),
            Msg::Hover(index, false) if state.hovered == Some(index) => state.hovered = None,
            Msg::Hover(..) => return Update::none(),
            Msg::Open(index) => state.opened = Some(index),
            Msg::Snooze(index) => state.snoozed.push(index),
            Msg::Settle(index) => state.settled.push(index),
        }
        Update::render()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = ctx.theme();
        let state = &ctx.state;
        let rows = THREADS.iter().enumerate().map(|(index, (title, age))| {
            let done = state.settled.contains(&index);
            let mut row = HStack::new()
                .gap(1)
                .height(Length::Px(1))
                .child(Text::new(if done { "✓" } else { "●" }).style(theme.accent))
                .child(Text::new(*title).width(Length::Flex(1)));
            row = if state.hovered == Some(index) && !done {
                row.child(
                    Button::new("Snooze")
                        .tab_stop(false)
                        .on_click(ctx.link().callback(move |_| Msg::Snooze(index))),
                )
                .child(
                    Button::new("✓ Settle")
                        .tab_stop(false)
                        .on_click(ctx.link().callback(move |_| Msg::Settle(index))),
                )
            } else {
                row.child(Text::new(*age).style(theme.muted))
            };
            ListItem::new(*title).key(*title).content(row)
        });
        let status = format!(
            "Opened: {} · Snoozed: {} · Settled: {}",
            state.opened.map_or("—", |index| THREADS[index].0),
            state.snoozed.len(),
            state.settled.len(),
        );
        VStack::new()
            .gap(1)
            .padding(1)
            .child(Text::new("Inbox · List with composed rows").style(theme.accent))
            .child(
                List::new()
                    .items([ListItem::header("Threads")].into_iter().chain(rows))
                    .selected(state.selected)
                    .scrollbar(true)
                    .on_select(
                        ctx.link()
                            .callback(|event: ListEvent| Msg::Select(event.index)),
                    )
                    .on_activate(
                        ctx.link()
                            .callback(|event: ListEvent| Msg::Open(event.index - 1)),
                    )
                    // Row 0 is the header; threads start at row 1.
                    .on_hover(ctx.link().callback_opt(|event: ListHoverEvent| {
                        (event.index > 0).then(|| Msg::Hover(event.index - 1, event.hovered))
                    }))
                    .key("inbox"),
            )
            .child(Text::new(status))
            .child(
                Text::new("Hover a thread for its actions · Click or Enter: open · ↑/↓: select")
                    .style(theme.muted),
            )
            .into()
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new().mount(Inbox).run()
}
