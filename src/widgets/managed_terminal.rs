//! Managed terminal widget with built-in PTY management.
//!
//! This composite widget wraps the low-level [`Terminal`] widget with automatic
//! PTY lifecycle management, providing a "batteries included" terminal that works
//! out of the box while still allowing low-level control when needed.
//!
//! # Example
//!
//! ```rust,ignore
//! use tui_lipan::prelude::*;
//!
//! // Simple usage - just works
//! ManagedTerminal::new()
//!     .config(TerminalPtyConfig::default().cwd("/home/user/projects"))
//!     .on_status(ctx.link().callback(|status| Msg::Status(status)))
//!
//! // With custom scrollback
//! ManagedTerminal::new()
//!     .scrollback(5000)
//!     .initial_size(120, 40)
//! ```
//!
//! For advanced use cases (custom PTY handling, multiple terminals, etc.),
//! use the low-level [`Terminal`] widget with [`TerminalPty`] directly.

use std::sync::Arc;
use std::time::Duration;

use crate::Command;
use crate::callback::{Callback, CommandLink};
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::style::Length;
use crate::widgets::terminal::{
    Terminal, TerminalInputEvent, TerminalPty, TerminalPtyConfig, TerminalPtyEvent,
    TerminalRenderSnapshot, TerminalScreen, TerminalViewport,
};
use crate::widgets::{Text, VStack};

/// Managed terminal component with built-in PTY lifecycle management.
///
/// This component handles PTY spawning, resizing, scrollback management, and
/// all the internal wiring required for a functional terminal emulator.
#[derive(Clone)]
pub struct ManagedTerminal {
    props: ManagedTerminalProps,
}

/// Properties for configuring a managed terminal.
#[derive(Clone, PartialEq)]
pub struct ManagedTerminalProps {
    /// PTY configuration (shell, cwd, env vars, etc.)
    pub config: TerminalPtyConfig,
    /// Scrollback buffer size in lines.
    /// Default: `2000`.
    pub scrollback: usize,
    /// Initial terminal size in columns.
    /// Default: `120`.
    pub initial_cols: u16,
    /// Initial terminal size in rows.
    /// Default: `24`.
    pub initial_rows: u16,
    /// Callback for status changes (connecting, ready, error, exited)
    pub on_status: Option<Callback<ManagedTerminalStatus>>,
    /// Whether to auto-start the PTY on component init.
    /// Default: `true`.
    pub auto_start: bool,
    /// Placeholder to show before PTY is ready
    pub placeholder: Option<Arc<str>>,
    /// Enable mouse forwarding to PTY.
    /// Default: `true`.
    pub forward_mouse: bool,
    /// Enable scroll wheel for scrollback.
    /// Default: `true`.
    pub scroll_wheel: bool,
    /// Delay before applying a burst of terminal viewport resizes.
    /// Default: `16ms`. Use [`std::time::Duration::ZERO`] to apply every resize immediately.
    /// Interval used to coalesce bursts of PTY resize requests; zero applies each request.
    pub resize_debounce: Duration,
    /// Style for the terminal content
    pub style: crate::style::Style,
    /// Whether the terminal should be focusable
    pub focusable: bool,
    /// Whether the terminal participates in Tab / Shift+Tab traversal.
    pub tab_stop: bool,
    /// Callback fired when the terminal gains focus.
    pub on_focus: Option<Callback<()>>,
    /// Callback fired when the terminal loses focus.
    pub on_blur: Option<Callback<()>>,
    /// Custom width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Custom height.
    /// Default: `Length::Flex(1)`.
    pub height: Length,
}

impl Default for ManagedTerminalProps {
    fn default() -> Self {
        Self {
            config: TerminalPtyConfig::default(),
            scrollback: 2000,
            initial_cols: 120,
            initial_rows: 24,
            on_status: None,
            auto_start: true,
            placeholder: Some(Arc::from("Starting terminal...")),
            forward_mouse: true,
            scroll_wheel: true,
            resize_debounce: Duration::from_millis(16),
            style: crate::style::Style::default(),
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            width: Length::Flex(1),
            height: Length::Flex(1),
        }
    }
}

/// Status events emitted by the managed terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManagedTerminalStatus {
    /// PTY is being initialized
    Starting,
    /// PTY is ready and accepting input
    Ready,
    /// Shell exited with status code
    Exited(i32),
    /// Error occurred (contains error message)
    Error(Arc<str>),
}

impl ManagedTerminal {
    /// Create a new managed terminal with default settings.
    pub fn new() -> Self {
        Self {
            props: ManagedTerminalProps::default(),
        }
    }

    /// Set the PTY configuration.
    pub fn config(mut self, config: TerminalPtyConfig) -> Self {
        self.props.config = config;
        self
    }

    /// Set the scrollback buffer size in lines.
    pub fn scrollback(mut self, lines: usize) -> Self {
        self.props.scrollback = lines;
        self
    }

    /// Set the initial terminal dimensions.
    pub fn initial_size(mut self, cols: u16, rows: u16) -> Self {
        self.props.initial_cols = cols.max(1);
        self.props.initial_rows = rows.max(1);
        self
    }

    /// Set callback for status changes.
    pub fn on_status(mut self, callback: Callback<ManagedTerminalStatus>) -> Self {
        self.props.on_status = Some(callback);
        self
    }

    /// Set whether to auto-start the PTY on init.
    /// Default: `true`.
    pub fn auto_start(mut self, auto_start: bool) -> Self {
        self.props.auto_start = auto_start;
        self
    }

    /// Set placeholder text to show before PTY is ready.
    pub fn placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.placeholder = Some(text.into());
        self
    }

    /// Set whether to forward mouse events to the PTY.
    pub fn forward_mouse(mut self, forward: bool) -> Self {
        self.props.forward_mouse = forward;
        self
    }

    /// Set whether scroll wheel controls scrollback.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.props.scroll_wheel = enabled;
        self
    }

    /// Set the window used to coalesce PTY and screen resizes.
    ///
    /// The first resize of a burst arms a single timer and the latest pending size
    /// is applied when it fires, so a continuous drag keeps reflowing at a steady
    /// cadence instead of stalling until the drag stops. A zero duration disables
    /// coalescing and applies each resize immediately.
    ///
    /// Coalescing matters beyond saving `ioctl` calls: a column change forces the
    /// screen to reflow, which drops every OSC 133 semantic mark, so an unthrottled
    /// width drag destroys shell-integration history.
    pub fn resize_debounce(mut self, delay: Duration) -> Self {
        self.props.resize_debounce = delay;
        self
    }

    /// Set the terminal content style.
    pub fn style(mut self, style: crate::style::Style) -> Self {
        self.props.style = style;
        self
    }

    /// Set whether the terminal is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.props.focusable = focusable;
        self
    }

    /// Set whether the terminal participates in Tab / Shift+Tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.props.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the terminal gains focus.
    pub fn on_focus(mut self, callback: Callback<()>) -> Self {
        self.props.on_focus = Some(callback);
        self
    }

    /// Set the callback fired when the terminal loses focus.
    pub fn on_blur(mut self, callback: Callback<()>) -> Self {
        self.props.on_blur = Some(callback);
        self
    }

    /// Set custom width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set custom height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }
}

impl Default for ManagedTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ManagedTerminal> for Element {
    fn from(terminal: ManagedTerminal) -> Self {
        let props = terminal.props.clone();
        crate::child(move || terminal.clone(), props)
    }
}

// Internal messages for the component (exposed for Component trait implementation)
#[derive(Clone)]
pub enum ManagedTerminalMsg {
    /// PTY is ready and connected
    PtyReady(TerminalPty),
    /// PTY event received (output, exited, error)
    PtyEvent(TerminalPtyEvent),
    /// Terminal input event from user
    TerminalInput(TerminalInputEvent),
    /// Mouse event bytes to forward to PTY
    TerminalMouse(Vec<u8>),
    /// Scroll to specific scrollback offset
    TerminalScrollTo(usize),
    /// Terminal resized
    Resize { cols: u16, rows: u16 },
    /// Apply the latest debounced terminal resize if this generation is current.
    FlushResize { generation: u64 },
    /// Start the PTY (manual mode only)
    Start,
}

/// Internal state for the managed terminal component.
pub struct ManagedTerminalState {
    screen: TerminalScreen,
    snapshot: TerminalRenderSnapshot,
    pty: Option<TerminalPty>,
    cols: u16,
    rows: u16,
    pending_resize: Option<(u16, u16)>,
    resize_generation: u64,
    #[cfg(test)]
    resize_apply_count: usize,
    status: ManagedTerminalStatus,
}

impl Component for ManagedTerminal {
    type Message = ManagedTerminalMsg;
    type Properties = ManagedTerminalProps;
    type State = ManagedTerminalState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        #[cfg_attr(not(feature = "terminal-images"), allow(unused_mut))]
        let mut screen =
            TerminalScreen::new(props.initial_rows, props.initial_cols, props.scrollback);
        // Size images against the host's real cell, and tell the child the same thing through the
        // PTY, so a picture the child sized for itself lands on the cells it reserved.
        #[cfg(feature = "terminal-images")]
        screen.set_cell_size(crate::host_cell_size());

        ManagedTerminalState {
            screen,
            snapshot: TerminalRenderSnapshot::default(),
            pty: None,
            cols: props.initial_cols,
            rows: props.initial_rows,
            pending_resize: None,
            resize_generation: 0,
            #[cfg(test)]
            resize_apply_count: 0,
            status: ManagedTerminalStatus::Starting,
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        // Emit initial status
        if let Some(on_status) = &ctx.props.on_status {
            on_status.emit(ManagedTerminalStatus::Starting);
        }

        if ctx.props.auto_start {
            let config = ctx.props.config.clone();
            Some(ctx.link().command(move |link| {
                Self::spawn_pty(link, &config);
            }))
        } else {
            None
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            ManagedTerminalMsg::PtyReady(pty) => {
                // Resize PTY to match our current dimensions
                let _ = pty.resize(ctx.state.cols, ctx.state.rows);
                ctx.state.pty = Some(pty);
                ctx.state.status = ManagedTerminalStatus::Ready;

                if let Some(on_status) = &ctx.props.on_status {
                    on_status.emit(ManagedTerminalStatus::Ready);
                }
                Update::full()
            }
            ManagedTerminalMsg::PtyEvent(event) => {
                match event {
                    TerminalPtyEvent::Output(bytes) => {
                        ctx.state.screen.process_bytes(&bytes);
                        // Forward any terminal responses (device queries, etc.) back to the PTY.
                        // This is critical for TUI apps like fzf that query terminal capabilities.
                        if let Some(pty) = &ctx.state.pty {
                            for response in ctx.state.screen.drain_responses() {
                                if let Err(err) = pty.write(&response) {
                                    let msg = format!("pty response write failed: {err}");
                                    ctx.state.status = ManagedTerminalStatus::Error(Arc::from(msg));
                                    break;
                                }
                            }
                        }
                        ctx.state.snapshot = ctx.state.screen.render_snapshot();
                    }
                    TerminalPtyEvent::Exited(code) => {
                        ctx.state.status = ManagedTerminalStatus::Exited(code);
                        ctx.state.pty = None;

                        if let Some(on_status) = &ctx.props.on_status {
                            on_status.emit(ManagedTerminalStatus::Exited(code));
                        }
                    }
                    TerminalPtyEvent::Error(message) => {
                        ctx.state.status = ManagedTerminalStatus::Error(message.clone());

                        if let Some(on_status) = &ctx.props.on_status {
                            on_status.emit(ManagedTerminalStatus::Error(message));
                        }
                    }
                }
                Update::full()
            }
            ManagedTerminalMsg::TerminalInput(input) => {
                if let Some(pty) = &ctx.state.pty {
                    if let Err(err) = pty.write(&input.bytes) {
                        let msg = format!("stdin write failed: {err}");
                        ctx.state.status = ManagedTerminalStatus::Error(Arc::from(msg));
                    }
                    // Snap to live view when user types
                    if ctx.state.screen.scrollback_offset() > 0 {
                        ctx.state.screen.set_scrollback(0);
                        ctx.state.snapshot = ctx.state.screen.render_snapshot();
                        return Update::full();
                    }
                }
                Update::none()
            }
            ManagedTerminalMsg::TerminalMouse(bytes) => {
                if let Some(pty) = &ctx.state.pty
                    && let Err(err) = pty.write(&bytes)
                {
                    let msg = format!("mouse write failed: {err}");
                    ctx.state.status = ManagedTerminalStatus::Error(Arc::from(msg));
                }
                Update::none()
            }
            ManagedTerminalMsg::TerminalScrollTo(offset) => {
                ctx.state.screen.set_scrollback(offset);
                ctx.state.snapshot = ctx.state.screen.render_snapshot();
                Update::full()
            }
            ManagedTerminalMsg::Resize { cols, rows } => {
                let dimensions = (cols.max(1), rows.max(1));
                if ctx.props.resize_debounce.is_zero() {
                    ctx.state.pending_resize = None;
                    ctx.state.resize_generation =
                        ctx.state.resize_generation.wrapping_add(1).max(1);
                    return Self::apply_resize(ctx, dimensions.0, dimensions.1);
                }

                let armed = ctx.state.pending_resize.is_some();
                if !armed && dimensions == (ctx.state.cols, ctx.state.rows) {
                    return Update::none();
                }

                ctx.state.pending_resize = Some(dimensions);
                // Only the first resize of a burst arms a timer. Re-arming on every
                // event would restart the window each frame of a drag, so the flush
                // would never fire until the drag paused.
                if armed {
                    return Update::none();
                }

                ctx.state.resize_generation = ctx.state.resize_generation.wrapping_add(1).max(1);
                let generation = ctx.state.resize_generation;
                Update::command_only(Command::after(
                    ctx.props.resize_debounce,
                    move |link: CommandLink<ManagedTerminalMsg>| {
                        link.send(ManagedTerminalMsg::FlushResize { generation });
                    },
                ))
            }
            ManagedTerminalMsg::FlushResize { generation } => {
                if generation != ctx.state.resize_generation {
                    return Update::none();
                }
                let Some((cols, rows)) = ctx.state.pending_resize.take() else {
                    return Update::none();
                };
                Self::apply_resize(ctx, cols, rows)
            }
            ManagedTerminalMsg::Start => {
                if ctx.state.pty.is_none() {
                    let config = ctx.props.config.clone();
                    return Update::with_command(ctx.link().command(move |link| {
                        Self::spawn_pty(link, &config);
                    }));
                }
                Update::none()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        // If no PTY is ready yet, show placeholder
        if ctx.state.pty.is_none() && ctx.props.placeholder.is_some() {
            let placeholder = ctx
                .props
                .placeholder
                .clone()
                .expect("placeholder.is_some() checked in enclosing if condition");
            return VStack::new()
                .width(ctx.props.width)
                .height(ctx.props.height)
                .child(Text::new(placeholder))
                .into();
        }

        let mut terminal = Terminal::new()
            .snapshot(ctx.state.snapshot.clone())
            .style(ctx.props.style)
            .focusable(ctx.props.focusable)
            .tab_stop(ctx.props.tab_stop)
            .width(ctx.props.width)
            .height(ctx.props.height)
            .scroll_wheel(ctx.props.scroll_wheel)
            .on_input(ctx.link().callback(ManagedTerminalMsg::TerminalInput))
            .on_resize(ctx.link().callback(|viewport: TerminalViewport| {
                ManagedTerminalMsg::Resize {
                    cols: viewport.cols,
                    rows: viewport.rows,
                }
            }))
            .on_scroll_to(ctx.link().callback(ManagedTerminalMsg::TerminalScrollTo));

        if let Some(on_focus) = ctx.props.on_focus.clone() {
            terminal = terminal.on_focus(on_focus);
        }
        if let Some(on_blur) = ctx.props.on_blur.clone() {
            terminal = terminal.on_blur(on_blur);
        }

        if ctx.props.forward_mouse {
            terminal =
                terminal.on_mouse_forward(ctx.link().callback(ManagedTerminalMsg::TerminalMouse));
        }

        terminal.into()
    }
}

impl ManagedTerminal {
    fn apply_resize(ctx: &mut Context<Self>, cols: u16, rows: u16) -> Update {
        if cols == ctx.state.cols && rows == ctx.state.rows {
            return Update::none();
        }

        ctx.state.cols = cols;
        ctx.state.rows = rows;
        #[cfg(test)]
        {
            ctx.state.resize_apply_count += 1;
        }

        // Resize PTY first so the child process learns the new dimensions.
        if let Some(pty) = &ctx.state.pty
            && let Err(err) = pty.resize(cols, rows)
        {
            let msg = format!("pty resize failed: {err}");
            ctx.state.status = ManagedTerminalStatus::Error(Arc::from(msg));
            return Update::full();
        }

        ctx.state.screen.resize(rows, cols);
        ctx.state.snapshot = ctx.state.screen.render_snapshot();
        Update::full()
    }
}

impl ManagedTerminal {
    /// Spawn the PTY and set up event handling.
    fn spawn_pty(link: CommandLink<ManagedTerminalMsg>, config: &TerminalPtyConfig) {
        #[cfg_attr(not(feature = "terminal-images"), allow(unused_mut))]
        let mut config = config.clone();
        #[cfg(feature = "terminal-images")]
        {
            config = config.cell_size(crate::host_cell_size());
        }
        let event_link = link.clone();

        match TerminalPty::spawn(config, move |event| {
            event_link.send(ManagedTerminalMsg::PtyEvent(event));
        }) {
            Ok(pty) => link.send(ManagedTerminalMsg::PtyReady(pty)),
            Err(err) => link.send(ManagedTerminalMsg::PtyEvent(TerminalPtyEvent::Error(
                err.to_string().into(),
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_terminal_props_default() {
        let props = ManagedTerminalProps::default();
        assert_eq!(props.scrollback, 2000);
        assert_eq!(props.initial_cols, 120);
        assert_eq!(props.initial_rows, 24);
        assert!(props.auto_start);
        assert!(props.forward_mouse);
        assert!(props.scroll_wheel);
        assert_eq!(props.resize_debounce, Duration::from_millis(16));
        assert!(props.focusable);
    }

    #[test]
    fn managed_terminal_builder() {
        let terminal = ManagedTerminal::new()
            .scrollback(5000)
            .initial_size(80, 30)
            .auto_start(false)
            .forward_mouse(false)
            .resize_debounce(Duration::ZERO);

        assert_eq!(terminal.props.scrollback, 5000);
        assert_eq!(terminal.props.initial_cols, 80);
        assert_eq!(terminal.props.initial_rows, 30);
        assert!(!terminal.props.auto_start);
        assert!(!terminal.props.forward_mouse);
        assert_eq!(terminal.props.resize_debounce, Duration::ZERO);
    }

    #[test]
    fn rapid_resize_burst_avoids_intermediate_mark_wipes_and_applies_once() {
        let props = ManagedTerminalProps {
            auto_start: false,
            resize_debounce: Duration::from_millis(32),
            ..ManagedTerminalProps::default()
        };
        let mut backend =
            crate::test_backend::TestBackend::new_with_props(ManagedTerminal::new(), props);
        backend
            .state_mut()
            .screen
            .process_bytes(b"\x1b]133;C\x1b\\output\r\n");
        let marks = backend.state().screen.semantic_marks();
        assert!(!marks.is_empty());

        backend
            .dispatch(ManagedTerminalMsg::Resize { cols: 10, rows: 24 })
            .unwrap();
        backend
            .dispatch(ManagedTerminalMsg::Resize {
                cols: 110,
                rows: 24,
            })
            .unwrap();

        // The burst arms exactly one timer: re-arming per event would restart the
        // window every frame of a drag, so the flush would never fire until it ended.
        let latest_generation = backend.state().resize_generation;
        assert_eq!(latest_generation, 1);
        backend
            .dispatch(ManagedTerminalMsg::FlushResize {
                generation: latest_generation.saturating_add(1),
            })
            .unwrap();

        // Neither intermediate width was applied, so the semantic marks remain anchored.
        assert_eq!(backend.state().cols, 120);
        assert_eq!(backend.state().resize_apply_count, 0);
        assert_eq!(backend.state().screen.semantic_marks(), marks);

        std::thread::sleep(Duration::from_millis(64));
        backend.pump().unwrap();

        // Only the final, different width reaches the resize path.
        assert_eq!(backend.state().cols, 110);
        assert_eq!(backend.state().resize_apply_count, 1);
        // A settled width reflow still invalidates absolute semantic indices;
        // debounce prevents every transient width from doing this repeatedly.
        assert!(backend.state().screen.semantic_marks().is_empty());
    }

    #[test]
    fn a_resize_after_a_flush_arms_a_fresh_window() {
        let props = ManagedTerminalProps {
            auto_start: false,
            resize_debounce: Duration::from_millis(16),
            ..ManagedTerminalProps::default()
        };
        let mut backend =
            crate::test_backend::TestBackend::new_with_props(ManagedTerminal::new(), props);

        backend
            .dispatch(ManagedTerminalMsg::Resize { cols: 90, rows: 24 })
            .unwrap();
        std::thread::sleep(Duration::from_millis(48));
        backend.pump().unwrap();
        assert_eq!(backend.state().cols, 90);
        assert_eq!(backend.state().resize_apply_count, 1);

        // A continuous drag keeps reflowing: the next burst is not swallowed by the
        // generation guard left behind by the previous one.
        backend
            .dispatch(ManagedTerminalMsg::Resize { cols: 70, rows: 24 })
            .unwrap();
        std::thread::sleep(Duration::from_millis(48));
        backend.pump().unwrap();
        assert_eq!(backend.state().cols, 70);
        assert_eq!(backend.state().resize_apply_count, 2);
    }
}
