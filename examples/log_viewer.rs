use std::fs;
use std::sync::Arc;
use tui_lipan::prelude::*;

struct LogViewer;

struct State {
    entries: Arc<[LogEntry]>,
    filter: String,
    filter_cursor: usize,
    filter_anchor: Option<usize>,
    auto_follow: bool,
    paused: bool,
    filter_mode: LogFilterMode,
    selected_index: usize,
    selected_log: Option<LogEntry>,
}

#[derive(Clone, Debug)]
enum Msg {
    FilterChanged(InputEvent),
    ToggleAutoFollow,
    TogglePaused,
    CycleFilterMode,
    LogSelected(LogViewEvent),
    ClearFilter,
}

impl Component for LogViewer {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let log_path = "examples/assets/sample.log";
        let entries = match fs::read_to_string(log_path) {
            Ok(content) => content
                .lines()
                .map(|line| {
                    let level = if line.to_uppercase().contains("ERROR")
                        || line.to_uppercase().contains("FAIL")
                    {
                        LogLevel::Error
                    } else if line.to_uppercase().contains("WARN") {
                        LogLevel::Warn
                    } else if line.to_uppercase().contains("DEBUG") {
                        LogLevel::Debug
                    } else if line.to_uppercase().contains("TRACE") {
                        LogLevel::Trace
                    } else {
                        LogLevel::Info
                    };
                    LogEntry::new(level, line)
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![LogEntry::error(
                "Failed to load examples/assets/sample.log. Make sure it exists!",
            )],
        };

        State {
            entries: entries.into(),
            filter: String::new(),
            filter_cursor: 0,
            filter_anchor: None,
            auto_follow: false,
            paused: false,
            filter_mode: LogFilterMode::Fuzzy,
            selected_index: 0,
            selected_log: None,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FilterChanged(ev) => {
                ctx.state.filter = ev.value.to_string();
                ctx.state.filter_cursor = ev.cursor;
                ctx.state.filter_anchor = ev.anchor;
                Update::full()
            }
            Msg::ToggleAutoFollow => {
                ctx.state.auto_follow = !ctx.state.auto_follow;
                Update::full()
            }
            Msg::TogglePaused => {
                ctx.state.paused = !ctx.state.paused;
                Update::full()
            }
            Msg::CycleFilterMode => {
                ctx.state.filter_mode = match ctx.state.filter_mode {
                    LogFilterMode::Fuzzy => LogFilterMode::Substring,
                    LogFilterMode::Substring => LogFilterMode::Exact,
                    LogFilterMode::Exact => LogFilterMode::Fuzzy,
                };
                Update::full()
            }
            Msg::LogSelected(ev) => {
                ctx.state.selected_log = Some(ev.entry);
                ctx.state.selected_index = ev.visible_index;
                Update::full()
            }
            Msg::ClearFilter => {
                ctx.state.filter.clear();
                ctx.state.filter_cursor = 0;
                ctx.state.filter_anchor = None;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.code == KeyCode::Char('q') {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let filter_text = ctx.state.filter.clone();

        let log_view = LogView::new()
            .entries_arc(ctx.state.entries.clone())
            .filter(filter_text)
            .filter_mode(ctx.state.filter_mode)
            .auto_follow(ctx.state.auto_follow)
            .paused(ctx.state.paused)
            .selected(ctx.state.selected_index)
            .border(true)
            .border_style(BorderStyle::Rounded)
            .scrollbar(true)
            .on_select(ctx.link().callback(Msg::LogSelected));

        let details = if let Some(entry) = &ctx.state.selected_log {
            // Simple parsing of "Month Day Time Hostname Service[PID]: Message"
            let parts: Vec<&str> = entry.message.splitn(5, ' ').collect();
            if parts.len() >= 5 {
                let timestamp = format!("{} {} {}", parts[0], parts[1], parts[2]);
                let hostname = parts[3];
                let rest = parts[4];

                let (service_info, message) = if let Some(idx) = rest.find(": ") {
                    (&rest[..idx], &rest[idx + 2..])
                } else {
                    ("Unknown", rest)
                };

                let (service, pid) = if let Some(start) = service_info.find('[')
                    && let Some(end) = service_info.find(']')
                {
                    (&service_info[..start], &service_info[start + 1..end])
                } else {
                    (service_info, "N/A")
                };

                format!(
                    "TIMESTAMP: {}\nHOSTNAME:  {}\nSERVICE:   {}\nPID:       {}\nLEVEL:     {}\n\nMESSAGE:\n{}",
                    timestamp,
                    hostname,
                    service,
                    pid,
                    entry.level.label(),
                    message
                )
            } else {
                format!("LEVEL: {}\nRAW:   {}", entry.level.label(), entry.message)
            }
        } else {
            "Select a log line to see details".to_string()
        };

        rsx! {
            VStack {
                padding: 1,
                gap: 1,
                Frame {
                    title: "Log Filter",
                    border: true,
                    height: Length::Auto,
                    HStack {
                        gap: 2,
                        Input {
                            value: ctx.state.filter.clone(),
                            cursor: ctx.state.filter_cursor,
                            anchor: ctx.state.filter_anchor,
                            placeholder: "Search logs (regex supported)...",
                            prefix: "🔍 ",
                            on_change: ctx.link().callback(Msg::FilterChanged),
                            width: Length::Flex(1),
                        },
                        Button {
                            label: "Clear",
                            on_click: ctx.link().callback(|_| Msg::ClearFilter),
                        },
                    },
                },
                HStack {
                    gap: 1,
                    height: Length::Auto,
                    Button {
                        label: ctx.state.filter_mode.label(),
                        on_click: ctx.link().callback(|_| Msg::CycleFilterMode),
                    },
                    Button {
                        label: if ctx.state.auto_follow { "Auto-Follow: ON" } else { "Auto-Follow: OFF" },
                        on_click: ctx.link().callback(|_| Msg::ToggleAutoFollow),
                    },
                    Button {
                        label: if ctx.state.paused { "Paused: ON" } else { "Paused: OFF" },
                        on_click: ctx.link().callback(|_| Msg::TogglePaused),
                    },
                    Text::new(format!(" Total: {} lines", ctx.state.entries.len()))
                        .style(Style::new().dim()),
                },
                log_view,
                Frame {
                    title: "Log Details",
                    border: true,
                    height: Length::Px(10),
                    TextArea::new(details).border(false).read_only(true).wrap(true),
                },
                StatusBar::new()
                    .left(
                        Text::new(" q: quit | arrows: navigate | click: select ")
                            .style(Style::new().bold()),
                    )
                    .style(Style::new().bg(Color::DarkGray).fg(Color::White))
                    .height(Length::Auto),
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - LogView Example")
        .mount(LogViewer)
        .run()
}
