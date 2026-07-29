//! State-owned exit animation recipe using `ExitQueue`.
//!
//! Run with:
//!
//! ```text
//! cargo run --example exit_animation
//! ```
//!
//! Press `x` to move the first live row into the exit queue, `a` to add a row,
//! or `q` to quit. The queued row remains mounted until
//! `Animated::on_exit_complete` sends its id back to the component.

use tui_lipan::prelude::*;

const EXIT_DURATION_MS: u64 = 420;

struct ExitAnimation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    id: usize,
    label: &'static str,
    color: Color,
}

struct State {
    rows: Vec<Row>,
    exiting: ExitQueue<usize, Row>,
    next_id: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Remove(usize),
    AddRow,
    ExitComplete(usize),
}

impl Component for ExitAnimation {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let rows = vec![
            Row {
                id: 0,
                label: "compile",
                color: Color::Rgb(93, 180, 255),
            },
            Row {
                id: 1,
                label: "test",
                color: Color::Rgb(120, 213, 153),
            },
            Row {
                id: 2,
                label: "release",
                color: Color::Rgb(237, 183, 91),
            },
        ];
        let mut exiting = ExitQueue::new();
        exiting.sync(rows.iter().copied().map(|row| (row.id, row)));
        Self::State {
            rows,
            exiting,
            next_id: 3,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Remove(id) => {
                if let Some(index) = ctx.state.rows.iter().position(|row| row.id == id) {
                    let row = ctx.state.rows.remove(index);
                    let live = ctx
                        .state
                        .rows
                        .iter()
                        .copied()
                        .map(|row| (row.id, row))
                        .collect::<Vec<_>>();
                    ctx.state.exiting.sync(live);
                    // The removed row is retained by `sync` as an invisible entry.
                    let _ = row;
                }
            }
            Msg::AddRow => {
                let id = ctx.state.next_id;
                ctx.state.next_id += 1;
                ctx.state.rows.push(Row {
                    id,
                    label: match id % 3 {
                        0 => "package",
                        1 => "deploy",
                        _ => "observe",
                    },
                    color: match id % 3 {
                        0 => Color::Rgb(202, 133, 235),
                        1 => Color::Rgb(233, 112, 124),
                        _ => Color::Rgb(104, 198, 190),
                    },
                });
                let live = ctx
                    .state
                    .rows
                    .iter()
                    .copied()
                    .map(|row| (row.id, row))
                    .collect::<Vec<_>>();
                ctx.state.exiting.sync(live);
            }
            Msg::ExitComplete(id) => {
                // The animation owns the visual lifetime; state owns the data
                // lifetime. Remove only the completed entry from the queue.
                ctx.state.exiting.finish(&id);
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let msg = match key.code {
            KeyCode::Char('x') => ctx.state.rows.first().map(|row| Msg::Remove(row.id)),
            KeyCode::Char('a') => Some(Msg::AddRow),
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                return KeyUpdate::handled(Update::full());
            }
            _ => None,
        };

        match msg {
            Some(msg) => {
                ctx.link().send(msg);
                KeyUpdate::handled(Update::full())
            }
            None => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut rows = VStack::new().gap(1).height(Length::Auto);
        let mut live_count = 0usize;
        let mut exiting_count = 0usize;
        for (id, row, visible) in ctx.state.exiting.iter() {
            if visible {
                live_count += 1;
            } else {
                exiting_count += 1;
            }
            // Live and exiting entries stay under this same keyed parent. Only
            // `exit(visible, ..)` changes, so reconcile retains node identity.
            rows = rows.child(animated_row(*row, visible, ctx).key(format!("row-{id}")));
        }
        if ctx.state.exiting.is_empty() {
            rows = rows.child(Text::new("No rows — press a to add one.").style(Style::new().dim()));
        }

        Frame::new()
            .header_left("ExitQueue + Animated")
            .footer_left(format!(
                "x remove first • a add row • q quit • live: {} • exiting: {}",
                live_count, exiting_count
            ))
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(
                        "Rows stay in state.exiting until finish runs after the collapse completes.",
                    ))
                    .child(rows),
            )
            .into()
    }
}

fn animated_row(row: Row, visible: bool, ctx: &Context<ExitAnimation>) -> Element {
    let action: Element = if visible {
        Button::new("remove [x]")
            .on_click(ctx.link().callback(move |_| Msg::Remove(row.id)))
            .into()
    } else {
        Text::new("leaving…").style(Style::new().dim()).into()
    };

    let content = Frame::new()
        .border(true)
        .style(Style::new().bg(Color::Rgb(30, 36, 49)))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Text::new(format!("#{:<2} {}", row.id, row.label))
                        .style(Style::new().fg(row.color).bold()),
                )
                .child(action),
        );

    let mut animated = Animated::new(content).exit(visible, EXIT_DURATION_MS);
    if !visible {
        // Keep the row's natural allocation while Animated interpolates its own
        // visible height to zero. Without this override the parent stack assigns
        // a zero-height rect immediately, so there is nothing left to animate.
        animated = animated.layout_height(Some(Length::Auto));
    }
    let mut animated: Element = animated
        .on_exit_complete(ctx.link().callback(move |_| Msg::ExitComplete(row.id)))
        .into();
    animated = animated.key(format!("row-{}", row.id));
    animated
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Exit Animation")
        .terminal_bg(query_host_colors().map(|colors| colors.bg))
        .mount(ExitAnimation)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    #[test]
    fn removed_row_remains_rendered_while_its_exit_animation_starts() {
        let mut backend = TestBackend::new(ExitAnimation);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 72,
            h: 18,
        });
        backend.render();
        assert!(backend.capture_frame().to_fixed_grid().contains("compile"));

        backend.dispatch(Msg::Remove(0)).unwrap();
        backend.render();

        assert!(backend.state().exiting.is_exiting(&0));
        assert!(
            backend.capture_frame().to_fixed_grid().contains("compile"),
            "the row must remain mounted for Animated to interpolate its exit"
        );
    }
}
