use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use web_time::Instant;

use ratatui_image::picker::Picker;
use ratatui_image::picker::cap_parser::QueryStdioOptions;

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
    let options = QueryStdioOptions {
        timeout: Duration::from_millis(200),
        ..QueryStdioOptions::default()
    };

    let queried = Picker::from_query_stdio_with_options(options);
    CELL_SIZE_QUERIED.store(queried.is_ok(), std::sync::atomic::Ordering::Release);
    let mut picker = queried.unwrap_or_else(|_| Picker::halfblocks());
    if let Some(protocol) = forced_protocol() {
        picker.set_protocol_type(protocol);
    }
    if let Ok(mut slot) = picker_state().write() {
        *slot = picker;
    }
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
