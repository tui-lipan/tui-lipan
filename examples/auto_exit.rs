//! Automatic exit animations with `Animated::auto_exit`.
//!
//! Run with:
//!
//! ```text
//! cargo run --example auto_exit
//! ```
//!
//! Press `x` to remove the first row, `a` to add one, or `q` to quit.
//!
//! Contrast this with `exit_animation.rs`, which drives the same effect manually
//! through `ExitQueue`. Here the component state holds only the live rows: it has
//! no `exiting` collection, no completion message, and no removal bookkeeping.
//! Dropping a row from `state.rows` is the entire removal path, and the container
//! keeps the already-rendered subtree around long enough to collapse it.

use tui_lipan::prelude::*;

const EXIT_DURATION_MS: u64 = 420;

struct AutoExit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    id: usize,
    label: &'static str,
    color: Color,
}

struct State {
    rows: Vec<Row>,
    next_id: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Remove,
    AddRow,
}

const LABELS: [(&str, Color); 4] = [
    ("deploy", Color::Rgb(122, 162, 247)),
    ("migrate", Color::Rgb(158, 206, 106)),
    ("backfill", Color::Rgb(224, 175, 104)),
    ("reindex", Color::Rgb(187, 154, 247)),
];

impl Component for AutoExit {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let rows = LABELS
            .iter()
            .enumerate()
            .map(|(id, (label, color))| Row {
                id,
                label,
                color: *color,
            })
            .collect::<Vec<_>>();
        State {
            next_id: rows.len(),
            rows,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut list = VStack::new().gap(0).height(Length::Auto);
        for row in &ctx.state.rows {
            // The key is what lets the container tell "this child left" apart from
            // "the list reordered", so auto_exit requires one.
            list = list.child(
                Animated::new(
                    Frame::new()
                        .padding((0, 1))
                        .style(Style::new().bg(row.color).fg(Color::Black))
                        .child(Text::new(format!(" {} ", row.label))),
                )
                // A bare duration means "fade out". An `ExitAnimation` says what leaving
                // should look like instead: here the row slides left as it goes, and the
                // rows below it close the gap.
                .auto_exit(ExitAnimation::slide(EXIT_DURATION_MS, -4, 0).with_collapse(true))
                .key(row.id.to_string()),
            );
        }

        VStack::new()
            .gap(1)
            .padding(1)
            .child(Text::new("auto_exit").style(Style::new().bold()))
            .child(Text::new("x removes the first row   a adds one   q quits"))
            .child(
                Text::new("rows slide out and collapse; the manual queue below fades")
                    .style(Style::new().fg(Color::DarkGray)),
            )
            .child(list)
            .child(
                Text::new(format!("{} live rows in state", ctx.state.rows.len()))
                    .style(Style::new().fg(Color::DarkGray)),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            // No exit bookkeeping: the row simply stops being described.
            Msg::Remove => {
                if !ctx.state.rows.is_empty() {
                    ctx.state.rows.remove(0);
                }
            }
            Msg::AddRow => {
                let id = ctx.state.next_id;
                let (label, color) = LABELS[id % LABELS.len()];
                ctx.state.next_id += 1;
                ctx.state.rows.push(Row { id, label, color });
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('x') => KeyUpdate::handled(self.update(Msg::Remove, ctx)),
            KeyCode::Char('a') => KeyUpdate::handled(self.update(Msg::AddRow, ctx)),
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Auto Exit")
        .terminal_bg(query_host_colors().map(|colors| colors.bg))
        .mount(AutoExit)
        .run()
}
