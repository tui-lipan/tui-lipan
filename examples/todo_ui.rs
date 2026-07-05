//! Todo app example using the `ui!` macro (autocomplete-friendly alternative to `rsx!`).
//!
//! Compare with `todo.rs` which uses `rsx!` for the same app.

use std::time::Duration;

use tui_lipan::prelude::*;
use tui_lipan::style::palette;

#[derive(Clone, Debug)]
struct TodoItem {
    id: u64,
    text: String,
    done: bool,
}

struct TodoApp;

#[derive(Default)]
struct State {
    draft: TextInput,
    todos: Vec<TodoItem>,
    next_id: u64,
    scroll: usize,
    confirm_delete: Option<u64>,
}

#[derive(Clone, Debug)]
enum Msg {
    DraftChanged(InputEvent),
    Add,
    Toggle(u64),
    RequestDelete(u64),
    CancelDelete,
    ConfirmDelete(u64),
    Scrolled(ScrollEvent),
    Seeded(Vec<TodoItem>),
}

impl Component for TodoApp {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        Some(ctx.link().command(|link| {
            std::thread::sleep(Duration::from_millis(50));
            link.send(Msg::Seeded(vec![
                TodoItem {
                    id: 1,
                    text: "Dogfood ui! with loops".to_string(),
                    done: false,
                },
                TodoItem {
                    id: 2,
                    text: "Add focus-within active panels".to_string(),
                    done: false,
                },
            ]));
        }))
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let confirm_id = ctx.state.confirm_delete;

        let sidebar_active = ctx.has_focus_within_key("sidebar");
        let main_active = ctx.has_focus_within_key("main");

        let panel_border_style = |active: bool| {
            if active {
                BorderStyle::Thick
            } else {
                BorderStyle::Rounded
            }
        };

        let panel_style = |active: bool| {
            Style::new().fg(if active {
                Color::LightBlue
            } else {
                Color::DarkGray
            })
        };

        let main_content: Element = ui! {
            Frame::new()
                .title("Todo App")
                .status("Tab to focus • Enter adds • Ctrl+Q quits")
                .padding(1)
                .border(true)
                .border_style(BorderStyle::Rounded) => {
                HStack::new().gap(1) => {
                    Frame::new()
                        .title("Todos")
                        .border(true)
                        .border_style(panel_border_style(sidebar_active))
                        .style(panel_style(sidebar_active))
                        .padding(1) @ "sidebar" => {
                        ScrollView::new()
                            .scrollbar(true)
                            .gap(0)
                            .offset(ctx.state.scroll)
                            .on_scroll(ctx.link().callback(Msg::Scrolled)) => {
                            for (_idx, todo) in ctx.state.todos.iter().enumerate() {
                                HStack::new().gap(1).height(Length::Auto) @ format!("todo-{}", todo.id) => {
                                    Button::new(if todo.done { format!("✓ {}", todo.text) } else { todo.text.clone() })
                                        .full_width(true)
                                        .style(
                                            if todo.done {
                                                Style::new().fg(Color::DarkGray)
                                            } else {
                                                Style::new().fg(Color::White)
                                            },
                                        )
                                        .hover_style(Style::new().fg(Color::LightCyan))
                                        .focus_style(Style::new().fg(Color::LightBlue).bold())
                                        .on_click(
                                            ctx
                                                .link()
                                                .callback({
                                                    let id = todo.id;
                                                    move |_| Msg::Toggle(id)
                                                }),
                                        )
                                        .on_key(
                                            ctx
                                                .link()
                                                .key_handler({
                                                    let id = todo.id;
                                                    move |k: KeyEvent| {
                                                        if k.is(KeyCode::Enter) { Some(Msg::Toggle(id)) } else { None }
                                                    }
                                                }),
                                        ),
                                    Button::new("✕")
                                        .width(Length::Px(5))
                                        .style(Style::new().fg(Color::indexed(203)))
                                        .hover_style(Style::new().fg(Color::LightRed).bg(Color::indexed(52)))
                                        .focus_style(Style::new().fg(Color::White).bg(Color::LightRed))
                                        .on_click(
                                            ctx
                                                .link()
                                                .callback({
                                                    let id = todo.id;
                                                    move |_| Msg::RequestDelete(id)
                                                }),
                                        )
                                        .on_key(
                                            ctx
                                                .link()
                                                .key_handler({
                                                    let id = todo.id;
                                                    move |k: KeyEvent| {
                                                        if k.is(KeyCode::Enter) {
                                                            Some(Msg::RequestDelete(id))
                                                        } else {
                                                            None
                                                        }
                                                    }
                                                }),
                                        ),
                                },
                            },
                        },
                    },
                    Frame::new()
                        .title("New Task")
                        .border(true)
                        .border_style(panel_border_style(main_active))
                        .style(panel_style(main_active))
                        .padding(1) @ "main" => {
                        VStack::new().gap(1) => {
                            HStack::new().gap(1) => {
                                Input::new(ctx.state.draft.text().to_owned())
                                    .cursor(ctx.state.draft.cursor())
                                    .placeholder("Add a task...")
                                    .border(true)
                                    .border_style(BorderStyle::Rounded)
                                    .hover_border_style(BorderStyle::Thick)
                                    .focus_style(Style::new().fg(Color::LightCyan))
                                    .on_change(ctx.link().callback(Msg::DraftChanged))
                                    .on_key(
                                        ctx
                                            .link()
                                            .key_handler(|k: KeyEvent| {
                                                if k.is(KeyCode::Enter) { Some(Msg::Add) } else { None }
                                            }),
                                    ) @ "draft",
                                Button::new("Add")
                                    .width(Length::Px(10))
                                    .variant(ButtonVariant::Filled)
                                    .style(Style::new().bg(Color::indexed(30)).fg(Color::White))
                                    .hover_style(Style::new().bg(Color::indexed(37)).fg(Color::White))
                                    .focus_style(Style::new().bg(Color::LightCyan).fg(Color::Black))
                                    .on_click(ctx.link().callback(|_| Msg::Add))
                                    .on_key(
                                        ctx
                                            .link()
                                            .key_handler(|k: KeyEvent| {
                                                if k.is(KeyCode::Enter) { Some(Msg::Add) } else { None }
                                            }),
                                    ) @ "add",
                            },
                            Divider::horizontal().style(Style::new().fg(Color::indexed(239))),
                            Text::new(
                                    format!(
                                        "{} todos • {} done", ctx.state.todos.len(), ctx.state.todos.iter()
                                        .filter(| t | t.done).count()
                                    ),
                                )
                                .style(Style::new().fg(Color::DarkGray)),
                            Spacer::new().height(Length::Px(1)),
                            Text::new("Tips:").style(Style::new().fg(Color::indexed(245)).bold()),
                            Text::new("• Active panel border follows focus")
                                .style(Style::new().fg(Color::indexed(240))),
                            Text::new("• Delete shows a modal dialog")
                                .style(Style::new().fg(Color::indexed(240))),
                            Text::new("• Toasts appear on actions")
                                .style(Style::new().fg(Color::indexed(240))),
                        },
                    },
                },
            }
        };

        if let Some(delete_id) = confirm_id {
            let modal = ui! {
                Modal::new()
                    .title("Confirm Delete")
                    .title_style(Style::new().fg(Color::LightRed).bold())
                    .frame_style(Style::new().fg(palette::red::B500))
                    .border_style(BorderStyle::Rounded)
                    .width(Length::Auto)
                    .on_close(ctx.link().callback(|_| Msg::CancelDelete)) => {
                    VStack::new().gap(1) => {
                        Text::new("Are you sure you want to delete this task?")
                            .style(Style::new().fg(Color::indexed(250))),
                        HStack::new().gap(1) => {
                            Button::outlined("Cancel")
                                .width(Length::Flex(1))
                                .style(Style::new().fg(Color::indexed(245)))
                                .hover_style(Style::new().fg(Color::White))
                                .focus_style(Style::new().fg(Color::LightCyan))
                                .on_click(ctx.link().callback(|_| Msg::CancelDelete))
                                .on_key(
                                    ctx
                                        .link()
                                        .key_handler(|k: KeyEvent| {
                                            if k.is(KeyCode::Enter) || k.is(KeyCode::Esc) {
                                                Some(Msg::CancelDelete)
                                            } else {
                                                None
                                            }
                                        }),
                                ) @ "modal-cancel",
                            Button::outlined("Delete")
                                .width(Length::Flex(1))
                                .style(Style::new().bg(Color::indexed(160)).fg(palette::red::B500))
                                .hover_style(Style::new().fg(palette::red::B400))
                                .focus_style(Style::new().fg(palette::red::B300))
                                .on_click(ctx.link().callback(move |_| Msg::ConfirmDelete(delete_id)))
                                .on_key(
                                    ctx
                                        .link()
                                        .key_handler(move |k: KeyEvent| {
                                            if k.is(KeyCode::Enter) {
                                                Some(Msg::ConfirmDelete(delete_id))
                                            } else {
                                                None
                                            }
                                        }),
                                ) @ "modal-delete",
                        },
                    },
                }
            };

            ui! {
                ZStack::new() => {
                    main_content,
                    modal,
                }
            }
        } else {
            main_content
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::DraftChanged(ev) => {
                ctx.state.draft.set_text(ev.value.as_ref().to_string());
                ctx.state.draft.set_cursor(ev.cursor);
                Update::full()
            }
            Msg::Add => {
                let text = ctx.state.draft.text().trim().to_string();
                if text.is_empty() {
                    ctx.toast().push(Toast::new("Nothing to add"));
                    return Update::full();
                }

                let id = ctx.state.next_id.max(3);
                ctx.state.next_id = id.saturating_add(1);

                ctx.state.todos.push(TodoItem {
                    id,
                    text: text.clone(),
                    done: false,
                });

                ctx.state.draft.set_text(String::new());
                ctx.state.draft.set_cursor(0);
                ctx.toast().push(Toast::new(format!("Added: {}", text)));
                Update::full()
            }
            Msg::Toggle(id) => {
                if let Some(todo) = ctx.state.todos.iter_mut().find(|t| t.id == id) {
                    todo.done = !todo.done;
                    if todo.done {
                        ctx.toast().push(Toast::new("Marked as done"));
                    } else {
                        ctx.toast().push(Toast::new("Marked as not done"));
                    }
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::RequestDelete(id) => {
                ctx.state.confirm_delete = Some(id);
                Update::full()
            }
            Msg::CancelDelete => {
                ctx.state.confirm_delete = None;
                ctx.toast().push(Toast::new("Delete cancelled"));
                Update::full()
            }
            Msg::ConfirmDelete(id) => {
                let name = ctx
                    .state
                    .todos
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.text.clone())
                    .unwrap_or_default();
                ctx.state.todos.retain(|t| t.id != id);
                ctx.state.confirm_delete = None;

                let max = ctx.state.todos.len().saturating_sub(1);
                ctx.state.scroll = ctx.state.scroll.min(max);

                ctx.toast().push(Toast::new(format!("Deleted: {}", name)));
                Update::full()
            }
            Msg::Scrolled(ev) => {
                let max = ctx.state.todos.len().saturating_sub(1);
                ctx.state.scroll = ev.offset.min(max);
                Update::full()
            }
            Msg::Seeded(mut todos) => {
                if ctx.state.todos.is_empty() {
                    ctx.state.todos.append(&mut todos);
                    ctx.state.next_id = ctx
                        .state
                        .todos
                        .iter()
                        .map(|t| t.id)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1);
                    ctx.toast().push(Toast::new("Loaded initial todos"));
                    Update::full()
                } else {
                    Update::none()
                }
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .toast_placement(ToastPlacement::BottomEnd)
        .toast_gap(1)
        .mount(TodoApp)
        .run()
}
