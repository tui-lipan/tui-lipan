use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use web_time::Instant;

#[cfg(not(unix))]
use ratatui_image::picker::Capability;
use ratatui_image::picker::Picker;
use ratatui_image::picker::cap_parser::QueryStdioOptions;

#[cfg(test)]
thread_local! {
    static TEST_HOST: std::cell::RefCell<Option<(Picker, bool)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, feature = "terminal-images"))]
pub(crate) fn test_kitty_host(cell: (u16, u16), measured: bool) -> impl Drop {
    struct Scope(Option<(Picker, bool)>);
    impl Drop for Scope {
        fn drop(&mut self) {
            TEST_HOST.with(|host| *host.borrow_mut() = self.0.take());
        }
    }
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(cell.into());
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    Scope(TEST_HOST.with(|host| host.replace(Some((picker, measured)))))
}

/// Whether output reaches the terminal through tmux, which does not forward Kitty placements.
#[cfg(feature = "terminal-images")]
pub(crate) fn host_is_behind_tmux() -> bool {
    #[cfg(test)]
    if TEST_HOST.with(|host| host.borrow().is_some()) {
        return false;
    }
    std::env::var_os("TMUX").is_some()
}

fn picker_state() -> &'static RwLock<Picker> {
    static PICKER: OnceLock<RwLock<Picker>> = OnceLock::new();
    PICKER.get_or_init(|| RwLock::new(Picker::halfblocks()))
}

fn render_suspend_until() -> &'static RwLock<Option<Instant>> {
    static SUSPEND_UNTIL: OnceLock<RwLock<Option<Instant>>> = OnceLock::new();
    SUSPEND_UNTIL.get_or_init(|| RwLock::new(None))
}

/// Whether [`init_image_picker`] got its font size from the host rather than guessing.
static CELL_SIZE_QUERIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the picker's font size is the host's answer to the cell-size query.
///
/// A query that goes unanswered is not an error: the picker reports `Ok` carrying either an
/// arbitrary built-in size or the truncated result of dividing a padded window, both of which are
/// plausible numbers rather than true ones. Only the query's own answer says the size is real,
/// which is what a pixel coordinate has to be divided by.
#[cfg(not(unix))]
fn cell_size_was_queried(picker: &Picker) -> bool {
    picker
        .capabilities()
        .iter()
        .any(|capability| matches!(capability, Capability::CellSize(Some(_))))
}

/// Environment override for the image protocol, for a host whose answer cannot be trusted.
///
/// Detection asks the host and believes the answer, which is right until the answer is wrong: a
/// terminal behind a relay that swallows the reply, or one that draws pictures without admitting it,
/// both end up rendering half-blocks. Naming a protocol here settles it. The cell size is still
/// whatever detection found, since that question is answered separately.
const PROTOCOL_ENV: &str = "TUI_LIPAN_IMAGE_PROTOCOL";

fn forced_protocol() -> Option<ratatui_image::picker::ProtocolType> {
    use ratatui_image::picker::ProtocolType;

    let value = std::env::var(PROTOCOL_ENV).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "kitty" => Some(ProtocolType::Kitty),
        "sixel" => Some(ProtocolType::Sixel),
        "iterm2" => Some(ProtocolType::Iterm2),
        "halfblocks" => Some(ProtocolType::Halfblocks),
        _ => None,
    }
}

pub(crate) fn init_image_picker() {
    let (mut picker, cell_size_queried) = query_picker();
    CELL_SIZE_QUERIED.store(cell_size_queried, std::sync::atomic::Ordering::Release);
    if let Some(protocol) = forced_protocol() {
        picker.set_protocol_type(protocol);
    }
    if let Ok(mut slot) = picker_state().write() {
        *slot = picker;
    }
}

/// How long the host has to answer the image query.
const QUERY_BUDGET: Duration = Duration::from_millis(200);

/// Ask the host what it can draw, and how big its cells are.
///
/// `Picker::from_query_stdio` asks the same question on a thread that reads stdin until the host
/// answers, and its timeout only stops waiting for that thread, not the thread itself. On a host
/// slower than the timeout the thread goes on reading after startup: it takes the first input meant
/// for the app, and switches raw mode off under it once an answer finally arrives. So the question
/// is put through the startup exchange instead, which stops reading at its deadline and keeps any
/// input it read, and the picker is built from the answers the way ratatui-image builds it.
#[cfg(unix)]
fn query_picker() -> (Picker, bool) {
    use ratatui_image::picker::ProtocolType;
    use ratatui_image::picker::cap_parser::{Parser, Response};

    // Built first for its tmux detection, which also turns on tmux's passthrough the query needs.
    let fallback = Picker::halfblocks();
    let mut options = QueryStdioOptions {
        timeout: QUERY_BUDGET,
        ..QueryStdioOptions::default()
    };
    // As ratatui-image: WezTerm's and Konsole's answers claim protocols they draw badly.
    if ["WEZTERM_EXECUTABLE", "KONSOLE_VERSION"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
    {
        options.blacklist_protocols = vec![ProtocolType::Kitty, ProtocolType::Sixel];
    }
    let question = Parser::query(fallback.tmux_detected(), options);
    let parse = |replies: &[u8]| {
        let mut parser = Parser::new();
        replies
            .iter()
            .flat_map(|&byte| parser.push(char::from(byte)))
            .collect::<Vec<_>>()
    };
    let replies = crate::style::terminal_colors::ask_host_during_startup(
        question.as_bytes(),
        QUERY_BUDGET,
        // The query ends with a status request every terminal answers, after everything else.
        |replies| parse(replies).contains(&Response::Status),
    );
    let responses = parse(&replies);

    let mut protocol = None;
    let mut queried_cell = None;
    for response in &responses {
        match response {
            Response::Kitty => protocol = Some(ProtocolType::Kitty),
            Response::Sixel => {
                protocol.get_or_insert(ProtocolType::Sixel);
            }
            Response::CellSize(Some(cell)) => queried_cell = Some(*cell),
            _ => {}
        }
    }
    // An unanswered query still leaves the window's own size to divide, as ratatui-image does.
    let font_size = queried_cell.or_else(window_cell_size);
    let Some(font_size) = font_size else {
        return (fallback, false);
    };
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(font_size.into());
    if let Some(protocol) = protocol {
        picker.set_protocol_type(protocol);
    }
    (picker, queried_cell.is_some())
}

/// The cell size implied by dividing the window, truncated as ratatui-image does.
#[cfg(unix)]
fn window_cell_size() -> Option<(u16, u16)> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    Some((size.width / size.columns, size.height / size.rows))
}

#[cfg(not(unix))]
fn query_picker() -> (Picker, bool) {
    let options = QueryStdioOptions {
        timeout: QUERY_BUDGET,
        ..QueryStdioOptions::default()
    };
    let queried = Picker::from_query_stdio_with_options(options);
    let cell_size_queried = queried.as_ref().is_ok_and(cell_size_was_queried);
    (
        queried.unwrap_or_else(|_| Picker::halfblocks()),
        cell_size_queried,
    )
}

/// Forget the previous run's answer until this run's query replaces it.
pub(crate) fn forget_queried_cell_size() {
    CELL_SIZE_QUERIED.store(false, std::sync::atomic::Ordering::Release);
}

/// The cell size in pixels the host itself reported, or `None` when it was never asked or did not
/// answer.
///
/// Unlike [`host_cell_size`] this never falls back to the image encoder's guess, so it can be
/// divided into a pixel coordinate without putting the pointer in the wrong cell.
pub(crate) fn queried_host_cell_size() -> Option<(u16, u16)> {
    if !CELL_SIZE_QUERIED.load(std::sync::atomic::Ordering::Acquire) {
        return None;
    }
    let size = picker_snapshot().font_size();
    Some((size.width, size.height))
}

/// Exact dimensions required by unscaled native Kitty placements.
#[cfg(feature = "terminal-images")]
pub(crate) fn measured_host_cell_size() -> Option<(u16, u16)> {
    #[cfg(test)]
    if let Some(cell) = TEST_HOST.with(|host| {
        host.borrow().as_ref().map(|(picker, measured)| {
            let size = picker.font_size();
            measured.then_some((size.width, size.height))
        })
    }) {
        return cell;
    }

    #[cfg(unix)]
    if let Some(cell) = crate::app::input::pixel_mouse::cell_size() {
        return Some(cell);
    }
    queried_host_cell_size()
}

/// The host terminal's cell size in pixels.
///
/// The window size the host reports comes first: it divides into the grid exactly or not at all, it
/// follows a font zoom, and it is there even when the host ignores the size query the image encoder
/// asks. Only with neither is this the encoder's own guess, which is what a test backend and a plain
/// pipe both leave it as.
///
/// Whoever is drawing pictures needs this to be the truth rather than a plausible number. A pane
/// sized by a guess tells the program inside it that its window is a resolution the host will never
/// display, so every frame it draws has to be resampled to fit - which costs more per frame than
/// drawing it did.
#[cfg(feature = "terminal-images")]
pub(crate) fn host_cell_size() -> crate::widgets::TerminalCellSize {
    #[cfg(unix)]
    if let Some((width, height)) = crate::app::input::pixel_mouse::cell_size() {
        return crate::widgets::TerminalCellSize::new(width, height);
    }
    let size = picker_snapshot().font_size();
    crate::widgets::TerminalCellSize::new(size.width, size.height)
}

pub(crate) fn picker_snapshot() -> Picker {
    #[cfg(test)]
    if let Some(picker) =
        TEST_HOST.with(|host| host.borrow().as_ref().map(|(picker, _)| picker.clone()))
    {
        return picker;
    }

    picker_state()
        .read()
        .map(|picker| picker.clone())
        .unwrap_or_else(|_| Picker::halfblocks())
}

pub(crate) fn suspend_image_rendering_for(duration: Duration) {
    let now = Instant::now();
    let deadline = now + duration;
    if let Ok(mut slot) = render_suspend_until().write() {
        let current = *slot;
        *slot = Some(current.map(|value| value.max(deadline)).unwrap_or(deadline));
    }
}

pub(crate) fn image_rendering_suspended() -> bool {
    #[cfg(test)]
    if TEST_HOST.with(|host| host.borrow().is_some()) {
        return false;
    }

    let now = Instant::now();
    let Ok(mut slot) = render_suspend_until().write() else {
        return false;
    };

    match *slot {
        Some(deadline) if now < deadline => true,
        Some(_) => {
            *slot = None;
            false
        }
        None => false,
    }
}
