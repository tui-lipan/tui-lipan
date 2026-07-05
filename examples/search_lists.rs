use tui_lipan::prelude::*;

const FILES: &[&str] = &[
    "Cargo.toml",
    "README.md",
    "src/lib.rs",
    "src/app/mod.rs",
    "src/backend/ratatui_backend/mod.rs",
    "src/widgets/list.rs",
    "src/widgets/frame.rs",
    "src/widgets/input.rs",
    "examples/widgets.rs",
    "examples/todo.rs",
    "docs/ROADMAP.md",
    "docs/DESIGN.md",
];

const BRANCHES: &[&str] = &[
    "main",
    "feat/list-indicators",
    "fix/scrollbar-metrics",
    "feat/search-panels",
    "docs/widgets-roadmap",
    "chore/cleanup",
    "perf/layout-pass",
    "refactor/runtime",
    "feat/input-focus",
];

const TASKS: &[&str] = &[
    "Fix list indicators",
    "Align scrollbar metrics",
    "Wire global search",
    "Style frame headers",
    "Add search example",
    "Test on small terminals",
    "Polish focus behavior",
    "Update documentation",
    "Verify mouse hit-testing",
];

const SEARCH_MIN: u16 = 12;
const SEARCH_MAX: u16 = 28;

struct SearchLists;

#[derive(Default)]
struct State {
    global: TextInput,
    files_query: TextInput,
    branches_query: TextInput,
    tasks_query: TextInput,
    files_selected: usize,
    branches_selected: usize,
    tasks_selected: usize,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    GlobalChanged(InputEvent),
    FilesChanged(InputEvent),
    BranchesChanged(InputEvent),
    TasksChanged(InputEvent),
    FilesSelected(ListEvent),
    FilesScrollTo(usize),
    BranchesSelected(ListEvent),
    BranchesScrollTo(usize),
    TasksSelected(ListEvent),
    TasksScrollTo(usize),
}

impl Component for SearchLists {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            status: "Click a search bar or press / for global search".to_string(),
            ..State::default()
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if matches!(key.code, KeyCode::Char('/')) {
            ctx.request_focus("global-search");
            return KeyUpdate::handled(Update::none());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let global_query = ctx.state.global.text();
        let files = filter_items(FILES, global_query, ctx.state.files_query.text());
        let branches = filter_items(BRANCHES, global_query, ctx.state.branches_query.text());
        let tasks = filter_items(TASKS, global_query, ctx.state.tasks_query.text());

        let files_selected = clamp_selected(ctx.state.files_selected, files.len());
        let branches_selected = clamp_selected(ctx.state.branches_selected, branches.len());
        let tasks_selected = clamp_selected(ctx.state.tasks_selected, tasks.len());

        let files_active = ctx.has_focus_within_key("files-panel");
        let branches_active = ctx.has_focus_within_key("branches-panel");
        let tasks_active = ctx.has_focus_within_key("tasks-panel");

        let border_style = |active: bool| {
            if active {
                BorderStyle::Thick
            } else {
                BorderStyle::Rounded
            }
        };

        let active_style = |active: bool| {
            if active {
                Style::new().fg(Color::LightBlue)
            } else {
                Style::new().fg(Color::DarkGray)
            }
        };

        let global_style = Style::new().fg(Color::LightBlue).bold();

        rsx! {
            Frame {
                status: ctx.state.status.clone(),
                border: true,
                border_style: BorderStyle::Rounded,
                padding: (0, 1, 1, 1),
                header: rsx! {
                    HStack {
                        gap: 1,
                        Text {
                            content: "Search Lists",
                            style: global_style,
                        },
                        Input {
                            value: ctx.state.global.text().to_owned(),
                            cursor: ctx.state.global.cursor(),
                            placeholder: "Global search (/)".to_owned(),
                            prefix: "[",
                            suffix: "]",
                            focus_prefix_style: Style::new().fg(Color::White),
                            focus_suffix_style: Style::new().fg(Color::White),
                            focus_style: Style::new().fg(Color::LightBlue),
                            placeholder_style: Style::new().fg(Color::DarkGray).dim(),
                            focus_placeholder_style: Style::new().fg(Color::White),
                            truncate_head: true,
                            padding: 0,
                            width: Length::Auto,
                            min_width: Length::Px(SEARCH_MIN),
                            max_width: Length::Px(SEARCH_MAX),
                            border: false,
                            on_change: ctx.link().callback(Msg::GlobalChanged),
                            key: "global-search",
                        },
                    }
                },
                HStack {
                    gap: 1,
                    Frame {
                        border: true,
                        border_style: border_style(files_active),
                        style: active_style(files_active),
                        padding: (0, 1, 1, 1),
                        header: rsx! {
                            Input {
                                value: ctx.state.files_query.text().to_owned(),
                                cursor: ctx.state.files_query.cursor(),
                                placeholder: "Search files".to_owned(),
                                prefix: "[",
                                suffix: "]",
                                focus_prefix_style: Style::new().fg(Color::White),
                                focus_suffix_style: Style::new().fg(Color::White),
                                focus_style: Style::new().fg(Color::LightBlue),
                                placeholder_style: Style::new().fg(Color::DarkGray).dim(),
                                focus_placeholder_style: Style::new().fg(Color::White),
                                truncate_head: true,
                                padding: 0,
                                width: Length::Auto,
                                min_width: Length::Px(SEARCH_MIN),
                                max_width: Length::Px(SEARCH_MAX),
                                border: false,
                                on_change: ctx.link().callback(Msg::FilesChanged),
                                key: "files-search",
                            }
                        },
                        key: "files-panel",
                        List {
                            border: true,
                            scrollbar: true,
                            scrollbar_config: ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            show_scroll_indicators: true,
                            empty_text: "No files found",
                            items: files.iter().map(|s| ListItem::new(*s)).collect::<Vec<_>>(),
                            selected: files_selected,
                            on_select: ctx.link().callback(Msg::FilesSelected),
                            on_scroll_to: ctx.link().callback(Msg::FilesScrollTo),
                            active_symbol: Some("> "),
                            key: "files-list",
                        },
                    },
                    Frame {
                        border: true,
                        border_style: border_style(branches_active),
                        style: active_style(branches_active),
                        padding: (0, 1, 1, 1),
                        header: rsx! {
                            Input {
                                value: ctx.state.branches_query.text().to_owned(),
                                cursor: ctx.state.branches_query.cursor(),
                                placeholder: "Search branches".to_owned(),
                                prefix: "[",
                                suffix: "]",
                                focus_prefix_style: Style::new().fg(Color::White),
                                focus_suffix_style: Style::new().fg(Color::White),
                                focus_style: Style::new().fg(Color::LightBlue),
                                placeholder_style: Style::new().fg(Color::DarkGray).dim(),
                                focus_placeholder_style: Style::new().fg(Color::White),
                                truncate_head: true,
                                padding: 0,
                                width: Length::Auto,
                                min_width: Length::Px(SEARCH_MIN),
                                max_width: Length::Px(SEARCH_MAX),
                                border: false,
                                on_change: ctx.link().callback(Msg::BranchesChanged),
                                key: "branches-search",
                            }
                        },
                        key: "branches-panel",
                        List {
                            border: true,
                            scrollbar: true,
                            scrollbar_config: ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            show_scroll_indicators: false,
                            empty_text: "No branches found",
                            items: branches.iter().map(|s| ListItem::new(*s)).collect::<Vec<_>>(),
                            selected: branches_selected,
                            on_select: ctx.link().callback(Msg::BranchesSelected),
                            on_scroll_to: ctx.link().callback(Msg::BranchesScrollTo),
                            key: "branches-list",
                        },
                    },
                    Frame {
                        border: true,
                        border_style: border_style(tasks_active),
                        style: active_style(tasks_active),
                        padding: (0, 1, 1, 1),
                        header: rsx! {
                            Input {
                                value: ctx.state.tasks_query.text().to_owned(),
                                cursor: ctx.state.tasks_query.cursor(),
                                placeholder: "Search tasks".to_owned(),
                                prefix: "[",
                                suffix: "]",
                                focus_prefix_style: Style::new().fg(Color::White),
                                focus_suffix_style: Style::new().fg(Color::White),
                                focus_style: Style::new().fg(Color::LightBlue),
                                placeholder_style: Style::new().fg(Color::DarkGray).dim(),
                                focus_placeholder_style: Style::new().fg(Color::White),
                                truncate_head: true,
                                padding: 0,
                                width: Length::Auto,
                                min_width: Length::Px(SEARCH_MIN),
                                max_width: Length::Px(SEARCH_MAX),
                                border: false,
                                on_change: ctx.link().callback(Msg::TasksChanged),
                                key: "tasks-search",
                            }
                        },
                        key: "tasks-panel",
                        List {
                            border: true,
                            scrollbar: true,
                            scrollbar_config: ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            show_scroll_indicators: true,
                            empty_text: "No tasks found",
                            items: tasks.iter().map(|s| ListItem::new(*s)).collect::<Vec<_>>(),
                            selected: tasks_selected,
                            on_select: ctx.link().callback(Msg::TasksSelected),
                            on_scroll_to: ctx.link().callback(Msg::TasksScrollTo),
                            key: "tasks-list",
                        },
                    },
                },
            }
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::GlobalChanged(ev) => {
                ctx.state.global.set_text(ev.value.as_ref().to_string());
                ctx.state.global.set_cursor(ev.cursor);
                ctx.state.files_selected = 0;
                ctx.state.branches_selected = 0;
                ctx.state.tasks_selected = 0;
                Update::full()
            }
            Msg::FilesChanged(ev) => {
                ctx.state
                    .files_query
                    .set_text(ev.value.as_ref().to_string());
                ctx.state.files_query.set_cursor(ev.cursor);
                ctx.state.files_selected = 0;
                Update::full()
            }
            Msg::BranchesChanged(ev) => {
                ctx.state
                    .branches_query
                    .set_text(ev.value.as_ref().to_string());
                ctx.state.branches_query.set_cursor(ev.cursor);
                ctx.state.branches_selected = 0;
                Update::full()
            }
            Msg::TasksChanged(ev) => {
                ctx.state
                    .tasks_query
                    .set_text(ev.value.as_ref().to_string());
                ctx.state.tasks_query.set_cursor(ev.cursor);
                ctx.state.tasks_selected = 0;
                Update::full()
            }
            Msg::FilesSelected(ev) => {
                ctx.state.files_selected = ev.index;
                Update::full()
            }
            Msg::FilesScrollTo(index) => {
                ctx.state.files_selected = index;
                Update::full()
            }
            Msg::BranchesSelected(ev) => {
                ctx.state.branches_selected = ev.index;
                Update::full()
            }
            Msg::BranchesScrollTo(index) => {
                ctx.state.branches_selected = index;
                Update::full()
            }
            Msg::TasksSelected(ev) => {
                ctx.state.tasks_selected = ev.index;
                Update::full()
            }
            Msg::TasksScrollTo(index) => {
                ctx.state.tasks_selected = index;
                Update::full()
            }
        }
    }
}

fn clamp_selected(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len.saturating_sub(1))
    }
}

fn filter_items<'a>(items: &'a [&'a str], global: &str, local: &str) -> Vec<&'a str> {
    let global = global.trim().to_lowercase();
    let local = local.trim().to_lowercase();

    items
        .iter()
        .copied()
        .filter(|item| {
            let hay = item.to_lowercase();
            (global.is_empty() || hay.contains(&global))
                && (local.is_empty() || hay.contains(&local))
        })
        .collect()
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Search Lists")
        .mount(SearchLists)
        .run()
}
