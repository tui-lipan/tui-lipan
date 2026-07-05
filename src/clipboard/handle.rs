use std::rc::Rc;

use super::osc52::write_osc52;
use super::service::{ClipboardConfig, ClipboardService};

/// Handle for programmatic clipboard access from components.
///
/// Obtained via [`Context::clipboard()`](crate::core::component::Context::clipboard).
///
/// # Example
///
/// ```ignore
/// if let Err(e) = ctx.clipboard().copy("Hello, world!") {
///     ctx.toast().error("Clipboard write failed");
/// }
/// ```
#[derive(Clone)]
pub struct ClipboardHandle {
    service: Rc<ClipboardService>,
    config: ClipboardConfig,
}

impl ClipboardHandle {
    pub(crate) fn new(service: Rc<ClipboardService>, config: ClipboardConfig) -> Self {
        Self { service, config }
    }

    /// Copy text to the system clipboard.
    ///
    /// Also emits OSC 52 when enabled (for clipboard over SSH) and writes to
    /// the primary selection on supported platforms.
    pub fn copy(&self, text: &str) -> Result<(), super::error::ClipboardError> {
        self.service.write_clipboard_text(text)?;

        if self.config.enable_osc52 {
            write_osc52(text);
        }

        if self.config.enable_primary_selection && self.service.supports_primary_selection() {
            // Best-effort; don't fail the overall copy if primary selection fails.
            let _ = self.service.write_primary_selection_text(text);
        }

        Ok(())
    }

    /// Read text from the system clipboard.
    pub fn read(&self) -> Result<String, super::error::ClipboardError> {
        self.service.read_clipboard_text()
    }
}
