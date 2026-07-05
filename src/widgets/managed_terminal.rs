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
    /// Style for the terminal content
    pub style: crate::style::Style,
    /// Whether the terminal should be focusable
    pub focusable: bool,
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
            style: crate::style::Style::default(),
            focusable: true,
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
    status: ManagedTerminalStatus,
}

impl Component for ManagedTerminal {
    type Message = ManagedTerminalMsg;
    type Properties = ManagedTerminalProps;
    type State = ManagedTerminalState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        ManagedTerminalState {
            screen: TerminalScreen::new(props.initial_rows, props.initial_cols, props.scrollback),
            snapshot: TerminalRenderSnapshot::default(),
            pty: None,
            cols: props.initial_cols,
            rows: props.initial_rows,
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
                if cols == ctx.state.cols && rows == ctx.state.rows {
                    return Update::none();
                }

                ctx.state.cols = cols;
                ctx.state.rows = rows;

                // Resize PTY first so the child process learns the new dimensions
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

        if ctx.props.forward_mouse {
            terminal =
                terminal.on_mouse_forward(ctx.link().callback(ManagedTerminalMsg::TerminalMouse));
        }

        terminal.into()
    }
}

impl ManagedTerminal {
    /// Spawn the PTY and set up event handling.
    fn spawn_pty(link: CommandLink<ManagedTerminalMsg>, config: &TerminalPtyConfig) {
        let config = config.clone();
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
        assert!(props.focusable);
    }

    #[test]
    fn managed_terminal_builder() {
        let terminal = ManagedTerminal::new()
            .scrollback(5000)
            .initial_size(80, 30)
            .auto_start(false)
            .forward_mouse(false);

        assert_eq!(terminal.props.scrollback, 5000);
        assert_eq!(terminal.props.initial_cols, 80);
        assert_eq!(terminal.props.initial_rows, 30);
        assert!(!terminal.props.auto_start);
        assert!(!terminal.props.forward_mouse);
    }
}
