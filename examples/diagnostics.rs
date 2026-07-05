//! Consolidated diagnostics example for render and mouse-event behavior.
//!
//! Sections (top-level tabs):
//! - Active: animated/hoverable widgets
//! - Idle: 0-FPS idle verification
//! - Tabbed: inner tab behavior with hoverables
//!
//! Run with: cargo run --example diagnostics

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tui_lipan::prelude::*;

static ACTIVE_VIEW_COUNT: AtomicUsize = AtomicUsize::new(0);
static IDLE_VIEW_COUNT: AtomicUsize = AtomicUsize::new(0);
static TABBED_VIEW_COUNT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ACTIVE_RENDER_WINDOW: RefCell<RollingWindow> = RefCell::new(RollingWindow::new(2.0));
    static IDLE_RENDER_WINDOW: RefCell<RollingWindow> = RefCell::new(RollingWindow::new(2.0));
    static TABBED_RENDER_WINDOW: RefCell<RollingWindow> = RefCell::new(RollingWindow::new(2.0));
}

struct RollingWindow {
    timestamps: VecDeque<Instant>,
    window_secs: f64,
}

impl RollingWindow {
    fn new(window_secs: f64) -> Self {
        Self {
            timestamps: VecDeque::new(),
            window_secs,
        }
    }

    fn record(&mut self) {
        let now = Instant::now();
        self.timestamps.push_back(now);
        self.prune(now);
    }

    fn prune(&mut self, now: Instant) {
        while let Some(front) = self.timestamps.front() {
            if now.duration_since(*front).as_secs_f64() > self.window_secs {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn rate(&mut self) -> f64 {
        let now = Instant::now();
        self.prune(now);
        if self.timestamps.is_empty() {
            return 0.0;
        }
        self.timestamps.len() as f64 / self.window_secs
    }

    fn count(&self) -> usize {
        self.timestamps.len()
    }

    fn reset(&mut self) {
        self.timestamps.clear();
    }
}

struct Diagnostics;

#[derive(Default)]
struct State {
    section: usize,

    // Active section state
    active_input_value: String,
    active_textarea_value: String,
    active_selected: usize,
    active_slider_value: f64,
    active_show_spinner: bool,

    // Idle section state
    idle_selected: usize,
    idle_last_action: String,

    // Tabbed section state
    tabbed_active_tab: usize,
    tabbed_selected: usize,
}

#[derive(Clone)]
enum Msg {
    SectionChange(usize),

    ActiveInputChange(String),
    ActiveTextAreaChange(String),
    ActiveSelect(usize),
    ActiveSliderChange(f64),
    ActiveToggleSpinner,
    ActiveReset,

    IdleSelect(usize),
    IdleReset,

    TabbedTabChange(usize),
    TabbedSelect(usize),
    TabbedReset,
}

impl Component for Diagnostics {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            section: 0,
            active_input_value: "Type here...".into(),
            active_textarea_value: "Multi-line\ntext area".into(),
            active_selected: 0,
            active_slider_value: 0.5,
            active_show_spinner: false,
            idle_selected: 0,
            idle_last_action: "None".into(),
            tabbed_active_tab: 0,
            tabbed_selected: 0,
        }
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SectionChange(idx) => {
                ctx.state.section = idx;
                Update::full()
            }

            Msg::ActiveInputChange(s) => {
                ctx.state.active_input_value = s;
                Update::full()
            }
            Msg::ActiveTextAreaChange(s) => {
                ctx.state.active_textarea_value = s;
                Update::full()
            }
            Msg::ActiveSelect(idx) => {
                ctx.state.active_selected = idx;
                Update::full()
            }
            Msg::ActiveSliderChange(v) => {
                ctx.state.active_slider_value = v;
                Update::full()
            }
            Msg::ActiveToggleSpinner => {
                ctx.state.active_show_spinner = !ctx.state.active_show_spinner;
                Update::full()
            }
            Msg::ActiveReset => {
                ACTIVE_VIEW_COUNT.store(0, Ordering::SeqCst);
                ACTIVE_RENDER_WINDOW.with(|w| w.borrow_mut().reset());
                tui_lipan::debug::reset_mouse_events();
                Update::full()
            }

            Msg::IdleSelect(idx) => {
                ctx.state.idle_selected = idx;
                ctx.state.idle_last_action = format!("Selected item {}", idx);
                Update::full()
            }
            Msg::IdleReset => {
                IDLE_VIEW_COUNT.store(0, Ordering::SeqCst);
                IDLE_RENDER_WINDOW.with(|w| w.borrow_mut().reset());
                tui_lipan::debug::reset_mouse_events();
                ctx.state.idle_last_action = "Reset counter".into();
                Update::full()
            }

            Msg::TabbedTabChange(idx) => {
                ctx.state.tabbed_active_tab = idx;
                Update::full()
            }
            Msg::TabbedSelect(idx) => {
                ctx.state.tabbed_selected = idx;
                Update::full()
            }
            Msg::TabbedReset => {
                TABBED_VIEW_COUNT.store(0, Ordering::SeqCst);
                TABBED_RENDER_WINDOW.with(|w| w.borrow_mut().reset());
                tui_lipan::debug::reset_mouse_events();
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                ctx.link().send(Msg::SectionChange(0));
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('i') | KeyCode::Char('I') => {
                ctx.link().send(Msg::SectionChange(1));
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                ctx.link().send(Msg::SectionChange(2));
                return KeyUpdate::handled(Update::full());
            }
            _ => {}
        }

        match ctx.state.section {
            0 => match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    ctx.link().send(Msg::ActiveToggleSpinner);
                    KeyUpdate::handled(Update::full())
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    ctx.link().send(Msg::ActiveReset);
                    KeyUpdate::handled(Update::full())
                }
                _ => KeyUpdate::unhandled(Update::none()),
            },
            1 => {
                if matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R')) {
                    ctx.link().send(Msg::IdleReset);
                    return KeyUpdate::handled(Update::full());
                }
                KeyUpdate::unhandled(Update::none())
            }
            _ => match key.code {
                KeyCode::Char('1') => {
                    ctx.link().send(Msg::TabbedTabChange(0));
                    KeyUpdate::handled(Update::full())
                }
                KeyCode::Char('2') => {
                    ctx.link().send(Msg::TabbedTabChange(1));
                    KeyUpdate::handled(Update::full())
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    ctx.link().send(Msg::TabbedReset);
                    KeyUpdate::handled(Update::full())
                }
                _ => KeyUpdate::unhandled(Update::none()),
            },
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        match ctx.state.section {
            0 => self.view_active_section(ctx),
            1 => self.view_idle_section(ctx),
            _ => self.view_tabbed_section(ctx),
        }
    }
}

impl Diagnostics {
    fn section_tabs(&self, ctx: &Context<Self>) -> Element {
        rsx! {
            Tabs {
                tabs: vec!["Active".into(), "Idle".into(), "Tabbed".into()],
                active: ctx.state.section,
                on_change: ctx.link().callback(|e: TabsEvent| Msg::SectionChange(e.index)),
            }
        }
    }

    fn view_active_section(&self, ctx: &Context<Self>) -> Element {
        let count = ACTIVE_VIEW_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let (renders_in_window, render_rate) = ACTIVE_RENDER_WINDOW.with(|w| {
            let mut w = w.borrow_mut();
            w.record();
            (w.count(), w.rate())
        });
        let mouse_events = tui_lipan::debug::mouse_events_processed();

        let status = format!(
            "Renders: {} (last 2s: {} @ {:.1}/s) | Mouse Events: {} | Spinner: {}",
            count,
            renders_in_window,
            render_rate,
            mouse_events,
            if ctx.state.active_show_spinner {
                "ON"
            } else {
                "OFF"
            }
        );

        rsx! {
            VStack {
                padding: 2,
                gap: 1,
                Text { content: "Diagnostics - Active section" },
                self.section_tabs(ctx),
                Text { content: status },
                Text { content: "Press 's' to toggle spinner, 'r' to reset" },
                Text { content: "" },
                HStack {
                    gap: 2,
                    VStack {
                        width: Length::Flex(1),
                        gap: 1,
                        Text { content: "Editable Input (focus for cursor blink):" },
                        Input {
                            value: ctx.state.active_input_value.clone(),
                            on_change: ctx.link().callback(|e: InputEvent| Msg::ActiveInputChange(e.value.to_string())),
                            border: true,
                        },
                        Text { content: "" },
                        Text { content: "Editable TextArea:" },
                        TextArea {
                            value: ctx.state.active_textarea_value.clone(),
                            on_change: ctx.link()
                                .callback(|e: TextAreaEvent| Msg::ActiveTextAreaChange(e.value.to_string())),
                            height: Length::Px(5),
                            border: true,
                        },
                        Text { content: "" },
                        Text { content: "Slider:" },
                        Slider {
                            value: ctx.state.active_slider_value,
                            on_change: ctx.link().callback(|v: f64| Msg::ActiveSliderChange(v)),
                        },
                    },
                    VStack {
                        width: Length::Flex(1),
                        gap: 1,
                        Text { content: "List with hover styles:" },
                        List {
                            items: vec![
                                "Hover over me!".into(),
                                "I have hover styles".into(),
                                "Mouse move triggers render".into(),
                                "Only when hover changes".into(),
                            ],
                            selected: ctx.state.active_selected,
                            on_select: ctx.link().callback(|e: ListEvent| Msg::ActiveSelect(e.index)),
                            item_hover_style: Style::new().bg(Color::DarkGray),
                            height: Length::Px(6),
                            border: true,
                        },
                        Text { content: "" },
                        if ctx.state.active_show_spinner {
                            HStack {
                                gap: 1,
                                Text { content: "Spinner (50ms tick):" },
                                Spinner {},
                            },
                        } else {
                            Text { content: "Spinner: OFF (press 's' to show)" },
                        },
                    },
                },
                Text { content: "" },
                Text {
                    content: "Expected rates:",
                    style: Style::new().fg(Color::Yellow),
                },
                Text { content: "- Idle (nothing focused, no spinner): 0/s" },
                Text { content: "- Input/TextArea focused: ~2/s (cursor blink)" },
                Text { content: "- Spinner visible: ~20/s" },
                Text { content: "- Mouse moving over List: renders on item change" },
            }
        }
    }

    fn view_idle_section(&self, ctx: &Context<Self>) -> Element {
        let count = IDLE_VIEW_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let (renders_in_window, render_rate) = IDLE_RENDER_WINDOW.with(|w| {
            let mut w = w.borrow_mut();
            w.record();
            (w.count(), w.rate())
        });
        let mouse_events = tui_lipan::debug::mouse_events_processed();

        rsx! {
            VStack {
                padding: 2,
                gap: 1,
                Text { content: "Diagnostics - Idle section" },
                self.section_tabs(ctx),
                Text {
                    content: format!(
                        "Renders: {} (last 2s: {} @ {:.1}/s) | Mouse Events: {}", count,
                        renders_in_window, render_rate, mouse_events
                    ),
                },
                Text { content: format!("Last Action: {}", ctx.state.idle_last_action) },
                Text { content: "" },
                Text { content: "Instructions:" },
                Text { content: "- Wait 2 seconds: rate should drop to 0" },
                Text { content: "- Move mouse on empty space: mouse events up, renders stay 0" },
                Text { content: "- Click list item: count +1" },
                Text { content: "- Press 'r' to reset counter" },
                Text { content: "" },
                TextArea {
                    value: "This is read-only. No cursor blink here.",
                    read_only: true,
                    height: Length::Px(3),
                    border: true,
                },
                List {
                    items: vec!["Click me (item 0)".into(), "Click me (item 1)".into(), "Click me (item 2)".into()],
                    selected: ctx.state.idle_selected,
                    on_select: ctx.link().callback(|e: ListEvent| Msg::IdleSelect(e.index)),
                    height: Length::Px(5),
                    border: true,
                },
                Text { content: "" },
                Text {
                    content: "(Empty space below - mouse here should NOT trigger renders)",
                    style: Style::new().fg(Color::DarkGray),
                },
            }
        }
    }

    fn view_tabbed_section(&self, ctx: &Context<Self>) -> Element {
        let count = TABBED_VIEW_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let (renders_in_window, render_rate) = TABBED_RENDER_WINDOW.with(|w| {
            let mut w = w.borrow_mut();
            w.record();
            (w.count(), w.rate())
        });
        let mouse_events = tui_lipan::debug::mouse_events_processed();
        let status = format!(
            "Renders: {} (last 2s: {} @ {:.1}/s) | Mouse Events: {}",
            count, renders_in_window, render_rate, mouse_events
        );

        rsx! {
            VStack {
                padding: 2,
                gap: 1,
                Text { content: "Diagnostics - Tabbed section" },
                self.section_tabs(ctx),
                Text { content: status },
                Text { content: "Press '1' for idle tab, '2' for hover tab, 'r' to reset" },
                Text { content: "" },
                Tabs {
                    tabs: vec!["Tab 1 (Idle)".into(), "Tab 2 (Hoverables)".into()],
                    active: ctx.state.tabbed_active_tab,
                    on_change: ctx.link().callback(|e: TabsEvent| Msg::TabbedTabChange(e.index)),
                },
                if ctx.state.tabbed_active_tab == 0 {
                    VStack {
                        gap: 1,
                        padding: 1,
                        Text {
                            content: "Tab 1: NO hover styles, NO editable widgets",
                            style: Style::new().fg(Color::Green),
                        },
                        Text { content: "" },
                        Text { content: "Expected behavior:" },
                        Text { content: "- Idle (no mouse movement): 0 renders/s" },
                        Text { content: "- Mouse over empty space: 0 renders/s" },
                        Text { content: "- Mouse over Tabs widget: 0 renders/s (fixed!)" },
                        Text { content: "" },
                        Text {
                            content: "FIX: Tabs.is_hoverable() now only returns true if",
                            style: Style::new().fg(Color::Green),
                        },
                        Text {
                            content: "on_click or hover styles are set, NOT just on_change",
                            style: Style::new().fg(Color::Green),
                        },
                        TextArea {
                            value: "Read-only. No cursor blink. No hover.",
                            read_only: true,
                            height: Length::Px(3),
                            border: true,
                        },
                    },
                } else {
                    VStack {
                        gap: 1,
                        padding: 1,
                        Text {
                            content: "Tab 2: List with item_hover_style",
                            style: Style::new().fg(Color::Yellow),
                        },
                        Text { content: "" },
                        Text { content: "Expected behavior:" },
                        Text { content: "- Mouse over list items: render on item change" },
                        Text { content: "- Mouse over same item: no render" },
                        List {
                            items: vec!["Hover item 0".into(), "Hover item 1".into(), "Hover item 2".into()],
                            selected: ctx.state.tabbed_selected,
                            on_select: ctx.link().callback(|e: ListEvent| Msg::TabbedSelect(e.index)),
                            item_hover_style: Style::new().bg(Color::DarkGray),
                            height: Length::Px(5),
                            border: true,
                        },
                    },
                },
                Text { content: "" },
                Text {
                    content: "Mouse Events: All mouse events received by framework",
                    style: Style::new().fg(Color::Cyan),
                },
                Text {
                    content: "Renders: Only when visual state changes (hover, blink, etc)",
                    style: Style::new().fg(Color::Cyan),
                },
            }
        }
    }
}

fn main() -> Result<()> {
    App::new().title("Diagnostics").mount(Diagnostics).run()
}
