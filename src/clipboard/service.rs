use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::clipboard::error::ClipboardError;
#[cfg(feature = "terminal")]
use crate::clipboard::provider::ClipboardPasteContent;
use crate::clipboard::provider::{ClipboardProvider, ImageContent};
use crate::style::Style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Determines what Shift+Insert should paste.
pub enum PasteShiftInsertBehavior {
    /// Paste from the system clipboard.
    Clipboard,
    /// Paste from the primary selection (X11), if supported.
    PrimarySelection,
}

#[derive(Debug, Clone)]
/// Clipboard configuration for the runtime.
pub struct ClipboardConfig {
    /// When true, Ctrl+C copy is only consumed if selection exists.
    pub enable_performable_ctrl_c_copy: bool,
    /// Enable primary selection if supported.
    pub enable_primary_selection: bool,
    /// Configure Shift+Insert paste behavior.
    pub paste_shift_insert_behavior: PasteShiftInsertBehavior,
    /// Maximum number of bytes to paste at once (0 disables clamping).
    pub paste_max_bytes: usize,
    /// Emit OSC52 escape sequence on copy/cut.
    pub enable_osc52: bool,
    /// Maximum number of bytes for image paste (0 disables clamping).
    pub paste_max_image_bytes: usize,
    /// Duration in milliseconds for the selection copy flash (0 disables).
    pub copy_feedback_duration_ms: u16,
    /// Style merged onto the active text selection during the copy flash.
    pub copy_feedback_style: Style,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        let enable_primary_selection = cfg!(target_os = "linux")
            && (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok());
        let paste_shift_insert_behavior = if enable_primary_selection {
            PasteShiftInsertBehavior::PrimarySelection
        } else {
            PasteShiftInsertBehavior::Clipboard
        };

        Self {
            enable_performable_ctrl_c_copy: true,
            enable_primary_selection,
            paste_shift_insert_behavior,
            paste_max_bytes: 1_000_000,
            enable_osc52: true,
            paste_max_image_bytes: 10_000_000,
            copy_feedback_duration_ms: 150,
            copy_feedback_style: Style::new().lighten_by(0.35),
        }
    }
}

pub type ClipboardReporter = Rc<dyn Fn(ClipboardError) + 'static>;

pub struct ClipboardService {
    provider: RefCell<Box<dyn ClipboardProvider>>,
    reporter: RefCell<ClipboardReporter>,
}

impl ClipboardService {
    pub fn new(provider: Box<dyn ClipboardProvider>, reporter: ClipboardReporter) -> Self {
        Self {
            provider: RefCell::new(provider),
            reporter: RefCell::new(reporter),
        }
    }

    pub fn read_clipboard_text(&self) -> Result<String, ClipboardError> {
        self.provider.borrow_mut().read_clipboard_text()
    }

    pub fn write_clipboard_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.provider.borrow_mut().write_clipboard_text(text)
    }

    #[cfg(feature = "terminal")]
    pub(crate) fn read_terminal_paste(&self) -> Result<ClipboardPasteContent, ClipboardError> {
        self.provider.borrow_mut().read_terminal_paste()
    }

    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    pub(crate) fn set_clipboard_text_cache(&self, text: String) {
        self.provider.borrow_mut().set_clipboard_text_cache(text);
    }

    pub fn read_primary_selection_text(&self) -> Result<String, ClipboardError> {
        self.provider.borrow_mut().read_primary_selection_text()
    }

    pub fn write_primary_selection_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.provider
            .borrow_mut()
            .write_primary_selection_text(text)
    }

    pub fn supports_primary_selection(&self) -> bool {
        self.provider.borrow().supports_primary_selection()
    }

    pub fn read_clipboard_image(&self) -> Result<ImageContent, ClipboardError> {
        self.provider.borrow_mut().read_clipboard_image()
    }

    pub(crate) fn write_clipboard_image(
        &self,
        content: &ImageContent,
    ) -> Result<(), ClipboardError> {
        self.provider.borrow_mut().write_clipboard_image(content)
    }

    pub fn read_clipboard_files(&self) -> Result<Vec<PathBuf>, ClipboardError> {
        self.provider.borrow_mut().read_clipboard_files()
    }

    pub fn write_clipboard_files(&self, paths: &[PathBuf]) -> Result<(), ClipboardError> {
        self.provider.borrow_mut().write_clipboard_files(paths)
    }

    pub fn supports_file_clipboard(&self) -> bool {
        self.provider.borrow().supports_file_clipboard()
    }

    pub fn report_error(&self, error: ClipboardError) {
        let reporter = self.reporter.borrow().clone();
        reporter(error);
    }

    pub(crate) fn replace_provider(&self, provider: Box<dyn ClipboardProvider>) {
        *self.provider.borrow_mut() = provider;
    }

    pub(crate) fn replace_reporter(&self, reporter: ClipboardReporter) {
        *self.reporter.borrow_mut() = reporter;
    }
}

pub fn default_clipboard_reporter() -> ClipboardReporter {
    Rc::new(|error| {
        let op = error.operation();
        let message = match error {
            ClipboardError::Unsupported { .. } => "unsupported clipboard operation".to_string(),
            ClipboardError::Provider { message, .. } => message.to_string(),
            ClipboardError::InvalidInput { message, .. } => message.to_string(),
        };
        crate::debug::internal_log!("[tui-lipan] clipboard {:?}: {}", op, message);
    })
}
