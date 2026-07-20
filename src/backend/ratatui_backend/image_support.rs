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

pub(crate) fn init_image_picker() {
    let options = QueryStdioOptions {
        timeout: Duration::from_millis(200),
        ..QueryStdioOptions::default()
    };

    let picker =
        Picker::from_query_stdio_with_options(options).unwrap_or_else(|_| Picker::halfblocks());
    if let Ok(mut slot) = picker_state().write() {
        *slot = picker;
    }
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
