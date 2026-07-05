/// ScrollView repro closer to the opencode session screen.
///
/// Usage:
///   cargo run --example scroll_view_opencode_repro --features "markdown diff-view"
///
/// The message timeline and composer exercise the same scroll/edit paths as the
/// real app. Sidebar behavior matches opencode
/// session: at width > 120 the sidebar is shown automatically (`Auto`); narrower
/// terminals use a `ZStack` overlay when the sidebar is visible. Press `s` to
/// cycle hidden vs forced-on (same as opencode's `ToggleSidebar`). Press `i` to
/// toggle smooth wheel scrolling on the main timeline. `q` quits.
/// DiffView: `n` context, `w` wrap, `m` split/unified, `e` sep, `h` hide/show `diff --git` line on patch diffs.
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tui_lipan::prelude::*;

const MESSAGE_PAIR_COUNT: usize = 90;
const STREAM_TICK_MS: u64 = 180;
const STREAM_STEP_COUNT: usize = 14;
/// Same breakpoint as `opencode-tui` session screen (`ctx.viewport().w > 120`).
const SIDEBAR_WIDE_BREAKPOINT: u16 = 120;

struct OpencodeScrollRepro;

struct State {
    offset: usize,
    visible: usize,
    max_offset: usize,
    messages: Vec<DemoMessage>,
    sidebar_mode: SidebarMode,
    sidebar_open: bool,
    smooth_wheel_scroll: bool,
    active_preset: usize,
    theme_provider_on: bool,
    syntax_on: bool,
    show_search: bool,
    show_theme_picker: bool,
    selected_search: Option<Arc<str>>,
    theme_index: usize,
    original_theme_index: Option<usize>,
    diff_context_cycle: u8,
    diff_wrap: bool,
    diff_split: bool,
    diff_show_context_separator: bool,
    diff_show_git_header: bool,
    next_stream_id: u64,
    active_stream_id: Option<u64>,
    stream_step: usize,
    stream_follow_bottom: bool,
    input: TextEditor,
}

#[derive(Clone, Copy)]
struct DiffRuntime {
    context_cycle: u8,
    wrap: bool,
    split: bool,
    show_context_separator: bool,
    show_git_header: bool,
}

fn diff_context_from_cycle(cycle: u8) -> Option<usize> {
    match cycle % 7 {
        0 => None,
        n => Some(usize::from(n - 1)),
    }
}

fn diff_context_cycle_label(cycle: u8) -> &'static str {
    match cycle % 7 {
        0 => "full",
        1 => "0",
        2 => "1",
        3 => "2",
        4 => "3",
        5 => "4",
        _ => "5",
    }
}

fn patch_text_without_git_header_line(patch: &Arc<str>, show_git_header: bool) -> Arc<str> {
    if show_git_header {
        return Arc::clone(patch);
    }
    let mut out = patch
        .lines()
        .filter(|line| !line.starts_with("diff --git"))
        .collect::<Vec<_>>()
        .join("\n");
    if patch.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    Arc::from(out)
}

#[derive(Clone, Debug)]
enum Msg {
    ScrollChanged(ScrollEvent),
    SetPreset(usize),
    ToggleSearch(bool),
    ToggleThemePicker(bool),
    SearchActivated(SearchEvent<Arc<str>>),
    ThemePreview(usize),
    ThemePreviewQuery(Arc<str>),
    ThemeActivated(usize),
    InputChanged(TextAreaEvent),
    StartStreaming,
    StreamTick { stream_id: u64, step: usize },
    StopStreaming,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarMode {
    Auto,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReproPreset {
    Mixed,
    DiffHeavy,
    PatchStack,
    TextHeavy,
    ErrorHeavy,
    DiffTest,
    DiffPatch,
}

impl ReproPreset {
    const ALL: [Self; 7] = [
        Self::Mixed,
        Self::DiffHeavy,
        Self::PatchStack,
        Self::TextHeavy,
        Self::ErrorHeavy,
        Self::DiffTest,
        Self::DiffPatch,
    ];

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Mixed)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mixed => "Mixed",
            Self::DiffHeavy => "Diff Heavy",
            Self::PatchStack => "Patch Stack",
            Self::TextHeavy => "Text Heavy",
            Self::ErrorHeavy => "Error Heavy",
            Self::DiffTest => "Diff Test",
            Self::DiffPatch => "Patch Diff",
        }
    }

    fn sidebar_note(self) -> &'static str {
        match self {
            Self::Mixed => {
                "Baseline mixed session: markdown, diffs, diagnostics, tasks, todos, and generic tool blocks."
            }
            Self::DiffHeavy => {
                "High DiffView density with split + unified surfaces per assistant row. Useful for isolating diff layout/render cost."
            }
            Self::PatchStack => {
                "Nested multi-block patch stacks: several diffs inside one tool panel, closer to heavy apply_patch output."
            }
            Self::TextHeavy => {
                "Mostly Text widgets with multiline output, metadata, warnings, and inline rows. Useful to isolate Text behavior."
            }
            Self::ErrorHeavy => {
                "Assistant errors, diagnostics, and multiline failure output. Good for resize/scroll issues caused by noisy error blocks."
            }
            Self::DiffTest => {
                "Single assistant message: two large file diffs (scroll_metrics, dirty_pipeline) plus short A/B/C. For DiffView testing."
            }
            Self::DiffPatch => {
                "Three DiffView::from_patch(unified diff text) tools only. Compare with Diff Test / Diff Heavy."
            }
        }
    }
}

#[derive(Clone)]
enum DemoMessage {
    User {
        id: String,
        text: Arc<str>,
        files: Vec<DemoFile>,
        timestamp: Arc<str>,
        queued: bool,
        has_compaction: bool,
    },
    Assistant {
        id: String,
        parts: Vec<AssistantPart>,
        mode: Arc<str>,
        model: Arc<str>,
        duration: Arc<str>,
        interrupted: bool,
        error: Option<Arc<str>>,
    },
}

#[derive(Clone)]
struct DemoFile {
    label: Arc<str>,
    kind: DemoFileKind,
}

#[derive(Clone, Copy)]
enum DemoFileKind {
    File,
    Image,
    Pdf,
    Dir,
}

#[derive(Clone)]
enum AssistantPart {
    Markdown(Arc<str>),
    Reasoning(Arc<str>),
    Tool(DemoToolCall),
    Subtask {
        description: Arc<str>,
        toolcalls: usize,
        current_tool: Option<Arc<str>>,
        key_hint: Option<Arc<str>>,
        failed: bool,
    },
    Agent(Arc<str>),
    Retry {
        attempt: usize,
    },
}

#[derive(Clone)]
enum DemoToolCall {
    BashBlock {
        title: Arc<str>,
        command: Arc<str>,
        output: Arc<str>,
        running: bool,
    },
    Inline {
        icon: Arc<str>,
        pending: Arc<str>,
        content: Arc<str>,
        complete: bool,
        error: Option<Arc<str>>,
        highlight: InlineHighlight,
    },
    Read {
        path: Arc<str>,
        suffix: Arc<str>,
        loaded: Vec<Arc<str>>,
    },
    Diff {
        title: Arc<str>,
        before: Arc<str>,
        after: Arc<str>,
        #[allow(dead_code)]
        wrap: bool,
        #[allow(dead_code)]
        split: bool,
        /// When set, [`DiffView::context_lines`]; `None` shows full files.
        #[allow(dead_code)]
        context_lines: Option<usize>,
    },
    DiffFromPatch {
        title: Arc<str>,
        patch: Arc<str>,
    },
    Todo {
        items: Vec<DemoTodo>,
    },
    Questions {
        items: Vec<DemoQuestion>,
    },
    Task {
        title: Arc<str>,
        description: Arc<str>,
        toolcalls: usize,
        current_tool: Option<Arc<str>>,
        key_hint: Option<Arc<str>>,
        running: bool,
        failed: bool,
    },
    Diagnostics {
        title: Arc<str>,
        content: Arc<str>,
        diagnostics: Vec<DemoDiagnostic>,
    },
    Generic {
        title: Arc<str>,
        output: Arc<str>,
    },
}

#[derive(Clone, Copy)]
enum InlineHighlight {
    Normal,
    Warning,
    Denied,
}

#[derive(Clone)]
struct DemoTodo {
    content: Arc<str>,
    status: DemoTodoStatus,
}

#[derive(Clone, Copy)]
enum DemoTodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone)]
struct DemoQuestion {
    question: Arc<str>,
    answer: Arc<str>,
}

#[derive(Clone)]
struct DemoDiagnostic {
    line: usize,
    col: usize,
    message: Arc<str>,
}

struct DemoPalette {
    panel_bg: Color,
    input_bg: Color,
    text: Color,
    muted: Color,
    dim: Color,
    accent: Color,
    subtle: Color,
    warn: Color,
    error: Color,
}

impl Component for OpencodeScrollRepro {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            offset: 0,
            visible: 1,
            max_offset: 0,
            messages: build_messages_for_preset(ReproPreset::Mixed),
            sidebar_mode: SidebarMode::Auto,
            sidebar_open: false,
            smooth_wheel_scroll: true,
            active_preset: 0,
            theme_provider_on: true,
            syntax_on: true,
            show_search: false,
            show_theme_picker: false,
            selected_search: None,
            theme_index: 1,
            original_theme_index: None,
            diff_context_cycle: 0,
            diff_wrap: true,
            diff_split: true,
            diff_show_context_separator: true,
            diff_show_git_header: true,
            next_stream_id: 0,
            active_stream_id: None,
            stream_step: 0,
            stream_follow_bottom: false,
            input: TextEditor::new(""),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ScrollChanged(ev) => {
                ctx.state.offset = ev.offset.min(ev.metrics.max_offset);
                ctx.state.visible = ev.metrics.visible.max(1);
                ctx.state.max_offset = ev.metrics.max_offset;
                if ctx.state.active_stream_id.is_some() {
                    ctx.state.stream_follow_bottom =
                        ev.metrics.max_offset == 0 || ev.offset >= ev.metrics.max_offset;
                }
                // The ScrollView handler has already updated the node-local
                // offset and requested a layout-only redraw.  Rebuilding the
                // whole opencode-like transcript on every wheel tick dominates
                // fast-scroll CPU, so keep state in sync for the next full
                // render without promoting this hot path to a root rebuild.
                Update::none()
            }
            Msg::SetPreset(index) => {
                let next = index.min(ReproPreset::ALL.len().saturating_sub(1));
                ctx.state.active_preset = next;
                ctx.state.messages = build_messages_for_preset(ReproPreset::from_index(next));
                ctx.state.offset = 0;
                ctx.state.max_offset = 0;
                ctx.state.active_stream_id = None;
                ctx.state.stream_step = 0;
                ctx.state.stream_follow_bottom = false;
                Update::full()
            }
            Msg::ToggleSearch(show) => {
                ctx.state.show_search = show;
                Update::full()
            }
            Msg::ToggleThemePicker(show) => {
                if show {
                    ctx.state.original_theme_index = Some(ctx.state.theme_index);
                } else if let Some(original) = ctx.state.original_theme_index.take() {
                    ctx.state.theme_index = original;
                }
                ctx.state.show_theme_picker = show;
                Update::full()
            }
            Msg::SearchActivated(event) => {
                ctx.state.selected_search = Some(event.item.value);
                ctx.state.show_search = false;
                Update::full()
            }
            Msg::ThemePreview(index) => {
                if ctx.state.theme_index != index {
                    ctx.state.theme_index = index;
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::ThemePreviewQuery(query) => {
                if let Some(index) = first_theme_match(query.as_ref()) {
                    if ctx.state.theme_index != index {
                        ctx.state.theme_index = index;
                        return Update::full();
                    }
                } else if let Some(original) = ctx.state.original_theme_index
                    && ctx.state.theme_index != original
                {
                    ctx.state.theme_index = original;
                    return Update::full();
                }
                Update::none()
            }
            Msg::ThemeActivated(index) => {
                ctx.state.theme_index = index;
                ctx.state.original_theme_index = None;
                ctx.state.show_theme_picker = false;
                Update::full()
            }
            Msg::InputChanged(ev) => {
                ev.apply_to(&mut ctx.state.input);
                Update::full()
            }
            Msg::StartStreaming => {
                if ctx.state.active_stream_id.is_some() {
                    return Update::none();
                }

                let stream_id = ctx.state.next_stream_id.saturating_add(1);
                ctx.state.next_stream_id = stream_id;
                ctx.state.active_stream_id = Some(stream_id);
                ctx.state.stream_step = 0;
                ctx.state.stream_follow_bottom =
                    ctx.state.max_offset == 0 || ctx.state.offset >= ctx.state.max_offset;

                let prompt = ctx.state.input.text().trim().to_string();
                let prompt = if prompt.is_empty() {
                    "Patch the ScrollView anchor so streaming appends stay stable while I read earlier output."
                        .to_string()
                } else {
                    ctx.state.input.clear();
                    prompt
                };

                append_streaming_exchange(&mut ctx.state.messages, stream_id, &prompt);
                let command = ctx.link().command(move |link| {
                    thread::sleep(Duration::from_millis(STREAM_TICK_MS));
                    link.send(Msg::StreamTick { stream_id, step: 0 });
                });
                Update::with_command(command)
            }
            Msg::StreamTick { stream_id, step } => {
                if ctx.state.active_stream_id != Some(stream_id) {
                    return Update::none();
                }

                apply_streaming_step(&mut ctx.state.messages, stream_id, step);
                ctx.state.stream_step = step.saturating_add(1);
                if ctx.state.stream_step >= STREAM_STEP_COUNT {
                    finish_streaming_exchange(&mut ctx.state.messages, stream_id, false);
                    ctx.state.active_stream_id = None;
                    ctx.state.stream_follow_bottom = false;
                    return Update::full();
                }

                let next_step = ctx.state.stream_step;
                let command = ctx.link().command(move |link| {
                    thread::sleep(Duration::from_millis(STREAM_TICK_MS));
                    link.send(Msg::StreamTick {
                        stream_id,
                        step: next_step,
                    });
                });
                Update::with_command(command)
            }
            Msg::StopStreaming => {
                let Some(stream_id) = ctx.state.active_stream_id.take() else {
                    return Update::none();
                };
                finish_streaming_exchange(&mut ctx.state.messages, stream_id, true);
                ctx.state.stream_follow_bottom = false;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                if ctx.state.show_search {
                    ctx.state.show_search = false;
                    KeyUpdate::handled(Update::full())
                } else if ctx.state.show_theme_picker {
                    if let Some(original) = ctx.state.original_theme_index.take() {
                        ctx.state.theme_index = original;
                    }
                    ctx.state.show_theme_picker = false;
                    KeyUpdate::handled(Update::full())
                } else {
                    ctx.quit();
                    KeyUpdate::handled(Update::none())
                }
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                let wide = ctx.viewport().w > SIDEBAR_WIDE_BREAKPOINT;
                let is_visible = ctx.state.sidebar_open
                    || (matches!(ctx.state.sidebar_mode, SidebarMode::Auto) && wide);
                ctx.state.sidebar_mode = if is_visible {
                    SidebarMode::Hide
                } else {
                    SidebarMode::Auto
                };
                ctx.state.sidebar_open = !is_visible;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('i') | KeyCode::Char('I') => {
                ctx.state.smooth_wheel_scroll = !ctx.state.smooth_wheel_scroll;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                ctx.state.theme_provider_on = !ctx.state.theme_provider_on;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('g') | KeyCode::Char('G') if !ctx.state.show_theme_picker => {
                ctx.state.original_theme_index = Some(ctx.state.theme_index);
                ctx.state.show_theme_picker = true;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                ctx.state.syntax_on = !ctx.state.syntax_on;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                ctx.state.diff_context_cycle = (ctx.state.diff_context_cycle + 1) % 7;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                ctx.state.diff_wrap = !ctx.state.diff_wrap;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                ctx.state.diff_split = !ctx.state.diff_split;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                ctx.state.diff_show_context_separator = !ctx.state.diff_show_context_separator;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                ctx.state.diff_show_git_header = !ctx.state.diff_show_git_header;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                KeyUpdate::handled(self.update(Msg::StartStreaming, ctx))
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                KeyUpdate::handled(self.update(Msg::StopStreaming, ctx))
            }
            KeyCode::Char('/') if !ctx.state.show_search => {
                ctx.state.show_search = true;
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let preset = ReproPreset::from_index(ctx.state.active_preset);
        let messages = &ctx.state.messages;
        let active_theme = selected_theme(ctx.state.theme_index);
        let current_theme_name = BUILTIN_THEMES
            .get(ctx.state.theme_index)
            .map(|t| t.0)
            .unwrap_or(BUILTIN_THEMES[0].0);
        let bg = active_theme
            .primary
            .bg
            .map(|paint| paint.color())
            .unwrap_or(Color::Black);
        let panel_bg = active_theme
            .primary
            .bg
            .map(|paint| paint.color())
            .unwrap_or(Color::Black)
            .lighten_by(0.04);
        let input_bg = active_theme
            .primary
            .bg
            .map(|paint| paint.color())
            .unwrap_or(Color::Black)
            .lighten_by(0.02);
        let text = active_theme
            .primary
            .fg
            .map(|paint| paint.color())
            .unwrap_or(Color::White);
        let muted = active_theme
            .muted
            .fg
            .map(|paint| paint.color())
            .unwrap_or(text.blend_toward(bg, 0.50));
        let dim = active_theme
            .border
            .fg
            .map(|paint| paint.color())
            .unwrap_or(text.blend_toward(bg, 0.65));
        let accent = active_theme
            .accent
            .fg
            .or(active_theme.selection.fg)
            .map(|paint| paint.color())
            .unwrap_or(text);
        let subtle = active_theme
            .border
            .fg
            .map(|paint| paint.color())
            .unwrap_or(text.blend_toward(bg, 0.40));
        let warn = active_theme
            .syntax
            .number
            .fg
            .map(|paint| paint.color())
            .unwrap_or(accent.blend_toward(Color::Yellow, 0.50));
        let error = active_theme
            .diff
            .removed_marker
            .fg
            .map(|paint| paint.color())
            .unwrap_or(accent.blend_toward(Color::Red, 0.55));
        let palette = DemoPalette {
            panel_bg,
            input_bg,
            text,
            muted,
            dim,
            accent,
            subtle,
            warn,
            error,
        };

        let viewport_w = ctx.viewport().w;
        let wide = viewport_w > SIDEBAR_WIDE_BREAKPOINT;
        let sidebar_visible =
            ctx.state.sidebar_open || (matches!(ctx.state.sidebar_mode, SidebarMode::Auto) && wide);
        let sidebar_layout = if !sidebar_visible {
            "off"
        } else if wide {
            "docked"
        } else {
            "overlay"
        };
        let diff_rt = DiffRuntime {
            context_cycle: ctx.state.diff_context_cycle,
            wrap: ctx.state.diff_wrap,
            split: ctx.state.diff_split,
            show_context_separator: ctx.state.diff_show_context_separator,
            show_git_header: ctx.state.diff_show_git_header,
        };
        let streaming_status = if ctx.state.active_stream_id.is_some() {
            format!(
                "stream {}/{} {}",
                ctx.state.stream_step.min(STREAM_STEP_COUNT),
                STREAM_STEP_COUNT,
                if ctx.state.stream_follow_bottom {
                    "tail"
                } else {
                    "anchored"
                }
            )
        } else {
            "stream idle".to_string()
        };
        let status = format!(
            "{} rows | {} | w={} {} | sidebar {} {} | wheel {} | {} | theme {} ({}) | syntax {} | diff ctx {} wrap {} {} sep {} git {} | offset {} / {} | visible {} | r stream | x stop | s sidebar | i wheel | g themes | n/w/m/e/h diff | q quit",
            messages.len(),
            preset.label(),
            viewport_w,
            if wide { "wide" } else { "narrow" },
            match ctx.state.sidebar_mode {
                SidebarMode::Auto => "auto",
                SidebarMode::Hide => "hide",
            },
            sidebar_layout,
            if ctx.state.smooth_wheel_scroll {
                "smooth"
            } else {
                "immediate"
            },
            streaming_status,
            if ctx.state.theme_provider_on {
                "on"
            } else {
                "off"
            },
            current_theme_name,
            if ctx.state.syntax_on { "on" } else { "off" },
            diff_context_cycle_label(ctx.state.diff_context_cycle),
            if ctx.state.diff_wrap { "on" } else { "off" },
            if ctx.state.diff_split {
                "split"
            } else {
                "unified"
            },
            if ctx.state.diff_show_context_separator {
                "on"
            } else {
                "off"
            },
            if ctx.state.diff_show_git_header {
                "on"
            } else {
                "off"
            },
            ctx.state.offset,
            ctx.state.max_offset,
            ctx.state.visible,
        );

        let timeline = ScrollView::new()
            .border(false)
            .scrollbar(true)
            .scrollbar_config(
                ScrollbarConfig::new()
                    .track_style(Style::new().bg(panel_bg))
                    .gap(1),
            )
            .padding(1)
            .gap(1)
            .offset(if ctx.state.stream_follow_bottom {
                usize::MAX
            } else {
                ctx.state.offset
            })
            .scroll_keys(ScrollKeymap::DEFAULT)
            .ambient_page_scroll(true)
            .scroll_wheel_behavior(if ctx.state.smooth_wheel_scroll {
                ScrollWheelBehavior::smooth(ScrollWheelConfig::new(56.0, 10.0, 360.0, 0.05))
            } else {
                ScrollWheelBehavior::immediate()
            })
            .on_scroll(ctx.link().callback(Msg::ScrollChanged))
            .children(messages.iter().map(|message| {
                render_message(
                    message,
                    &palette,
                    ctx.state.syntax_on,
                    ctx.state.theme_index,
                    diff_rt,
                )
            }))
            .key("opencode-scroll-timeline");

        let main_content: Element = rsx! {
            VStack {
                gap: 1,
                padding: (1, 1),
                style: Style::new().bg(bg),
                Frame {
                    border: true,
                    height: Length::Auto,
                    border_style: BorderStyle::Rounded,
                    style: Style::new().bg(panel_bg),
                    VStack {
                        height: Length::Auto,
                        gap: 1,
                        HStack {
                            height: Length::Px(1),
                            justify: Justify::SpaceBetween,
                            Text::from_spans(
                                vec![
                                    Span::new("session ").fg(muted), Span::new("opencode scroll repro").fg(text)
                                    .bold(),
                                ],
                            ),
                            Text::from_spans(
                                vec![
                                    Span::new("model ").fg(dim), Span::new("gpt-5.4").fg(accent),
                                    Span::new("  provider ").fg(dim), Span::new("openai").fg(text),
                                ],
                            ),
                        },
                        Tabs {
                            border: true,
                            border_style: BorderStyle::Rounded,
                            active: ctx.state.active_preset,
                            height: Length::Px(3),
                            on_change: ctx.link().callback(|e: TabsEvent| Msg::SetPreset(e.index)),
                            tab: "Mixed",
                            tab: "Diff Heavy",
                            tab: "Patch Stack",
                            tab: "Text Heavy",
                            tab: "Error Heavy",
                            tab: "Diff Test",
                            tab: "Patch Diff",
                        },
                        HStack {
                            height: Length::Px(1),
                            gap: 1,
                            Text {
                                content: "t",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: format!("theme {}", if ctx.state.theme_provider_on { "on" } else { "off" }),
                                style: Style::new().fg(muted),
                            },
                            Text {
                                content: "g",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: format!("picker {current_theme_name}"),
                                style: Style::new().fg(muted),
                            },
                            Text {
                                content: "y",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: format!("syntax {}", if ctx.state.syntax_on { "on" } else { "off" }),
                                style: Style::new().fg(muted),
                            },
                            Text {
                                content: "i",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: format!("wheel {}", if ctx.state.smooth_wheel_scroll { "smooth" } else { "immediate" }),
                                style: Style::new().fg(muted),
                            },
                            Text {
                                content: "r",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: if ctx.state.active_stream_id.is_some() { "streaming (x stop)".to_string() } else { "stream message".to_string() },
                                style: Style::new().fg(if ctx.state.active_stream_id.is_some() { warn } else { muted }),
                            },
                            Text {
                                content: "/",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: "search modal",
                                style: Style::new().fg(muted),
                            },
                        },
                        if let Some(selected) = &ctx.state.selected_search {
                            Text {
                                content: format!("search selected: {selected}"),
                                style: Style::new().fg(dim),
                                overflow: Overflow::Wrap,
                            },
                        },
                    },
                },
                Frame::new()
                    .title("Timeline")
                    .status(status)
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .child(timeline),
                Frame {
                    border: true,
                    height: Length::Auto,
                    border_style: BorderStyle::Rounded,
                    padding: (1, 1, 0, 3),
                    style: Style::new().bg(input_bg),
                    // decorations: vec![
                    //     EdgeDecoration::new(Edge::Left).glyph(DecorationGlyph::AutoBlock)
                    //     .style(Style::new().fg(accent)),
                    // ],
                    VStack {
                        height: Length::Auto,
                        gap: 1,
                        TextArea::bound(&ctx.state.input)
                            .caret_color(Color::hex("#E7E7E8"))
                            .style(Style::new().fg(Color::hex("#D8D7D9")))
                            .height(Length::Auto)
                            .width(Length::Px(70))
                            .border(false)
                            .placeholder("Ask anything... \"Fix broken tests\"")
                            .placeholder_style(Style::new().fg(Color::hex("#767576")))
                            .on_change(ctx.link().callback(Msg::InputChanged))
                            .max_height(Length::Px(6)),
                        HStack {
                            height: Length::Px(1),
                            gap: 1,
                            Text {
                                content: "tab",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: "agents",
                                style: Style::new().fg(muted),
                            },
                            Text {
                                content: "shift+tab",
                                style: Style::new().fg(text),
                            },
                            Text {
                                content: "focus back",
                                style: Style::new().fg(muted),
                            },
                        },
                    },
                },
            }
        };

        let sidebar = build_sidebar(
            messages.len(),
            ctx.state.max_offset,
            preset,
            &palette,
            diff_rt,
        );

        let content = if !sidebar_visible {
            main_content
        } else if wide {
            rsx! {
                HStack {
                    style: Style::new().bg(bg),
                    main_content,
                    sidebar,
                }
            }
        } else {
            rsx! {
                ZStack {
                    passthrough: true,
                    main_content,
                    HStack {
                        style: Style::new().tint_by(Color::hex("#000000"), 0.35),
                        justify: Justify::End,
                        sidebar,
                    },
                }
            }
        };

        let content = if ctx.state.show_search {
            let palette_overlay = SearchPalette::<Arc<str>>::new()
                .items(build_search_items(preset, messages))
                .height(Length::Auto)
                .input_border(false)
                .list_border(false)
                .list_scrollbar(true)
                .list_selection_full_width(true)
                .list_item_hover_style(Style::new().bg(palette.subtle))
                .on_activate(ctx.link().callback(Msg::SearchActivated));

            rsx! {
                ZStack {
                    content,
                    Modal::new()
                        .title(format!("Search {}", preset.label()))
                        .child(palette_overlay)
                        .width(Length::Px(72))
                        .height(Length::Auto)
                        .border_style(BorderStyle::Rounded)
                        .padding(0)
                        .backdrop_style(Style::new().tint_by(Color::rgb(10, 20, 60), 0.55))
                        .on_close(ctx.link().callback(|_| Msg::ToggleSearch(false)))
                        .key("search-palette"),
                }
            }
        } else {
            content
        };

        let content = if ctx.state.show_theme_picker {
            let theme_overlay = SearchPalette::<usize>::new()
                .items(build_theme_items(ctx.state.theme_index))
                .sync_selection(true)
                .height(Length::Auto)
                .input_border(false)
                .list_border(false)
                .list_scrollbar(true)
                .list_selection_full_width(true)
                .list_item_hover_style(Style::new().bg(palette.subtle))
                .on_select(
                    ctx.link()
                        .callback(|ev: SearchEvent<usize>| Msg::ThemePreview(ev.item.value)),
                )
                .on_query_change(ctx.link().callback(Msg::ThemePreviewQuery))
                .on_activate(
                    ctx.link()
                        .callback(|ev: SearchEvent<usize>| Msg::ThemeActivated(ev.item.value)),
                );

            rsx! {
                ZStack {
                    content,
                    Modal::new()
                        .title("Themes")
                        .child(theme_overlay)
                        .width(Length::Px(60))
                        .height(Length::Percent(50))
                        .border_style(BorderStyle::Rounded)
                        .padding(0)
                        .backdrop_style(Style::new().dim_by(0.30))
                        .on_close(ctx.link().callback(|_| Msg::ToggleThemePicker(false)))
                        .key("theme-picker"),
                }
            }
        } else {
            content
        };

        if ctx.state.theme_provider_on {
            ThemeProvider::new(active_theme).child(content).into()
        } else {
            content
        }
    }
}

fn render_message(
    message: &DemoMessage,
    palette: &DemoPalette,
    syntax_on: bool,
    theme_index: usize,
    diff_rt: DiffRuntime,
) -> Element {
    let panel_bg = palette.panel_bg;
    let text = palette.text;
    let muted = palette.muted;
    let dim = palette.dim;
    let accent = palette.accent;
    let error = palette.error;

    match message {
        DemoMessage::User {
            id,
            text: body,
            files,
            timestamp,
            queued,
            has_compaction,
        } => rsx! {
            VStack {
                height: Length::Auto,
                gap: 1,
                Frame {
                    border: false,
                    height: Length::Auto,
                    padding: (1, 0, 1, 3),
                    style: Style::new().bg(panel_bg),
                    decorations: vec![
                        EdgeDecoration::new(Edge::Left)
                            .glyph(DecorationGlyph::AutoBlock)
                            .style(Style::new().fg(accent)),
                    ],
                    VStack {
                        height: Length::Auto,
                        gap: 1,
                        DocumentView::new(body.clone())
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .wrap(true)
                            .focusable(false)
                            .scroll_wheel(false)
                            .shared_selection_id("messages")
                            .style(Style::new().fg(text).bg(panel_bg))
                            .height(Length::Auto),
                        if !files.is_empty() {
                            HStack {
                                padding: (1, 0),
                                height: Length::Auto,
                                gap: 1,
                                for file in files {
                                    render_file_badge(file, palette),
                                },
                            },
                        },
                        if *queued {
                            render_queued_badge(accent),
                        } else {
                            Text {
                                content: timestamp.to_string(),
                                style: Style::new().fg(muted),
                            },
                        },
                    },
                },
                if *has_compaction {
                    HStack {
                        height: Length::Px(1),
                        gap: 1,
                        Divider::horizontal(),
                        Text {
                            content: " Compaction ",
                            style: Style::new().fg(accent),
                        },
                        Divider::horizontal(),
                    },
                },
            }
        }
        .key(id.clone()),
        DemoMessage::Assistant {
            id,
            parts,
            mode,
            model,
            duration,
            interrupted,
            error: message_error,
        } => rsx! {
            VStack {
                height: Length::Auto,
                gap: 1,
                for part in parts
                    .iter()
                    .filter_map(|part| {
                        render_assistant_part(part, palette, syntax_on, theme_index, diff_rt)
                    }) {
                    part,
                },
                if let Some(content) = message_error {
                    Frame {
                        border: false,
                        height: Length::Auto,
                        padding: (1, 0, 1, 3),
                        style: Style::new().bg(panel_bg),
                        decorations: vec![
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(error)),
                        ],
                        Text {
                            content: content.to_string(),
                            style: Style::new().fg(muted),
                            overflow: Overflow::Wrap,
                        },
                    },
                },
                Frame {
                    border: false,
                    height: Length::Auto,
                    padding: (0, 0, 0, 3),
                    Text::from_spans(
                        metadata_spans(mode, model, duration, *interrupted, accent, text, dim),
                    ),
                },
            }
        }
        .key(id.clone()),
    }
}

fn render_assistant_part(
    part: &AssistantPart,
    palette: &DemoPalette,
    syntax_on: bool,
    theme_index: usize,
    diff_rt: DiffRuntime,
) -> Option<Element> {
    let text = palette.text;
    let muted = palette.muted;
    let subtle = palette.subtle;

    match part {
        AssistantPart::Markdown(content) => Some(rsx! {
            Frame {
                border: false,
                height: Length::Auto,
                padding: (0, 0, 0, 3),
                apply_markdown_doc_syntax(DocumentView::new(content.clone()), syntax_on, theme_index)
                    .markdown()
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .scroll_wheel(false)
                    .shared_selection_id("messages")
                    .style(Style::new().fg(text))
                    .height(Length::Auto),
            }
        }),
        AssistantPart::Reasoning(content) => Some(rsx! {
            Frame {
                border: false,
                height: Length::Auto,
                padding: (0, 0, 0, 3),
                decorations: vec![
                    EdgeDecoration::new(Edge::Left)
                        .glyph(DecorationGlyph::AutoBlock)
                        .style(Style::new().fg(subtle)),
                ],
                apply_markdown_doc_syntax(DocumentView::new(content.clone()), syntax_on, theme_index)
                    .markdown()
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .scroll_wheel(false)
                    .shared_selection_id("messages")
                    .style(Style::new().fg(muted))
                    .height(Length::Auto),
            }
        }),
        AssistantPart::Tool(tool) => Some(render_demo_tool(
            tool,
            palette,
            syntax_on,
            theme_index,
            diff_rt,
        )),
        AssistantPart::Subtask {
            description,
            toolcalls,
            current_tool,
            key_hint,
            failed,
        } => {
            let current_tool_text = current_tool.as_deref().unwrap_or("");
            let key_hint_text = key_hint.as_deref().unwrap_or("");
            Some(rsx! {
                Frame {
                    border: false,
                    height: Length::Auto,
                    padding: (0, 0, 0, 3),
                    VStack {
                        height: Length::Auto,
                        gap: 0,
                        Text {
                            content: format!("# {} ({} toolcalls)", description, toolcalls),
                            style: Style::new().fg(muted),
                        },
                        if !current_tool_text.is_empty() {
                            Text {
                                content: format!("|- {current_tool_text}"),
                                style: Style::new().fg(if *failed { palette.error } else { muted }),
                            },
                        },
                        if !key_hint_text.is_empty() {
                            Text::from_spans(
                                vec![Span::new(key_hint_text).fg(text), Span::new(" view subagents").fg(muted)],
                            ),
                        },
                    },
                }
            })
        }
        AssistantPart::Agent(name) => Some(rsx! {
            Frame {
                border: false,
                height: Length::Auto,
                padding: (0, 0, 0, 3),
                Text {
                    content: format!("# agent {name}"),
                    style: Style::new().fg(muted),
                },
            }
        }),
        AssistantPart::Retry { attempt } => Some(rsx! {
            Frame {
                border: false,
                height: Length::Auto,
                padding: (0, 0, 0, 3),
                Text {
                    content: format!("# retry {attempt}"),
                    style: Style::new().fg(muted),
                },
            }
        }),
    }
}

fn render_demo_tool(
    tool: &DemoToolCall,
    palette: &DemoPalette,
    syntax_on: bool,
    theme_index: usize,
    diff_rt: DiffRuntime,
) -> Element {
    match tool {
        DemoToolCall::BashBlock {
            title,
            command,
            output,
            running,
        } => {
            let body: Element = rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 1,
                    Text {
                        content: format!("$ {}", command),
                        style: Style::new().fg(palette.text),
                    },
                    code_document(output.clone(), palette, syntax_on, theme_index, "sh", true, false),
                }
            };
            render_tool_panel(title.as_ref(), body, *running, palette)
        }
        DemoToolCall::Inline {
            icon,
            pending,
            content,
            complete,
            error,
            highlight,
        } => render_inline_tool(
            icon.as_ref(),
            pending.as_ref(),
            content.as_ref(),
            *complete,
            error.as_deref(),
            *highlight,
            palette,
        ),
        DemoToolCall::Read {
            path,
            suffix,
            loaded,
        } => {
            let inline = render_inline_tool(
                ">",
                "Reading file...",
                format!("Read {}{}", path, suffix).as_str(),
                true,
                None,
                InlineHighlight::Normal,
                palette,
            );
            rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 0,
                    inline,
                    for loaded_path in loaded {
                        Frame {
                            border: false,
                            height: Length::Auto,
                            padding: (0, 0, 0, 6),
                            Text {
                                content: format!("-> Loaded {loaded_path}"),
                                style: Style::new().fg(palette.muted),
                            },
                        },
                    },
                }
            }
        }
        DemoToolCall::Diff {
            title,
            before,
            after,
            wrap: _,
            split: _,
            context_lines: _,
        } => render_tool_panel(
            title.as_ref(),
            build_demo_diff_view(
                before.clone(),
                after.clone(),
                diff_rt.wrap,
                diff_rt.split,
                diff_context_from_cycle(diff_rt.context_cycle),
                diff_rt.show_context_separator,
                palette,
                syntax_on,
                theme_index,
            )
            .into(),
            false,
            palette,
        ),
        DemoToolCall::DiffFromPatch { title, patch } => render_tool_panel(
            title.as_ref(),
            build_demo_diff_from_patch(
                patch_text_without_git_header_line(patch, diff_rt.show_git_header),
                diff_rt.wrap,
                diff_rt.split,
                diff_context_from_cycle(diff_rt.context_cycle),
                diff_rt.show_context_separator,
                palette,
            )
            .into(),
            false,
            palette,
        ),
        DemoToolCall::Todo { items } => {
            let body: Element = rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 0,
                    for item in items {
                        HStack {
                            height: Length::Auto,
                            gap: 0,
                            Text {
                                content: format!("[{}] ", todo_status_glyph(item.status)),
                                style: todo_status_style(item.status, palette),
                            },
                            Text {
                                content: item.content.to_string(),
                                style: todo_status_style(item.status, palette),
                            },
                        },
                    },
                }
            };
            render_tool_panel("# Todos", body, false, palette)
        }
        DemoToolCall::Questions { items } => {
            let body: Element = rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 1,
                    for item in items {
                        VStack {
                            height: Length::Auto,
                            gap: 0,
                            Text {
                                content: item.question.to_string(),
                                style: Style::new().fg(palette.muted),
                            },
                            Text {
                                content: item.answer.to_string(),
                                style: Style::new().fg(palette.text),
                            },
                        },
                    },
                }
            };
            render_tool_panel("# Questions", body, false, palette)
        }
        DemoToolCall::Task {
            title,
            description,
            toolcalls,
            current_tool,
            key_hint,
            running,
            failed,
        } => {
            let current_tool_text = current_tool.as_deref().unwrap_or("");
            let key_hint_text = key_hint.as_deref().unwrap_or("");
            let body: Element = rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 0,
                    Text {
                        content: format!("{} ({} toolcalls)", description, toolcalls),
                        style: Style::new().fg(palette.muted),
                    },
                    if !current_tool_text.is_empty() {
                        Text {
                            content: format!("|- {current_tool_text}"),
                            style: Style::new().fg(if *failed { palette.error } else { palette.muted }),
                        },
                    },
                    if !key_hint_text.is_empty() {
                        Text::from_spans(
                            vec![
                                Span::new(key_hint_text).fg(palette.text),
                                Span::new(" view subagents").fg(palette.muted),
                            ],
                        ),
                    },
                }
            };
            render_tool_panel(title.as_ref(), body, *running, palette)
        }
        DemoToolCall::Diagnostics {
            title,
            content,
            diagnostics,
        } => {
            let body: Element = rsx! {
                VStack {
                    height: Length::Auto,
                    gap: 1,
                    code_document(content.clone(), palette, syntax_on, theme_index, "rust", true, true),
                    for diag in diagnostics {
                        Text {
                            content: format!("Error [{}:{}]: {}", diag.line, diag.col, diag.message),
                            style: Style::new().fg(palette.error),
                        },
                    },
                }
            };
            render_tool_panel(title.as_ref(), body, false, palette)
        }
        DemoToolCall::Generic { title, output } => {
            let body: Element = rsx! {
                code_document(output.clone(), palette, syntax_on, theme_index, "sh", true, false)
            };
            render_tool_panel(title.as_ref(), body, false, palette)
        }
    }
}

fn render_inline_tool(
    icon: &str,
    pending: &str,
    content: &str,
    complete: bool,
    error: Option<&str>,
    highlight: InlineHighlight,
    palette: &DemoPalette,
) -> Element {
    let row_text = if complete {
        format!("{icon} {content}")
    } else {
        format!("~ {pending}")
    };
    let mut style = match highlight {
        InlineHighlight::Normal => {
            if complete {
                Style::new().fg(palette.muted)
            } else {
                Style::new().fg(palette.text)
            }
        }
        InlineHighlight::Warning => Style::new().fg(palette.warn),
        InlineHighlight::Denied => Style::new().fg(palette.muted),
    };
    if matches!(highlight, InlineHighlight::Denied) {
        style = style.strikethrough();
    }
    let error_text = error.unwrap_or("");
    let show_error = !error_text.is_empty() && !matches!(highlight, InlineHighlight::Denied);

    rsx! {
        VStack {
            height: Length::Auto,
            gap: 0,
            padding: (0, 0, 0, 3),
            Text {
                content: row_text,
                style: style,
            },
            if show_error {
                Text {
                    content: error_text.to_string(),
                    style: Style::new().fg(palette.error),
                },
            },
        }
    }
}

fn render_tool_panel(title: &str, body: Element, running: bool, palette: &DemoPalette) -> Element {
    let title_text = if running {
        format!("{title} (running)")
    } else {
        title.to_string()
    };
    let title_color = if running { palette.warn } else { palette.muted };

    rsx! {
        Frame {
            border: false,
            height: Length::Auto,
            padding: (1, 1, 1, 3),
            style: Style::new().bg(palette.panel_bg),
            decorations: vec![
                EdgeDecoration::new(Edge::Left)
                    .glyph(DecorationGlyph::AutoBlock)
                    .style(Style::new().fg(palette.subtle)),
            ],
            VStack {
                height: Length::Auto,
                gap: 1,
                Text {
                    content: title_text,
                    style: Style::new().fg(title_color),
                },
                body,
            },
        }
    }
}

fn render_queued_badge(accent: Color) -> Element {
    Text::from_spans(vec![
        Span::new(" QUEUED ").fg(Color::Black).bg(accent).bold(),
    ])
    .into()
}

fn render_file_badge(file: &DemoFile, palette: &DemoPalette) -> Element {
    let (tag, left_bg, left_fg) = match file.kind {
        DemoFileKind::File => (" file ", palette.warn, Color::Black),
        DemoFileKind::Image => (" img ", palette.accent, Color::Black),
        DemoFileKind::Pdf => (" pdf ", palette.warn, Color::Black),
        DemoFileKind::Dir => (" dir ", palette.subtle, palette.text),
    };

    Text::from_spans(vec![
        Span::new(tag).fg(left_fg).bg(left_bg).bold(),
        Span::new(format!(" {} ", file.label))
            .fg(palette.text)
            .bg(palette.input_bg),
    ])
    .into()
}

type BuiltinTheme = (&'static str, fn() -> Theme);

const BUILTIN_THEMES: [BuiltinTheme; 9] = [
    ("ANSI", Theme::ansi),
    ("One Dark", Theme::one_dark),
    ("Dracula", Theme::dracula),
    ("Nord", Theme::nord),
    ("Gruvbox", Theme::gruvbox),
    ("Catppuccin", Theme::catppuccin),
    ("Tokyo Night", Theme::tokyo_night),
    ("Solarized Dark", Theme::solarized_dark),
    ("Monokai", Theme::monokai),
];

#[cfg(feature = "syntax-syntect")]
fn active_syntect_theme_name(index: usize) -> &'static str {
    match BUILTIN_THEMES.get(index).map(|t| t.0).unwrap_or("One Dark") {
        "ANSI" => "One Dark (Atom)",
        "One Dark" => "One Dark (Atom)",
        "Dracula" => "Dracula",
        "Nord" => "base16-ocean.dark",
        "Gruvbox" => "Monokai Extended",
        "Catppuccin" => "Catppuccin Mocha",
        "Tokyo Night" => "One Dark (Atom)",
        "Solarized Dark" => "base16-ocean.dark",
        "Monokai" => "Monokai Extended",
        _ => "One Dark (Atom)",
    }
}

fn selected_theme(index: usize) -> Theme {
    BUILTIN_THEMES
        .get(index)
        .map(|(_, build)| build())
        .unwrap_or_else(Theme::one_dark)
}

fn build_theme_items(active_index: usize) -> Vec<SearchItem<usize>> {
    BUILTIN_THEMES
        .iter()
        .enumerate()
        .map(|(index, (name, _))| SearchItem::new(*name, index).active(index == active_index))
        .collect()
}

fn first_theme_match(query: &str) -> Option<usize> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }

    BUILTIN_THEMES
        .iter()
        .enumerate()
        .find(|(_, (name, _))| name.to_ascii_lowercase().contains(q.as_str()))
        .map(|(index, _)| index)
}

fn apply_markdown_doc_syntax(
    doc: DocumentView,
    syntax_on: bool,
    theme_index: usize,
) -> DocumentView {
    #[cfg(not(feature = "syntax-syntect"))]
    {
        let _ = (syntax_on, theme_index);
        doc
    }
    #[cfg(feature = "syntax-syntect")]
    {
        let mut doc = doc;
        if syntax_on {
            doc = doc.code_syntax_strategy(
                SyntectStrategy::default().default_theme(active_syntect_theme_name(theme_index)),
            );
        }
        doc
    }
}

fn code_document(
    value: Arc<str>,
    palette: &DemoPalette,
    syntax_on: bool,
    theme_index: usize,
    language: &str,
    wrap: bool,
    line_numbers: bool,
) -> DocumentView {
    let content = if syntax_on {
        Arc::from(format!("```{language}\n{value}\n```"))
    } else {
        value
    };

    let mut doc = DocumentView::new(content)
        .border(false)
        .scrollbar(false)
        .h_scrollbar(!wrap)
        .wrap(wrap)
        .focusable(false)
        .scroll_wheel(false)
        .style(Style::new().fg(palette.text).bg(palette.input_bg))
        .height(Length::Auto)
        .line_numbers(line_numbers);

    if syntax_on {
        doc = apply_markdown_doc_syntax(doc, true, theme_index).markdown();
    }

    doc
}

fn apply_diff_syntax(diff: DiffView, syntax_on: bool, theme_index: usize) -> DiffView {
    #[cfg(not(feature = "syntax-syntect"))]
    {
        let _ = (syntax_on, theme_index);
        diff
    }
    #[cfg(feature = "syntax-syntect")]
    {
        let mut diff = diff;
        if syntax_on {
            diff = diff.with_syntax("rust", active_syntect_theme_name(theme_index));
        }
        diff
    }
}

#[allow(clippy::too_many_arguments)]
fn build_demo_diff_view(
    before: Arc<str>,
    after: Arc<str>,
    wrap: bool,
    split: bool,
    context_lines: Option<usize>,
    show_context_separator: bool,
    palette: &DemoPalette,
    syntax_on: bool,
    theme_index: usize,
) -> DiffView {
    let mut diff = DiffView::with_content(before, after)
        .backend(DiffViewBackend::DocumentView)
        .document_view(DocumentView::new("").scroll_wheel(false))
        .shared_selection_id("messages")
        .mode(if split {
            DiffViewMode::Split
        } else {
            DiffViewMode::Unified
        })
        .height(Length::Auto)
        .border(false)
        .panels_border(false)
        .highlight_full_width(true)
        .line_numbers(true)
        .gutter_inset(1)
        .wrap(wrap)
        .h_scrollbar(!wrap)
        .scrollbar(false)
        .focusable(false)
        .neutral_bg(palette.input_bg)
        .show_context_separator(show_context_separator);

    if let Some(n) = context_lines {
        diff = diff.context_lines(n);
    }

    apply_diff_syntax(diff, syntax_on, theme_index)
}

fn build_demo_diff_from_patch(
    patch: Arc<str>,
    wrap: bool,
    split: bool,
    context_lines: Option<usize>,
    show_context_separator: bool,
    palette: &DemoPalette,
) -> DiffView {
    let mut diff = DiffView::from_patch(patch)
        .backend(DiffViewBackend::DocumentView)
        .document_view(DocumentView::new("").scroll_wheel(false))
        .shared_selection_id("messages")
        .trim_common_indent(false)
        .mode(if split {
            DiffViewMode::Split
        } else {
            DiffViewMode::Unified
        })
        .height(Length::Auto)
        .border(false)
        .panels_border(false)
        .highlight_full_width(true)
        .line_numbers(true)
        .min_line_number_width(3)
        .gutter_inset(1)
        .wrap(wrap)
        .h_scrollbar(!wrap)
        .scrollbar(false)
        .focusable(false)
        .neutral_bg(palette.input_bg)
        .show_context_separator(show_context_separator);

    if let Some(n) = context_lines {
        diff = diff.context_lines(n);
    }

    diff
}

fn todo_status_glyph(status: DemoTodoStatus) -> &'static str {
    match status {
        DemoTodoStatus::Completed => "x",
        DemoTodoStatus::InProgress => ">",
        DemoTodoStatus::Pending => " ",
    }
}

fn todo_status_style(status: DemoTodoStatus, palette: &DemoPalette) -> Style {
    match status {
        DemoTodoStatus::InProgress => Style::new().fg(palette.warn),
        DemoTodoStatus::Completed => Style::new().fg(palette.text),
        DemoTodoStatus::Pending => Style::new().fg(palette.muted),
    }
}

fn metadata_spans(
    mode: &Arc<str>,
    model: &Arc<str>,
    duration: &Arc<str>,
    interrupted: bool,
    accent: Color,
    text: Color,
    dim: Color,
) -> Vec<Span> {
    let marker = if interrupted { dim } else { accent };
    let mut spans = vec![
        Span::new("# ").fg(marker),
        Span::new(mode.as_ref()).fg(text),
        Span::new(" · ").fg(dim),
        Span::new(model.as_ref()).fg(dim),
        Span::new(" · ").fg(dim),
        Span::new(duration.as_ref()).fg(dim),
    ];

    if interrupted {
        spans.push(Span::new(" · ").fg(dim));
        spans.push(Span::new("interrupted").fg(dim));
    }

    spans
}

fn build_sidebar(
    message_count: usize,
    max_offset: usize,
    preset: ReproPreset,
    palette: &DemoPalette,
    diff_rt: DiffRuntime,
) -> Element {
    let diff_ctx = diff_context_cycle_label(diff_rt.context_cycle);
    let diff_wrap = if diff_rt.wrap { "on" } else { "off" };
    let diff_mode = if diff_rt.split { "split" } else { "unified" };
    let diff_sep = if diff_rt.show_context_separator {
        "on"
    } else {
        "off"
    };
    let diff_git = if diff_rt.show_git_header { "on" } else { "off" };
    rsx! {
        Frame {
            width: Length::Px(36),
            border: true,
            border_style: BorderStyle::Rounded,
            padding: (1, 1),
            style: Style::new().bg(palette.panel_bg),
            VStack {
                height: Length::Flex(1),
                gap: 1,
                Text {
                    content: "Context",
                    style: Style::new().fg(palette.text).bold(),
                },
                Text::from_spans(
                    vec![
                        Span::new("messages ").fg(palette.muted),
                        Span::new(message_count.to_string()).fg(palette.accent),
                    ],
                ),
                Text::from_spans(
                    vec![
                        Span::new("max offset ").fg(palette.muted),
                        Span::new(max_offset.to_string()).fg(palette.accent),
                    ],
                ),
                Divider::horizontal(),
                Text {
                    content: "DiffView",
                    style: Style::new().fg(palette.text).bold(),
                },
                Text::from_spans(
                    vec![Span::new("context ").fg(palette.muted), Span::new(diff_ctx).fg(palette.accent)],
                ),
                Text::from_spans(
                    vec![Span::new("wrap ").fg(palette.muted), Span::new(diff_wrap).fg(palette.accent)],
                ),
                Text::from_spans(
                    vec![Span::new("mode ").fg(palette.muted), Span::new(diff_mode).fg(palette.accent)],
                ),
                Text::from_spans(
                    vec![Span::new("sep ").fg(palette.muted), Span::new(diff_sep).fg(palette.accent)],
                ),
                Text::from_spans(
                    vec![Span::new("git ").fg(palette.muted), Span::new(diff_git).fg(palette.accent)],
                ),
                Text {
                    content: "keys n w m e h",
                    style: Style::new().fg(palette.dim),
                },
                Divider::horizontal(),
                Text {
                    content: "Included",
                    style: Style::new().fg(palette.text).bold(),
                },
                Text {
                    content: "- queued user rows + compaction",
                    style: Style::new().fg(palette.muted),
                },
                Text {
                    content: "- markdown + reasoning + metadata",
                    style: Style::new().fg(palette.muted),
                },
                Text {
                    content: "- inline tools, read trees, task cards",
                    style: Style::new().fg(palette.muted),
                },
                Divider::horizontal(),
                Text {
                    content: "Diffs",
                    style: Style::new().fg(palette.text).bold(),
                },
                Text::from_spans(
                    vec![
                        Span::new("M ").fg(palette.warn),
                        Span::new("src/widgets/message_view.rs").fg(palette.muted),
                    ],
                ),
                Text::from_spans(
                    vec![
                        Span::new("M ").fg(palette.warn),
                        Span::new("examples/scroll_view_opencode_repro.rs").fg(palette.muted),
                    ],
                ),
                Divider::horizontal(),
                Text {
                    content: "Now includes DiffView, diagnostics, todos, questions, and task rows.",
                    style: Style::new().fg(palette.dim),
                    overflow: Overflow::Wrap,
                },
                Divider::horizontal(),
                Text {
                    content: "Preset",
                    style: Style::new().fg(palette.text).bold(),
                },
                Text {
                    content: preset.sidebar_note(),
                    style: Style::new().fg(palette.dim),
                    overflow: Overflow::Wrap,
                },
            },
        }
    }
}

fn build_search_items(preset: ReproPreset, messages: &[DemoMessage]) -> Vec<SearchItem<Arc<str>>> {
    let mut items = Vec::new();

    items.push(
        SearchItem::new(
            format!("Preset: {}", preset.label()),
            Arc::from(format!("preset:{}", preset.label())),
        )
        .description(preset.sidebar_note()),
    );

    for (index, message) in messages.iter().enumerate() {
        match message {
            DemoMessage::User {
                text,
                files,
                timestamp,
                ..
            } => {
                items.push(
                    SearchItem::new(
                        format!("User #{index}: {}", truncate_one_line(text, 56)),
                        Arc::from(format!("user:{index}")),
                    )
                    .description(format!(
                        "timestamp {timestamp} | files {} | {}",
                        files.len(),
                        truncate_one_line(text, 96)
                    )),
                );
            }
            DemoMessage::Assistant {
                parts,
                mode,
                model,
                error,
                ..
            } => {
                items.push(
                    SearchItem::new(
                        format!("Assistant #{index}: {mode} · {model}"),
                        Arc::from(format!("assistant:{index}")),
                    )
                    .description(format!(
                        "parts {} | error {}",
                        parts.len(),
                        if error.is_some() { "yes" } else { "no" }
                    )),
                );

                for (part_index, part) in parts.iter().enumerate() {
                    if let Some(item) = search_item_for_part(index, part_index, part) {
                        items.push(item);
                    }
                }
            }
        }
    }

    items
}

fn search_item_for_part(
    message_index: usize,
    part_index: usize,
    part: &AssistantPart,
) -> Option<SearchItem<Arc<str>>> {
    match part {
        AssistantPart::Markdown(content) => Some(
            SearchItem::new(
                format!("Markdown #{message_index}.{part_index}"),
                Arc::from(format!("markdown:{message_index}:{part_index}")),
            )
            .description(truncate_one_line(content, 96)),
        ),
        AssistantPart::Reasoning(content) => Some(
            SearchItem::new(
                format!("Reasoning #{message_index}.{part_index}"),
                Arc::from(format!("reasoning:{message_index}:{part_index}")),
            )
            .description(truncate_one_line(content, 96)),
        ),
        AssistantPart::Tool(tool) => Some(search_item_for_tool(message_index, part_index, tool)),
        AssistantPart::Subtask { description, .. } => Some(
            SearchItem::new(
                format!("Subtask #{message_index}.{part_index}"),
                Arc::from(format!("subtask:{message_index}:{part_index}")),
            )
            .description(description.clone()),
        ),
        AssistantPart::Agent(name) => Some(
            SearchItem::new(
                format!("Agent #{message_index}.{part_index}"),
                Arc::from(format!("agent:{message_index}:{part_index}")),
            )
            .description(name.clone()),
        ),
        AssistantPart::Retry { attempt } => Some(
            SearchItem::new(
                format!("Retry #{message_index}.{part_index}"),
                Arc::from(format!("retry:{message_index}:{part_index}")),
            )
            .description(format!("attempt {attempt}")),
        ),
    }
}

fn search_item_for_tool(
    message_index: usize,
    part_index: usize,
    tool: &DemoToolCall,
) -> SearchItem<Arc<str>> {
    match tool {
        DemoToolCall::BashBlock {
            title,
            command,
            output,
            ..
        } => SearchItem::new(
            format!("Bash #{message_index}.{part_index}: {title}"),
            Arc::from(format!("bash:{message_index}:{part_index}")),
        )
        .description(format!("{command} | {}", truncate_one_line(output, 80))),
        DemoToolCall::Inline { content, error, .. } => SearchItem::new(
            format!("Inline #{message_index}.{part_index}"),
            Arc::from(format!("inline:{message_index}:{part_index}")),
        )
        .description(format!(
            "{}{}",
            truncate_one_line(content, 72),
            error
                .as_deref()
                .map(|e| format!(" | {}", truncate_one_line(e, 40)))
                .unwrap_or_default()
        )),
        DemoToolCall::Read { path, loaded, .. } => SearchItem::new(
            format!("Read #{message_index}.{part_index}: {path}"),
            Arc::from(format!("read:{message_index}:{part_index}")),
        )
        .description(format!("loaded {} files", loaded.len())),
        DemoToolCall::Diff { title, .. } => SearchItem::new(
            format!("Diff #{message_index}.{part_index}: {title}"),
            Arc::from(format!("diff:{message_index}:{part_index}")),
        )
        .description("DiffView row with syntax/theme-sensitive rendering cost"),
        DemoToolCall::DiffFromPatch { title, .. } => SearchItem::new(
            format!("Patch diff #{message_index}.{part_index}: {title}"),
            Arc::from(format!("patch-diff:{message_index}:{part_index}")),
        )
        .description("DiffView::from_patch(unified diff string)"),
        DemoToolCall::Todo { items } => SearchItem::new(
            format!("Todos #{message_index}.{part_index}"),
            Arc::from(format!("todo:{message_index}:{part_index}")),
        )
        .description(format!("{} todo items", items.len())),
        DemoToolCall::Questions { items } => SearchItem::new(
            format!("Questions #{message_index}.{part_index}"),
            Arc::from(format!("questions:{message_index}:{part_index}")),
        )
        .description(format!("{} questions", items.len())),
        DemoToolCall::Task {
            title, description, ..
        } => SearchItem::new(
            format!("Task #{message_index}.{part_index}: {title}"),
            Arc::from(format!("task:{message_index}:{part_index}")),
        )
        .description(description.clone()),
        DemoToolCall::Diagnostics { title, content, .. } => SearchItem::new(
            format!("Diagnostics #{message_index}.{part_index}: {title}"),
            Arc::from(format!("diagnostics:{message_index}:{part_index}")),
        )
        .description(truncate_one_line(content, 88)),
        DemoToolCall::Generic { title, output } => SearchItem::new(
            format!("Generic #{message_index}.{part_index}: {title}"),
            Arc::from(format!("generic:{message_index}:{part_index}")),
        )
        .description(truncate_one_line(output, 88)),
    }
}

fn truncate_one_line(value: &str, max_chars: usize) -> Arc<str> {
    let line = value.lines().next().unwrap_or("").trim();
    let mut out = String::new();
    for ch in line.chars().take(max_chars) {
        out.push(ch);
    }
    if line.chars().count() > max_chars {
        out.push('…');
    }
    Arc::from(out)
}

fn append_streaming_exchange(messages: &mut Vec<DemoMessage>, stream_id: u64, prompt: &str) {
    messages.push(DemoMessage::User {
        id: format!("stream-user-{stream_id}"),
        text: Arc::from(prompt.to_string()),
        files: vec![demo_file(
            "examples/scroll_view_opencode_repro.rs",
            DemoFileKind::File,
        )],
        timestamp: Arc::from("now"),
        queued: false,
        has_compaction: false,
    });

    messages.push(DemoMessage::Assistant {
        id: streaming_assistant_id(stream_id),
        parts: vec![
            AssistantPart::Reasoning(Arc::from("Thinking about the scroll anchor path...")),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("$"),
                pending: Arc::from("Preparing command..."),
                content: Arc::from("bash command=\"cargo test scroll_view::reconcile::tests\""),
                complete: false,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::BashBlock {
                title: Arc::from("# cargo test scroll anchoring"),
                command: Arc::from(
                    "cargo test --package tui-lipan --lib --features \"markdown diff-view\" scroll_view::reconcile::tests",
                ),
                output: Arc::from(""),
                running: true,
            }),
            AssistantPart::Markdown(Arc::from("")),
        ],
        mode: Arc::from("build"),
        model: Arc::from("gpt-5.4"),
        duration: Arc::from("streaming"),
        interrupted: false,
        error: None,
    });
}

fn streaming_assistant_id(stream_id: u64) -> String {
    format!("stream-assistant-{stream_id}")
}

fn apply_streaming_step(messages: &mut [DemoMessage], stream_id: u64, step: usize) {
    let assistant_id = streaming_assistant_id(stream_id);
    let Some(DemoMessage::Assistant {
        parts,
        duration,
        interrupted,
        error,
        ..
    }) = messages.iter_mut().find(|message| match message {
        DemoMessage::Assistant { id, .. } => id == &assistant_id,
        DemoMessage::User { .. } => false,
    })
    else {
        return;
    };

    *duration = Arc::from(format!("streaming step {}", step.saturating_add(1)));
    *interrupted = false;
    *error = None;

    for part in parts {
        match part {
            AssistantPart::Reasoning(content) => {
                *content = Arc::from(streaming_reasoning(step));
            }
            AssistantPart::Tool(DemoToolCall::Inline {
                complete, content, ..
            }) if step >= 1 => {
                *complete = true;
                *content = Arc::from(
                    "bash command=\"cargo test --package tui-lipan --lib scroll_view::reconcile::tests\"",
                );
            }
            AssistantPart::Tool(DemoToolCall::BashBlock {
                output, running, ..
            }) => {
                *output = Arc::from(streaming_bash_output(step));
                *running = step + 1 < STREAM_STEP_COUNT;
            }
            AssistantPart::Markdown(content) => {
                *content = Arc::from(streaming_answer_markdown(step));
            }
            _ => {}
        }
    }
}

fn finish_streaming_exchange(
    messages: &mut [DemoMessage],
    stream_id: u64,
    interrupted_by_user: bool,
) {
    let assistant_id = streaming_assistant_id(stream_id);
    let Some(DemoMessage::Assistant {
        parts,
        duration,
        interrupted,
        error,
        ..
    }) = messages.iter_mut().find(|message| match message {
        DemoMessage::Assistant { id, .. } => id == &assistant_id,
        DemoMessage::User { .. } => false,
    })
    else {
        return;
    };

    *duration = Arc::from(if interrupted_by_user {
        "interrupted"
    } else {
        "2.4s"
    });
    *interrupted = interrupted_by_user;
    *error = interrupted_by_user.then(|| Arc::from("stream interrupted by repro user"));

    for part in parts {
        match part {
            AssistantPart::Tool(DemoToolCall::BashBlock {
                output, running, ..
            }) => {
                if !interrupted_by_user {
                    *output = Arc::from(streaming_bash_output(STREAM_STEP_COUNT));
                }
                *running = false;
            }
            AssistantPart::Tool(DemoToolCall::Inline {
                complete, content, ..
            }) => {
                *complete = true;
                *content = Arc::from(if interrupted_by_user {
                    "stream aborted before final patch summary"
                } else {
                    "stream completed; final response committed"
                });
            }
            AssistantPart::Markdown(content) if !interrupted_by_user => {
                *content = Arc::from(streaming_answer_markdown(STREAM_STEP_COUNT));
            }
            _ => {}
        }
    }
}

fn streaming_reasoning(step: usize) -> String {
    let lines = [
        "Thinking about the scroll anchor path...",
        "The top visible row should stay fixed when only bottom content grows.",
        "I am keeping the same assistant row id and mutating its parts like opencode SSE deltas.",
        "If you scroll up now, the stream switches from tail-follow to anchored offset mode.",
    ];
    lines
        .iter()
        .take((step / 2 + 1).min(lines.len()))
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

fn streaming_bash_output(step: usize) -> String {
    let lines = [
        "running 82 tests",
        "test append_with_indicators_keeps_exact_offset ... ok",
        "test append_keeps_offset_when_center_anchor_child_is_short ... ok",
        "test virtual_estimate_repeated_appends_keep_visible_anchor_stable ... ok",
        "test append_during_post_reconcile_auto_height_drift_keeps_top_visible_row ... ok",
        "test scroll_anchor_survives_post_reconcile_auto_height_drift ... ok",
        "test repro_timeline_split_wrapped_diff_keeps_both_panes_same_height_on_resize ... ok",
        "test result: ok. 82 passed; 0 failed; 0 ignored",
    ];
    let shown = (step.saturating_sub(1)).min(lines.len());
    if shown == 0 {
        "Compiling tui-lipan v0.1.0\n".to_string()
    } else {
        format!("Compiling tui-lipan v0.1.0\n{}", lines[..shown].join("\n"))
    }
}

fn streaming_answer_markdown(step: usize) -> String {
    let chunks = [
        "## Streaming repro\n",
        "This assistant row is mutated in place with stable keys, matching the session timeline shape in opencode-tui.\n\n",
        "- `r` starts a new user + assistant exchange and follows the tail.\n",
        "- Scroll upward during the stream to switch into anchored mid-history mode.\n",
        "- The bottom assistant continues to grow through reasoning, tool output, and markdown parts.\n\n",
        "The visible row should not nudge while these bottom chunks arrive.",
    ];
    chunks
        .iter()
        .take((step.saturating_sub(4) + 1).min(chunks.len()))
        .copied()
        .collect::<String>()
}

fn build_messages_for_preset(preset: ReproPreset) -> Vec<DemoMessage> {
    if preset == ReproPreset::DiffTest {
        return vec![
            build_user_message(0),
            DemoMessage::Assistant {
                id: "assistant-diff-test".to_string(),
                parts: build_diff_test_parts(),
                mode: Arc::from("review"),
                model: Arc::from("gpt-5.4"),
                duration: Arc::from("42ms"),
                interrupted: false,
                error: None,
            },
        ];
    }

    if preset == ReproPreset::DiffPatch {
        return vec![
            build_user_message(0),
            DemoMessage::Assistant {
                id: "assistant-patch-diff".to_string(),
                parts: build_diff_patch_parts(),
                mode: Arc::from("review"),
                model: Arc::from("gpt-5.4"),
                duration: Arc::from("55ms"),
                interrupted: false,
                error: None,
            },
        ];
    }

    let mut messages = Vec::with_capacity(MESSAGE_PAIR_COUNT * 2);

    for i in 0..MESSAGE_PAIR_COUNT {
        messages.push(build_user_message_for_preset(i, preset));
        messages.push(build_assistant_message_for_preset(i, preset));
    }

    messages
}

fn build_user_message_for_preset(i: usize, preset: ReproPreset) -> DemoMessage {
    match preset {
        ReproPreset::TextHeavy => DemoMessage::User {
            id: format!("user-{i}"),
            text: Arc::from(text_heavy_user_template(i)),
            files: if i % 5 == 0 {
                vec![demo_file("logs/agent.stderr", DemoFileKind::File)]
            } else {
                Vec::new()
            },
            timestamp: Arc::from(format!("{}:{:02} PM", 7 + (i % 4), (11 + i * 3) % 60)),
            queued: i % 9 == 4,
            has_compaction: i % 17 == 9,
        },
        ReproPreset::ErrorHeavy => DemoMessage::User {
            id: format!("user-{i}"),
            text: Arc::from(error_heavy_user_template(i)),
            files: vec![demo_file(
                "src/widgets/tool_part_view.rs",
                DemoFileKind::File,
            )],
            timestamp: Arc::from(format!("{}:{:02} AM", 10 + (i % 2), (9 + i * 7) % 60)),
            queued: i % 7 == 2,
            has_compaction: false,
        },
        _ => build_user_message(i),
    }
}

fn build_assistant_message_for_preset(i: usize, preset: ReproPreset) -> DemoMessage {
    match preset {
        ReproPreset::Mixed => build_assistant_message(i),
        ReproPreset::DiffHeavy => DemoMessage::Assistant {
            id: format!("assistant-{i}"),
            parts: build_diff_heavy_parts(i),
            mode: Arc::from("review"),
            model: Arc::from("gpt-5.4"),
            duration: Arc::from(format!("{}ms", 160 + (i % 5) * 90)),
            interrupted: i % 19 == 0,
            error: None,
        },
        ReproPreset::PatchStack => DemoMessage::Assistant {
            id: format!("assistant-{i}"),
            parts: build_patch_stack_parts(i),
            mode: Arc::from("patch"),
            model: Arc::from("claude-sonnet-4"),
            duration: Arc::from(format!("{}ms", 190 + (i % 4) * 110)),
            interrupted: false,
            error: None,
        },
        ReproPreset::TextHeavy => DemoMessage::Assistant {
            id: format!("assistant-{i}"),
            parts: build_text_heavy_parts(i),
            mode: Arc::from("chat"),
            model: Arc::from("gpt-5.4"),
            duration: Arc::from(format!("{}ms", 100 + (i % 6) * 55)),
            interrupted: i % 23 == 0,
            error: Some(Arc::from(multiline_error_text(i))),
        },
        ReproPreset::ErrorHeavy => DemoMessage::Assistant {
            id: format!("assistant-{i}"),
            parts: build_error_heavy_parts(i),
            mode: Arc::from("build"),
            model: Arc::from("claude-sonnet-4"),
            duration: Arc::from(format!("{}ms", 220 + (i % 7) * 70)),
            interrupted: i % 11 == 0,
            error: Some(Arc::from(multiline_apply_patch_error(i))),
        },
        ReproPreset::DiffTest => build_assistant_message(i),
        ReproPreset::DiffPatch => build_assistant_message(i),
    }
}

fn build_user_message(i: usize) -> DemoMessage {
    DemoMessage::User {
        id: format!("user-{i}"),
        text: Arc::from(user_template(i)),
        files: build_user_files(i),
        timestamp: Arc::from(format!(
            "{}:{:02} {}",
            9 + ((i / 6) % 3),
            (7 + i * 5) % 60,
            if i % 2 == 0 { "AM" } else { "PM" }
        )),
        queued: i % 11 == 5,
        has_compaction: i % 13 == 7,
    }
}

fn build_assistant_message(i: usize) -> DemoMessage {
    DemoMessage::Assistant {
        id: format!("assistant-{i}"),
        parts: build_assistant_parts(i),
        mode: Arc::from(match i % 4 {
            0 => "build",
            1 => "chat",
            2 => "plan",
            _ => "review",
        }),
        model: Arc::from(if i % 3 == 0 {
            "gpt-5.4"
        } else {
            "claude-sonnet-4"
        }),
        duration: Arc::from(format!("{}ms", 120 + (i % 7) * 85)),
        interrupted: i % 17 == 0,
        error: assistant_error(i),
    }
}

fn build_user_files(i: usize) -> Vec<DemoFile> {
    match i % 6 {
        0 => vec![demo_file("src/widgets/message_view.rs", DemoFileKind::File)],
        1 => vec![demo_file("src/screens/session.rs", DemoFileKind::File)],
        2 => vec![demo_file("perf/flamegraph.png", DemoFileKind::Image)],
        3 => vec![demo_file("notes/scroll-regression.pdf", DemoFileKind::Pdf)],
        4 => vec![
            demo_file("src/widgets", DemoFileKind::Dir),
            demo_file("src/widgets/tool_part_view.rs", DemoFileKind::File),
        ],
        _ => Vec::new(),
    }
}

fn demo_file(label: &str, kind: DemoFileKind) -> DemoFile {
    DemoFile {
        label: Arc::from(label),
        kind,
    }
}

fn build_assistant_parts(i: usize) -> Vec<AssistantPart> {
    let markdown = Arc::from(markdown_template(i));
    let reasoning = Arc::from(reasoning_template(i));
    let (before_a, after_a) = diff_pair(i);
    let (before_b, after_b) = diff_pair(i + 1);

    match i % 8 {
        0 => vec![
            AssistantPart::Reasoning(reasoning),
            AssistantPart::Tool(DemoToolCall::Diff {
                title: Arc::from("Patch src/widgets/message_view.rs"),
                before: Arc::from(before_a),
                after: Arc::from(after_a),
                wrap: true,
                split: true,
                context_lines: None,
            }),
            AssistantPart::Tool(DemoToolCall::BashBlock {
                title: Arc::from("# Run targeted scroll test"),
                command: Arc::from("cargo test session::tests::scroll_perf -- --exact"),
                output: Arc::from(bash_output(i)),
                running: false,
            }),
            AssistantPart::Markdown(markdown),
        ],
        1 => vec![
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("*"),
                pending: Arc::from("Finding files..."),
                content: Arc::from("Glob \"**/message_view.rs\" in src (3 matches)"),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Read {
                path: Arc::from("src/widgets/message_view.rs"),
                suffix: Arc::from(" [offset=280, limit=140]"),
                loaded: vec![
                    Arc::from("src/widgets/message_view.rs"),
                    Arc::from("src/widgets/tool_part_view.rs"),
                ],
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("*"),
                pending: Arc::from("Searching content..."),
                content: Arc::from("Grep \"DiffView|render_tool_part\" in src/widgets (9 matches)"),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from(">"),
                pending: Arc::from("Listing directory..."),
                content: Arc::from("List src/widgets"),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Markdown(markdown),
        ],
        2 => vec![
            AssistantPart::Tool(DemoToolCall::Task {
                title: Arc::from("# Explore Task"),
                description: Arc::from("Mirror every opencode message shape in the scroll repro"),
                toolcalls: 5 + (i % 3),
                current_tool: Some(Arc::from("Read src/widgets/message_view.rs")),
                key_hint: Some(Arc::from("ctrl+j")),
                running: i % 4 == 2,
                failed: false,
            }),
            AssistantPart::Tool(DemoToolCall::Todo {
                items: demo_todos(i),
            }),
            AssistantPart::Tool(DemoToolCall::Questions {
                items: demo_questions(i),
            }),
            AssistantPart::Subtask {
                description: Arc::from("Cross-check diff, task, and question renderers"),
                toolcalls: 3 + (i % 2),
                current_tool: Some(Arc::from("Read src/bin/mockup.rs")),
                key_hint: Some(Arc::from("ctrl+j")),
                failed: false,
            },
            AssistantPart::Markdown(markdown),
        ],
        3 => vec![
            AssistantPart::Tool(DemoToolCall::Diagnostics {
                title: Arc::from("# Wrote src/widgets/message_view.rs"),
                content: Arc::from(diagnostic_source(i)),
                diagnostics: demo_diagnostics(i),
            }),
            AssistantPart::Tool(DemoToolCall::Diff {
                title: Arc::from("Edit src/widgets/message_view.rs"),
                before: Arc::from(before_a),
                after: Arc::from(after_a),
                wrap: true,
                split: false,
                context_lines: None,
            }),
            AssistantPart::Markdown(markdown),
        ],
        4 => vec![
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("%"),
                pending: Arc::from("Fetching from the web..."),
                content: Arc::from(
                    "WebFetch https://github.com/sst/opencode/blob/main/packages/tui/components/message.tsx",
                ),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("o"),
                pending: Arc::from("Searching code..."),
                content: Arc::from(
                    "Exa Code Search \"terminal diff view documentview\" (8 results)",
                ),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("o"),
                pending: Arc::from("Searching web..."),
                content: Arc::from("Exa Web Search \"rust tui diffview performance\" (5 results)"),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from(">"),
                pending: Arc::from("Loading skill..."),
                content: Arc::from("Skill \"tui-lipan-widget\""),
                complete: true,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Markdown(markdown),
        ],
        5 => vec![
            AssistantPart::Tool(DemoToolCall::BashBlock {
                title: Arc::from("# Inspect perf report"),
                command: Arc::from("perf report --stdio"),
                output: Arc::from(perf_output(i)),
                running: false,
            }),
            AssistantPart::Tool(DemoToolCall::Generic {
                title: Arc::from("# trace_scroll summary"),
                output: Arc::from(generic_output(i)),
            }),
            AssistantPart::Agent(Arc::from(if i % 2 == 0 { "reviewer" } else { "planner" })),
            AssistantPart::Retry {
                attempt: 2 + (i % 3),
            },
            AssistantPart::Markdown(markdown),
        ],
        6 => vec![
            AssistantPart::Reasoning(reasoning),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("$"),
                pending: Arc::from("Writing command..."),
                content: Arc::from("bash command=\"cargo test --all-features\""),
                complete: false,
                error: None,
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("<"),
                pending: Arc::from("Preparing edit..."),
                content: Arc::from(
                    "Edit src/widgets/message_view.rs [oldString=scrollbar_gap, newString=gap]",
                ),
                complete: true,
                error: Some(Arc::from("2 tests still fail in message_view::scroll_perf")),
                highlight: InlineHighlight::Normal,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("!"),
                pending: Arc::from("Awaiting permission..."),
                content: Arc::from("Write /tmp/perf-capture.svg"),
                complete: true,
                error: Some(Arc::from("Permission denied by workspace rules")),
                highlight: InlineHighlight::Denied,
            }),
            AssistantPart::Tool(DemoToolCall::Inline {
                icon: Arc::from("!"),
                pending: Arc::from("Need approval..."),
                content: Arc::from("Read /var/log/perf.log"),
                complete: true,
                error: None,
                highlight: InlineHighlight::Warning,
            }),
            AssistantPart::Markdown(markdown),
        ],
        _ => vec![
            AssistantPart::Tool(DemoToolCall::Diff {
                title: Arc::from("Patch examples/scroll_view_opencode_repro.rs"),
                before: Arc::from(before_a),
                after: Arc::from(after_a),
                wrap: true,
                split: true,
                context_lines: None,
            }),
            AssistantPart::Tool(DemoToolCall::Diff {
                title: Arc::from("Patch src/widgets/scroll_view/mod.rs"),
                before: Arc::from(before_b),
                after: Arc::from(after_b),
                wrap: true,
                split: false,
                context_lines: None,
            }),
            AssistantPart::Tool(DemoToolCall::Generic {
                title: Arc::from("# release_notes"),
                output: Arc::from(
                    "Summary:\n- added diff-view rows to the repro\n- mirrored queued user badges\n- covered todos, questions, and task cards",
                ),
            }),
            AssistantPart::Subtask {
                description: Arc::from("Open child session to inspect flamegraph deltas"),
                toolcalls: 2,
                current_tool: Some(Arc::from("Bash perf record")),
                key_hint: Some(Arc::from("ctrl+j")),
                failed: i % 16 == 7,
            },
            AssistantPart::Markdown(markdown),
        ],
    }
}

fn build_diff_test_parts() -> Vec<AssistantPart> {
    let (before_a, after_a) = diff_pair(0);
    let (before_b, after_b) = diff_pair(1);
    let (before_c, after_c) = diff_pair(2);

    vec![
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Large: src/session/scroll_metrics.rs"),
            before: Arc::from(LARGE_DIFF_SCROLL_METRICS_BEFORE),
            after: Arc::from(LARGE_DIFF_SCROLL_METRICS_AFTER),
            wrap: true,
            split: true,
            context_lines: Some(4),
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Large: src/app/runner/dirty_pipeline.rs"),
            before: Arc::from(LARGE_DIFF_DIRTY_PIPELINE_BEFORE),
            after: Arc::from(LARGE_DIFF_DIRTY_PIPELINE_AFTER),
            wrap: true,
            split: true,
            context_lines: Some(4),
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Diff A: render_assistant_part match"),
            before: Arc::from(before_a),
            after: Arc::from(after_a),
            wrap: true,
            split: true,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Diff B: ScrollView timeline"),
            before: Arc::from(before_b),
            after: Arc::from(after_b),
            wrap: true,
            split: false,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Diff C: DiffView builder options"),
            before: Arc::from(before_c),
            after: Arc::from(after_c),
            wrap: false,
            split: false,
            context_lines: None,
        }),
    ]
}

const DEMO_PATCH_UNIFIED_LIB: &str = concat!(
    "diff --git a/src/lib.rs b/src/lib.rs\n",
    "--- a/src/lib.rs\n",
    "+++ b/src/lib.rs\n",
    "@@ -1,13 +1,15 @@\n",
    "+// DiffView::from_patch - added line 1\n",
    "+// DiffView::from_patch - added line 2\n",
    " // unchanged above 1\n",
    " // unchanged above 2\n",
    " // unchanged above 3\n",
    " // unchanged above 4\n",
    " // unchanged above 5\n",
    " pub fn demo() -> u32 {\n",
    "     1\n",
    " }\n",
    " // unchanged below 1\n",
    " // unchanged below 2\n",
    " // unchanged below 3\n",
    " // unchanged below 4\n",
    " // unchanged below 5\n",
);

const DEMO_PATCH_REPLACE: &str = concat!(
    "diff --git a/src/config.rs b/src/config.rs\n",
    "--- a/src/config.rs\n",
    "+++ b/src/config.rs\n",
    "@@ -1,15 +1,15 @@\n",
    " // above 1\n",
    " // above 2\n",
    " // above 3\n",
    " // above 4\n",
    " // above 5\n",
    " pub const FLAG: bool = true;\n",
    " \n",
    "-pub fn old_entry() -> u32 {\n",
    "+pub fn new_entry() -> u32 {\n",
    "     42\n",
    " }\n",
    " // below 1\n",
    " // below 2\n",
    " // below 3\n",
    " // below 4\n",
    " // below 5\n",
);

const DEMO_PATCH_TWO_HUNKS: &str = concat!(
    "diff --git a/src/worker.rs b/src/worker.rs\n",
    "--- a/src/worker.rs\n",
    "+++ b/src/worker.rs\n",
    "@@ -1,8 +1,9 @@\n",
    "+// inserted line\n",
    " // ctx 1\n",
    " // ctx 2\n",
    " // ctx 3\n",
    " // ctx 4\n",
    " // ctx 5\n",
    " pub fn first() -> u32 {\n",
    "     1\n",
    " }\n",
    "@@ -9,5 +10,5 @@\n",
    " // mid 1\n",
    " // mid 2\n",
    " pub fn third() -> u32 {\n",
    "-    0\n",
    "+    99\n",
    " }\n",
);

fn build_diff_patch_parts() -> Vec<AssistantPart> {
    vec![
        AssistantPart::Tool(DemoToolCall::DiffFromPatch {
            title: Arc::from("# from_patch: src/lib.rs (+2 lines at top)"),
            patch: Arc::from(DEMO_PATCH_UNIFIED_LIB),
        }),
        AssistantPart::Tool(DemoToolCall::DiffFromPatch {
            title: Arc::from("# from_patch: src/config.rs (rename hunk)"),
            patch: Arc::from(DEMO_PATCH_REPLACE),
        }),
        AssistantPart::Tool(DemoToolCall::DiffFromPatch {
            title: Arc::from("# from_patch: src/worker.rs (two hunks, one file)"),
            patch: Arc::from(DEMO_PATCH_TWO_HUNKS),
        }),
    ]
}

fn build_diff_heavy_parts(i: usize) -> Vec<AssistantPart> {
    let (before_a, after_a) = diff_pair(i);
    let (before_b, after_b) = diff_pair(i + 1);
    let (before_c, after_c) = diff_pair(i + 2);

    vec![
        AssistantPart::Reasoning(Arc::from(reasoning_template(i))),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("# Diff tool: src/session/scroll_metrics.rs (split, wrap, context 4)"),
            before: Arc::from(LARGE_DIFF_SCROLL_METRICS_BEFORE),
            after: Arc::from(LARGE_DIFF_SCROLL_METRICS_AFTER),
            wrap: true,
            split: true,
            context_lines: Some(4),
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from(
                "# Diff tool: src/app/runner/dirty_pipeline.rs (split, wrap, context 4)",
            ),
            before: Arc::from(LARGE_DIFF_DIRTY_PIPELINE_BEFORE),
            after: Arc::from(LARGE_DIFF_DIRTY_PIPELINE_AFTER),
            wrap: true,
            split: true,
            context_lines: Some(4),
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Patch src/widgets/message_view.rs"),
            before: Arc::from(before_a),
            after: Arc::from(after_a),
            wrap: i % 2 == 0,
            split: true,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Patch src/screens/session.rs"),
            before: Arc::from(before_b),
            after: Arc::from(after_b),
            wrap: true,
            split: false,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Patch src/widgets/tool_part_view.rs"),
            before: Arc::from(before_c),
            after: Arc::from(after_c),
            wrap: i % 3 != 0,
            split: i % 2 != 0,
            context_lines: None,
        }),
        AssistantPart::Markdown(Arc::from(markdown_template(i))),
    ]
}

fn build_patch_stack_parts(i: usize) -> Vec<AssistantPart> {
    let (before_a, after_a) = diff_pair(i);
    let (before_b, after_b) = diff_pair(i + 1);
    let (before_c, after_c) = diff_pair(i + 2);

    vec![
        AssistantPart::Tool(DemoToolCall::Generic {
            title: Arc::from("# apply_patch (3 files)"),
            output: Arc::from(
                "*** Begin Patch\n*** Update File: src/widgets/message_view.rs\n*** Update File: src/widgets/tool_part_view.rs\n*** Update File: src/screens/session.rs\n*** End Patch",
            ),
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Update src/widgets/message_view.rs"),
            before: Arc::from(before_a),
            after: Arc::from(after_a),
            wrap: true,
            split: true,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Update src/widgets/tool_part_view.rs"),
            before: Arc::from(before_b),
            after: Arc::from(after_b),
            wrap: false,
            split: false,
            context_lines: None,
        }),
        AssistantPart::Tool(DemoToolCall::Diff {
            title: Arc::from("Update src/screens/session.rs"),
            before: Arc::from(before_c),
            after: Arc::from(after_c),
            wrap: true,
            split: true,
            context_lines: None,
        }),
        AssistantPart::Subtask {
            description: Arc::from("Validate multi-file patch stack layout and scroll cost"),
            toolcalls: 4,
            current_tool: Some(Arc::from("DiffView render pass")),
            key_hint: Some(Arc::from("ctrl+j")),
            failed: i % 12 == 5,
        },
    ]
}

fn build_text_heavy_parts(i: usize) -> Vec<AssistantPart> {
    vec![
        AssistantPart::Tool(DemoToolCall::Inline {
            icon: Arc::from("!"),
            pending: Arc::from("Preparing error summary..."),
            content: Arc::from(multiline_text_tool_output(i)),
            complete: true,
            error: Some(Arc::from(multiline_inline_error(i))),
            highlight: InlineHighlight::Warning,
        }),
        AssistantPart::Tool(DemoToolCall::Questions {
            items: vec![
                DemoQuestion {
                    question: Arc::from(multiline_question(i)),
                    answer: Arc::from(multiline_answer(i)),
                },
                DemoQuestion {
                    question: Arc::from("What widget is rendering this block?"),
                    answer: Arc::from(
                        "Mostly Text rows with Wrap, plus footer metadata and inline tool labels.",
                    ),
                },
            ],
        }),
        AssistantPart::Retry {
            attempt: 1 + (i % 4),
        },
        AssistantPart::Agent(Arc::from("reviewer")),
    ]
}

fn build_error_heavy_parts(i: usize) -> Vec<AssistantPart> {
    vec![
        AssistantPart::Tool(DemoToolCall::BashBlock {
            title: Arc::from("# cargo clippy --all-targets --all-features -- -D warnings"),
            command: Arc::from("cargo clippy --all-targets --all-features -- -D warnings"),
            output: Arc::from(multiline_clippy_output(i)),
            running: false,
        }),
        AssistantPart::Tool(DemoToolCall::Diagnostics {
            title: Arc::from("# Wrote src/app.rs"),
            content: Arc::from(diagnostic_source(i)),
            diagnostics: demo_diagnostics(i),
        }),
        AssistantPart::Tool(DemoToolCall::Inline {
            icon: Arc::from("<"),
            pending: Arc::from("Preparing edit..."),
            content: Arc::from(
                "Edit src/app.rs [oldString=workspace_id, newString=workspace_id.as_deref()]",
            ),
            complete: true,
            error: Some(Arc::from(multiline_apply_patch_error(i))),
            highlight: InlineHighlight::Normal,
        }),
        AssistantPart::Markdown(Arc::from(
            "The failing rows below are intentionally noisy and multiline so you can isolate error-heavy message behavior.",
        )),
    ]
}

fn assistant_error(i: usize) -> Option<Arc<str>> {
    match i % 8 {
        3 => Some(Arc::from(
            "cargo test exited with status 101 after the write diagnostic pass.",
        )),
        6 if i % 4 == 2 => Some(Arc::from(
            "Full-suite validation still fails after the last edit; triage the remaining regressions.",
        )),
        _ => None,
    }
}

fn text_heavy_user_template(i: usize) -> &'static str {
    const TEMPLATES: [&str; 4] = [
        "Please print the exact stderr from the failed tool call, including every wrapped line and indentation level.",
        "I need the raw error output first. Do not summarize it yet; keep the original newlines so I can compare the layout.",
        "Can you show the patch failure exactly as returned by apply_patch, then add one short sentence underneath?",
        "Let us isolate the Text widget path: multiline labels, warnings, metadata rows, and noisy error output without DiffView.",
    ];
    TEMPLATES[i % TEMPLATES.len()]
}

fn error_heavy_user_template(i: usize) -> &'static str {
    const TEMPLATES: [&str; 4] = [
        "The last write failed. Show me the exact compiler and apply_patch errors inline in the timeline.",
        "Focus on the red failure rows only. I want the noisiest message shapes that still happen in the real session screen.",
        "Please rerun the patch and print the raw mismatch block so I can see whether newlines or wrapping are the issue.",
        "I suspect only some sessions go bad because of multiline error rows. Build a repro around those shapes.",
    ];
    TEMPLATES[i % TEMPLATES.len()]
}

fn multiline_error_text(i: usize) -> &'static str {
    const OUTPUTS: [&str; 3] = [
        "Error: apply_patch verification failed\n\nExpected:\n    let workspace_id = current_client_for_state(&self, &state);\nFound:\n    let workspace_id = current_workspace_id(&state);\n\nHint: reread the file before editing.",
        "Validation failed while updating the session view\n\n- 2 snapshot assertions changed\n- 1 multiline stderr block no longer wraps correctly\n- retry with exact context before reapplying the patch",
        "cargo test failed\n\nthread 'session::tests::render_errors' panicked at\nassertion `left == right` failed\nleft:  14 visual lines\nright: 9 visual lines",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_apply_patch_error(i: usize) -> &'static str {
    const OUTPUTS: [&str; 3] = [
        "Error: apply_patch verification failed: Expected to find these lines in /home/user/Work/Projects/opencode-tui/src/app.rs:\n    let workspace_id = current_client_for_state(&self, &state);\n    Arc::new(self.client.with_context(state.resolved_workspace_dir().as_str(), workspace_id.as_deref(), ))\nBut they were not found in the current file.",
        "Error: patch rejected while updating src/widgets/tool_part_view.rs\n\n@@\n-    content: old_output,\n+    content: new_output,\n\nThe surrounding context changed after the previous edit.",
        "Error: edit_file failed\n\noldString matched 0 occurrences\nnewString was not applied\n\nTry reading a larger window before retrying the edit.",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_text_tool_output(i: usize) -> &'static str {
    const OUTPUTS: [&str; 3] = [
        "Tool summary:\n- opened 3 files\n- found 2 matching blocks\n- preserved current scroll offset\n- invalidated line-wrap cache for resized rows",
        "Renderer notes:\nLine 1 remains visible\nLine 2 wraps because of the viewport width\nLine 3 should keep its indent\nLine 4 stresses multiline Text rendering",
        "Status:\nmessage_view.rs updated\nscroll_view_opencode_repro.rs updated\nfollow-up: profile resize again with the sidebar open",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_inline_error(i: usize) -> &'static str {
    const OUTPUTS: [&str; 2] = [
        "warning: line 2 exceeded the viewport width\nwarning: line 3 inserted a soft wrap\nerror: line 4 lost its visual separation",
        "error: failed to keep exact newline boundaries\nhelp: compare measured height vs rendered lines\nhelp: isolate Text-only rows in the repro",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_question(i: usize) -> &'static str {
    const OUTPUTS: [&str; 2] = [
        "Which rows still look expensive after removing DiffView-heavy content?\nPlease name the exact message surfaces.",
        "Does the lag only appear when the message contains raw multiline error text\nor also when it contains wrapped metadata rows?",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_answer(i: usize) -> &'static str {
    const OUTPUTS: [&str; 2] = [
        "So far the worst candidates are:\n- multiline Text error blocks\n- nested Frame wrappers\n- repeated metadata rows\n- any row that mixes all three near resize",
        "The next step is to switch presets at the top:\n1. Text Heavy\n2. Error Heavy\n3. Diff Heavy\nand compare how resize feels in each.",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn multiline_clippy_output(i: usize) -> &'static str {
    const OUTPUTS: [&str; 2] = [
        "error: this `map_or` can be simplified\n   --> src/backend/ratatui_backend/render.rs:132:14\n    |\n132 |     mouse_pos.map_or(false, |(mx, my)| rect.contains(mx as i16, my as i16))\n    |              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n    |\n    = help: use `is_some_and(...)` instead\n\nerror: could not compile `tui-lipan` (lib) due to previous error",
        "warning: variable does not need to be mutable\n   --> examples/scroll_view_opencode_repro.rs:420:13\n\nerror[E0716]: temporary value dropped while borrowed\n   --> src/backend/ratatui_backend/renderers/text.rs:216:14\n\nerror: aborting due to 1 previous error; 1 warning emitted",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn user_template(i: usize) -> &'static str {
    const TEMPLATES: [&str; 6] = [
        "Can you inspect the ScrollView slowdown in the session screen? The stress example feels fine but the real app still stutters.",
        "Please compare the real message timeline against our framework repro. I want the same nesting depth, not just the same row count.",
        "Let's mirror every message surface: diffs, todos, questions, task cards, file badges, and queued user rows.",
        "The stress example feels smooth. The opencode screen still spikes when a diff view or tool block enters the viewport.",
        "Focus on message_view.rs first. I suspect nested DocumentView and DiffView trees are still doing too much work while scrolling.",
        "Add the missing message shapes to the repro, then capture another flamegraph if CPU is still high.",
    ];
    TEMPLATES[i % TEMPLATES.len()]
}

fn markdown_template(i: usize) -> &'static str {
    const TEMPLATES: [&str; 5] = [
        "Here is the current theory:\n\n- `ScrollView` no longer needs a full `view()` rebuild on wheel scroll\n- `DocumentView` markdown is still expensive when large subtrees are re-hashed\n- diff blocks add more nested layout work than the synthetic stress case\n\n```rust\nmatch dirty {\n    DirtyLevel::Layout => reconcile_cached_tree(),\n    DirtyLevel::Full => rebuild_view(),\n    DirtyLevel::Paint => paint_only(),\n}\n```",
        "## Session timeline\n\nThe session screen is small, but each assistant row can contain multiple parts:\n\n1. rich markdown\n2. reasoning notes\n3. tool output blocks and diffs\n4. task cards, todos, and question groups\n5. footer metadata\n\nThat means fewer rows can still beat the stress test in total work.",
        "> The stress example mostly uses a single `Frame -> VStack -> DocumentView` shape.\n>\n> The real session has additional `Text`, `HStack`, `Divider`, status rows, and repeated nested `DocumentView`s for tool output and diff renderers.\n\nWe should reproduce the *structure*, not only the count.",
        "A wider message often includes tables too:\n\n| widget | count | notes |\n|--------|-------|-------|\n| Frame | many | left accent + padding |\n| DocumentView | many | markdown + plain output |\n| DiffView | some | unified + split |\n| Text | many | metadata, labels, badges |",
        "The latest repro now exercises:\n\n- inline tool rows\n- read trees with loaded-file children\n- diagnostics blocks with line numbers\n- task cards and subtask rows\n- multiple diff surfaces inside one assistant message",
    ];
    TEMPLATES[i % TEMPLATES.len()]
}

fn reasoning_template(i: usize) -> &'static str {
    const TEMPLATES: [&str; 3] = [
        "_Thinking:_ keep the scroll callback cheap and make layout-only reconcile reuse cached subtree hashes.",
        "_Thinking:_ a good repro should include markdown, diff views, task cards, footers, and roughly the same nesting depth as the session screen.",
        "_Thinking:_ if the heavy CPU only appears with mixed message parts, the repro needs those exact shapes in one scrolling stack.",
    ];
    TEMPLATES[i % TEMPLATES.len()]
}

/// Long before/after pair (~50 lines each) with edits across ~30 lines and distant unchanged spans
/// so `context_lines(4)` can collapse middle regions in Diff Heavy tool rows.
const LARGE_DIFF_SCROLL_METRICS_BEFORE: &str = r#"// src/session/scroll_metrics.rs - timeline layout helpers
use std::cmp::{max, min};
use std::sync::Arc;

pub const TIMELINE_V_GAP: u16 = 1;
pub const SIDEBAR_WIDE_BREAKPOINT: u16 = 120;

#[derive(Clone, Copy)]
pub enum SidebarMode {
    Auto,
    Hide,
}

pub fn sidebar_should_show(width: u16, mode: SidebarMode, user_toggled_open: bool) -> bool {
    user_toggled_open || (matches!(mode, SidebarMode::Auto) && width > SIDEBAR_WIDE_BREAKPOINT)
}

pub fn clamp_scroll_offset(offset: usize, max_offset: usize) -> usize {
    min(offset, max_offset)
}

pub fn estimate_row_height(lines: usize, line_height: u16) -> u16 {
    (lines as u16).saturating_mul(line_height).saturating_add(TIMELINE_V_GAP)
}

pub fn merge_adjacent_gaps(a: u16, b: u16) -> u16 {
    max(a, b)
}

pub fn timeline_inner_width(viewport: u16, chrome: u16) -> u16 {
    viewport.saturating_sub(chrome)
}

pub fn scrollbar_reserve(enabled: bool) -> u16 {
    if enabled {
        1
    } else {
        0
    }
}

pub fn pick_scroll_step(visible: usize, requested: usize) -> usize {
    min(visible, requested).max(1)
}

pub fn format_status_row(rows: usize, label: &str, viewport_w: u16) -> String {
    format!("{rows} rows | {label} | w={viewport_w}")
}

pub fn debug_log_scroll(viewport: u16, offset: usize, max_offset: usize) {
    eprintln!("scroll: viewport={viewport} offset={offset} max={max_offset}");
}
"#;

const LARGE_DIFF_SCROLL_METRICS_AFTER: &str = r#"// src/session/scroll_metrics.rs - timeline layout helpers
use std::cmp::min;
use std::sync::Arc;

pub const TIMELINE_V_GAP: u16 = 1;
pub const SIDEBAR_WIDE_BREAKPOINT: u16 = 120;

#[derive(Clone, Copy)]
pub enum SidebarMode {
    Auto,
    Hide,
}

pub fn sidebar_should_show(width: u16, mode: SidebarMode, user_toggled_open: bool) -> bool {
    user_toggled_open || (matches!(mode, SidebarMode::Auto) && width > SIDEBAR_WIDE_BREAKPOINT)
}

pub fn clamp_scroll_offset(offset: usize, max_offset: usize) -> usize {
    offset.min(max_offset)
}

pub fn estimate_row_height(lines: usize, line_height: u16) -> u16 {
    lines
        .saturating_mul(line_height as usize)
        .saturating_add(TIMELINE_V_GAP as usize) as u16
}

pub fn merge_adjacent_gaps(a: u16, b: u16) -> u16 {
    a.saturating_add(b).min(8)
}

pub fn timeline_inner_width(viewport: u16, chrome: u16) -> u16 {
    viewport.saturating_sub(chrome.saturating_mul(2))
}

pub fn scrollbar_reserve(enabled: bool) -> u16 {
    u16::from(enabled) * 2
}

pub fn pick_scroll_step(visible: usize, requested: usize) -> usize {
    let base = visible.max(1);
    (base / 2).max(1).min(requested.max(1))
}

pub fn format_status_row(rows: usize, label: &str, viewport_w: u16) -> String {
    format!("timeline {rows} · {label} · width {viewport_w}")
}

pub fn debug_log_scroll(viewport: u16, offset: usize, max_offset: usize) {
    tracing::debug!(viewport, offset, max_offset, "scroll_metrics");
}
"#;

const LARGE_DIFF_DIRTY_PIPELINE_BEFORE: &str = r#"// src/app/runner/dirty_pipeline.rs
use crate::render::DirtyLevel;

pub struct DirtyPipeline {
    pub level: DirtyLevel,
    pub layout_generation: u64,
}

impl DirtyPipeline {
    pub fn new() -> Self {
        Self {
            level: DirtyLevel::Full,
            layout_generation: 0,
        }
    }

    pub fn mark_paint_only(&mut self) {
        self.level = DirtyLevel::Paint;
    }

    pub fn mark_layout(&mut self) {
        self.level = DirtyLevel::Layout;
        self.layout_generation = self.layout_generation.wrapping_add(1);
    }

    pub fn mark_full(&mut self) {
        self.level = DirtyLevel::Full;
        self.layout_generation = self.layout_generation.wrapping_add(1);
    }

    pub fn should_reconcile_layout(&self) -> bool {
        matches!(self.level, DirtyLevel::Layout | DirtyLevel::Full)
    }

    pub fn should_rebuild_view(&self) -> bool {
        matches!(self.level, DirtyLevel::Full)
    }

    pub fn cheap_scroll_path(&self) -> bool {
        matches!(self.level, DirtyLevel::Paint)
    }
}

pub fn coalesce_dirty(a: DirtyLevel, b: DirtyLevel) -> DirtyLevel {
    use DirtyLevel::*;
    match (a, b) {
        (Full, _) | (_, Full) => Full,
        (Layout, _) | (_, Layout) => Layout,
        _ => Paint,
    }
}

pub fn bump_generation(gen: u64) -> u64 {
    gen.wrapping_add(1)
}
"#;

const LARGE_DIFF_DIRTY_PIPELINE_AFTER: &str = r#"// src/app/runner/dirty_pipeline.rs
use crate::render::DirtyLevel;

pub struct DirtyPipeline {
    pub level: DirtyLevel,
    pub layout_generation: u64,
    pub paint_serial: u32,
}

impl DirtyPipeline {
    pub fn new() -> Self {
        Self {
            level: DirtyLevel::Full,
            layout_generation: 0,
            paint_serial: 0,
        }
    }

    pub fn mark_paint_only(&mut self) {
        self.level = DirtyLevel::Paint;
        self.paint_serial = self.paint_serial.wrapping_add(1);
    }

    pub fn mark_layout(&mut self) {
        self.level = DirtyLevel::Layout;
        self.layout_generation = self.layout_generation.wrapping_add(1);
    }

    pub fn mark_full(&mut self) {
        self.level = DirtyLevel::Full;
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.paint_serial = 0;
    }

    pub fn should_reconcile_layout(&self) -> bool {
        matches!(
            self.level,
            DirtyLevel::Layout | DirtyLevel::Full
        )
    }

    pub fn should_rebuild_view(&self) -> bool {
        matches!(self.level, DirtyLevel::Full)
    }

    pub fn cheap_scroll_path(&self) -> bool {
        matches!(self.level, DirtyLevel::Paint) && self.paint_serial > 0
    }
}

pub fn coalesce_dirty(a: DirtyLevel, b: DirtyLevel) -> DirtyLevel {
    use DirtyLevel::*;
    match (a, b) {
        (Full, _) | (_, Full) => Full,
        (Layout, Layout) => Layout,
        (Layout, Paint) | (Paint, Layout) => Layout,
        _ => Paint,
    }
}

pub fn bump_generation(gen: u64) -> u64 {
    gen.saturating_add(1)
}
"#;

fn diff_pair(i: usize) -> (&'static str, &'static str) {
    const BEFORE_A: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
fn render_assistant_part(part: &Part) -> Option<Element> {
    match part {
        Part::Text(text) => render_markdown(text.text.trim()),
        Part::Tool(tool) => render_tool(tool),
        _ => None,
    }
}
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;
    const AFTER_A: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
fn render_assistant_part(part: &Part) -> Option<Element> {
    match part {
        Part::Text(text) => render_markdown(text.text.trim()),
        Part::Reasoning(reasoning) => render_reasoning(reasoning),
        Part::Tool(tool) => render_tool(tool),
        Part::Subtask(task) => render_subtask(task),
        Part::Agent(agent) => render_agent(agent),
        Part::Retry(retry) => render_retry(retry),
        _ => None,
    }
}
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;
    const BEFORE_B: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
let timeline = ScrollView::new()
    .scrollbar(true)
    .padding(1)
    .children(messages.iter().map(render_message));
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;
    const AFTER_B: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
let timeline = ScrollView::new()
    .scrollbar(true)
    .scrollbar_gap(1)
    .padding(1)
    .children(messages.iter().map(render_message));
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;
    const BEFORE_C: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
fn default_diff(before: &str, after: &str) -> DiffView {
    DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .wrap(true)
        .scrollbar(false)
        .focusable(false)
}
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;
    const AFTER_C: &str = r#"// unchanged prefix line 1
// unchanged prefix line 2
// unchanged prefix line 3
// unchanged prefix line 4
// unchanged prefix line 5
fn default_diff(before: &str, after: &str) -> DiffView {
    DiffView::with_content(before, after)
        .backend(DiffViewBackend::DocumentView)
        .mode(DiffViewMode::Split)
        .wrap(false)
        .h_scrollbar(true)
        .scrollbar(false)
        .focusable(false)
}
// unchanged suffix line 1
// unchanged suffix line 2
// unchanged suffix line 3
// unchanged suffix line 4
// unchanged suffix line 5
"#;

    const BEFORE_A_LONG: &str = r#"// src/render.rs
use crate::prelude::*;

pub const RENDER_VERSION: u32 = 7;

pub fn normalize_part(part: &Part) -> &Part {
    part
}

fn render_assistant_part(part: &Part) -> Option<Element> {
    match part {
        Part::Text(text) => render_markdown(text.text.trim()),
        Part::Tool(tool) => render_tool(tool),
        _ => None,
    }
}

pub fn tail_marker() -> usize {
    0
}

pub fn validate_stack_depth(depth: usize) -> bool {
    depth < 128
}
"#;
    const AFTER_A_LONG: &str = r#"// src/render.rs
use crate::prelude::*;

pub const RENDER_VERSION: u32 = 7;

pub fn normalize_part(part: &Part) -> &Part {
    part
}

fn render_assistant_part(part: &Part) -> Option<Element> {
    match part {
        Part::Text(text) => render_markdown(text.text.trim()),
        Part::Reasoning(reasoning) => render_reasoning(reasoning),
        Part::Tool(tool) => render_tool(tool),
        Part::Subtask(task) => render_subtask(task),
        Part::Agent(agent) => render_agent(agent),
        Part::Retry(retry) => render_retry(retry),
        _ => None,
    }
}

pub fn tail_marker() -> usize {
    0
}

pub fn validate_stack_depth(depth: usize) -> bool {
    depth < 128
}
"#;
    const BEFORE_B_LONG: &str = r#"// src/screens/session.rs
use crate::widgets::*;

pub fn build_timeline(messages: &[Message]) -> Element {
    let header = render_header();

    let timeline = ScrollView::new()
        .scrollbar(true)
        .padding(1)
        .children(messages.iter().map(render_message));

    VStack::new()
        .child(header)
        .child(timeline)
}

pub fn session_footer() -> &'static str {
    "footer"
}

pub fn timeline_id() -> u64 {
    0xfeed
}
"#;
    const AFTER_B_LONG: &str = r#"// src/screens/session.rs
use crate::widgets::*;

pub fn build_timeline(messages: &[Message]) -> Element {
    let header = render_header();

    let timeline = ScrollView::new()
        .scrollbar(true)
        .scrollbar_gap(1)
        .padding(1)
        .children(messages.iter().map(render_message));

    VStack::new()
        .child(header)
        .child(timeline)
}

pub fn session_footer() -> &'static str {
    "footer"
}

pub fn timeline_id() -> u64 {
    0xfeed
}
"#;
    const BEFORE_C_LONG: &str = r#"// src/widgets/diff_view.rs
use crate::prelude::*;

pub fn default_diff_view(before: &str, after: &str) -> DiffView {
    DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .wrap(true)
        .scrollbar(false)
        .focusable(false)
}

pub fn shared_diff_id() -> &'static str {
    "messages"
}

pub fn diff_view_schema_rev() -> u16 {
    3
}
"#;
    const AFTER_C_LONG: &str = r#"// src/widgets/diff_view.rs
use crate::prelude::*;

pub fn default_diff_view(before: &str, after: &str) -> DiffView {
    DiffView::with_content(before, after)
        .backend(DiffViewBackend::DocumentView)
        .mode(DiffViewMode::Split)
        .wrap(false)
        .h_scrollbar(true)
        .scrollbar(false)
        .focusable(false)
}

pub fn shared_diff_id() -> &'static str {
    "messages"
}

pub fn diff_view_schema_rev() -> u16 {
    3
}
"#;

    match i % 6 {
        0 => (BEFORE_A, AFTER_A),
        1 => (BEFORE_B, AFTER_B),
        2 => (BEFORE_C, AFTER_C),
        3 => (BEFORE_A_LONG, AFTER_A_LONG),
        4 => (BEFORE_B_LONG, AFTER_B_LONG),
        _ => (BEFORE_C_LONG, AFTER_C_LONG),
    }
}

fn bash_output(i: usize) -> &'static str {
    const OUTPUTS: [&str; 3] = [
        "running 1 test\ntest session::tests::scroll_perf ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored",
        "src/widgets/message_view.rs:314\nDocumentView::new(content)\n  .markdown()\n  .border(false)\n  .scrollbar(false)\n\nsrc/screens/session.rs:348\nScrollView::new()\n  .offset(data.scroll_offset)\n  .on_scroll(data.on_scroll.clone())",
        "diff --git a/src/app/runner/mod.rs b/src/app/runner/mod.rs\n@@\n- dirty.mark_full();\n+ dirty.mark_layout();\n\nlayout-only reconcile is enough when the node already mutated its scroll offset.",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn perf_output(i: usize) -> &'static str {
    const OUTPUTS: [&str; 2] = [
        "Samples: 129K of event 'cycles:u'\n  41.2% tui_lipan  [.] Buffer::diff\n  23.8% tui_lipan  [.] render_message\n  14.7% tui_lipan  [.] DiffView::render\n  11.9% tui_lipan  [.] DocumentView::render",
        "Samples: 141K of event 'cycles:u'\n  36.4% tui_lipan  [.] render_tool_part\n  22.6% tui_lipan  [.] layout_scroll_content_cached\n  19.5% tui_lipan  [.] DiffView::compose_panels\n  10.2% tui_lipan  [.] Buffer::diff",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn generic_output(i: usize) -> &'static str {
    const OUTPUTS: [&str; 3] = [
        "DirtyLevel::Layout -> 91%\nBuffer::diff -> 7%\npaint_only -> 2%",
        "The repro now covers queued rows, diffs, diagnostics, and task cards in one timeline.",
        "Next step: scroll the mixed-message stack and compare the flamegraph against opencode-tui.",
    ];
    OUTPUTS[i % OUTPUTS.len()]
}

fn diagnostic_source(i: usize) -> &'static str {
    const SOURCES: [&str; 2] = [
        "fn render_message_row(row: &MessageRow) -> Element {\n    let available_width = row.width.saturating_sub(4);\n    render_assistant_message_row(row, available_width)\n}\n",
        "fn render_tool_part(tool: &ToolPart) -> Element {\n    if should_show_diff(tool) {\n        return render_diff_tool(tool);\n    }\n    render_inline_tool(tool)\n}\n",
    ];
    SOURCES[i % SOURCES.len()]
}

fn demo_todos(i: usize) -> Vec<DemoTodo> {
    vec![
        DemoTodo {
            content: Arc::from("Mirror diff rows in the repro"),
            status: DemoTodoStatus::Completed,
        },
        DemoTodo {
            content: Arc::from("Add question and task cards"),
            status: DemoTodoStatus::Completed,
        },
        DemoTodo {
            content: Arc::from(if i % 2 == 0 {
                "Capture another scroll flamegraph"
            } else {
                "Compare repro against message_view.rs"
            }),
            status: DemoTodoStatus::InProgress,
        },
        DemoTodo {
            content: Arc::from("Trim any shapes that do not affect perf"),
            status: DemoTodoStatus::Pending,
        },
    ]
}

fn demo_questions(i: usize) -> Vec<DemoQuestion> {
    vec![
        DemoQuestion {
            question: Arc::from("Which message shape still burns the most CPU?"),
            answer: Arc::from(if i % 2 == 0 {
                "Mixed DiffView + DocumentView rows near the viewport edge."
            } else {
                "Large tool blocks with nested markdown and diff surfaces."
            }),
        },
        DemoQuestion {
            question: Arc::from("What should the next measurement include?"),
            answer: Arc::from("A scroll trace with the sidebar open and all tool groups visible."),
        },
    ]
}

fn demo_diagnostics(i: usize) -> Vec<DemoDiagnostic> {
    vec![
        DemoDiagnostic {
            line: 18 + (i % 4),
            col: 9,
            message: Arc::from("expected scroll metrics to stay in sync with viewport height"),
        },
        DemoDiagnostic {
            line: 44 + (i % 3),
            col: 17,
            message: Arc::from("DiffView path still triggers a full repaint in this branch"),
        },
    ]
}

fn main() -> Result<()> {
    App::new()
        .title("opencode scroll repro")
        .mount(OpencodeScrollRepro)
        .run()
}
