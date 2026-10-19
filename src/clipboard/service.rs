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

/// Clipboard target updated after completing a mouse text selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOnSelect {
    /// Do not copy completed mouse selections automatically.
    Disabled,
    /// Write only the platform primary selection.
    PrimarySelection,
    /// Write only the regular clipboard.
    Clipboard,
    /// Write both the primary selection and regular clipboard.
    Both,
}

/// Clipboard source used by a mouse paste gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteSource {
    /// Do not perform a paste.
    Disabled,
    /// Read from the platform primary selection.
    PrimarySelection,
    /// Read from the regular clipboard.
    Clipboard,
}

/// Action performed by an otherwise-unhandled right click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightClickAction {
    /// Preserve widget and application right-click behavior without a clipboard fallback.
    Disabled,
    /// Paste the regular clipboard into the focused editable widget.
    PasteClipboard,
    /// Copy an active text selection, or paste the regular clipboard when none exists.
    CopyOrPaste,
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
    /// Configure which clipboard receives a completed mouse text selection.
    pub copy_on_mouse_select: CopyOnSelect,
    /// Configure middle-click paste behavior.
    pub middle_click_paste: PasteSource,
    /// Configure fallback right-click clipboard behavior.
    pub right_click_action: RightClickAction,
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
        let copy_on_mouse_select = if enable_primary_selection {
            CopyOnSelect::PrimarySelection
        } else {
            CopyOnSelect::Disabled
        };
        let middle_click_paste = if enable_primary_selection {
            PasteSource::PrimarySelection
        } else {
            PasteSource::Disabled
        };

        Self {
            enable_performable_ctrl_c_copy: true,
            enable_primary_selection,
            paste_shift_insert_behavior,
            copy_on_mouse_select,
            middle_click_paste,
            right_click_action: RightClickAction::Disabled,
            paste_max_bytes: 1_000_000,
            enable_osc52: true,
            paste_max_image_bytes: 10_000_000,
            copy_feedback_duration_ms: 150,
            copy_feedback_style: Style::new().lighten_by(0.35),
        }
    }
}

pub(crate) fn normalize_config_for_primary_support(
    mut config: ClipboardConfig,
    supports_primary_selection: bool,
) -> ClipboardConfig {
    config.enable_primary_selection &= supports_primary_selection;
    if !config.enable_primary_selection {
        config.copy_on_mouse_select = match config.copy_on_mouse_select {
            CopyOnSelect::PrimarySelection => CopyOnSelect::Disabled,
            CopyOnSelect::Both => CopyOnSelect::Clipboard,
            target => target,
        };
        if matches!(config.middle_click_paste, PasteSource::PrimarySelection) {
            config.middle_click_paste = PasteSource::Disabled;
        }
        if matches!(
            config.paste_shift_insert_behavior,
            PasteShiftInsertBehavior::PrimarySelection
        ) {
            config.paste_shift_insert_behavior = PasteShiftInsertBehavior::Clipboard;
        }
    }

    config
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

    /// Read rich content before its text representation, honoring image size limits.
    #[cfg(feature = "terminal")]
    pub fn read_content(
        &self,
        config: &ClipboardConfig,
    ) -> Result<Option<crate::ClipboardContent>, ClipboardError> {
        let content = self.provider.borrow_mut().read_clipboard_content()?;
        if let Some(crate::ClipboardContent::Image(image)) = &content {
            validate_image_size(image, config.paste_max_image_bytes)?;
        }
        Ok(content)
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

#[cfg(feature = "terminal")]
fn validate_image_size(image: &ImageContent, limit: usize) -> Result<(), ClipboardError> {
    use crate::clipboard::error::ClipboardOperation;
    let bytes = image.to_bytes().map_err(|err| {
        ClipboardError::provider(ClipboardOperation::ReadImageClipboard, err.to_string())
    })?;
    if limit != 0 && bytes.len() > limit {
        return Err(ClipboardError::invalid_input(
            ClipboardOperation::ReadImageClipboard,
            "clipboard image exceeds paste_max_image_bytes",
        ));
    }
    Ok(())
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

#[cfg(all(test, feature = "terminal"))]
mod typed_read_tests {
    use super::*;
    use crate::clipboard::error::ClipboardOperation;
    use crate::{ClipboardContent, ClipboardHandle, ImageFormat};
    use std::cell::Cell;

    #[derive(Default)]
    struct Reads {
        files: Cell<usize>,
        images: Cell<usize>,
        text: Cell<usize>,
    }
    struct RichProvider {
        reads: Rc<Reads>,
        files: Vec<PathBuf>,
        image: Option<ImageContent>,
        fail_image: bool,
    }
    // Deliberately leave read_terminal_paste at its text-only default.
    impl ClipboardProvider for RichProvider {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            self.reads.text.set(self.reads.text.get() + 1);
            Ok("literal text fallback".into())
        }
        fn write_clipboard_text(&mut self, _: &str) -> Result<(), ClipboardError> {
            Ok(())
        }
        fn read_clipboard_files(&mut self) -> Result<Vec<PathBuf>, ClipboardError> {
            self.reads.files.set(self.reads.files.get() + 1);
            Ok(self.files.clone())
        }
        fn read_clipboard_image(&mut self) -> Result<ImageContent, ClipboardError> {
            self.reads.images.set(self.reads.images.get() + 1);
            if self.fail_image {
                return Err(ClipboardError::provider(
                    ClipboardOperation::ReadImageClipboard,
                    "image read failed",
                ));
            }
            self.image
                .clone()
                .ok_or_else(|| ClipboardError::unsupported(ClipboardOperation::ReadImageClipboard))
        }
    }
    fn handle(
        files: Vec<PathBuf>,
        image: Option<ImageContent>,
        fail_image: bool,
    ) -> (ClipboardHandle, Rc<Reads>) {
        let reads = Rc::new(Reads::default());
        let provider = RichProvider {
            reads: reads.clone(),
            files,
            image,
            fail_image,
        };
        let service = Rc::new(ClipboardService::new(
            Box::new(provider),
            default_clipboard_reporter(),
        ));
        (
            ClipboardHandle::new(service, Rc::new(RefCell::new(ClipboardConfig::default()))),
            reads,
        )
    }
    #[test]
    fn custom_provider_prefers_files_without_reading_image_or_text() {
        let files = vec![PathBuf::from("/tmp/photo.png")];
        let (handle, reads) = handle(
            files.clone(),
            Some(ImageContent::from_bytes(b"png", ImageFormat::Png)),
            false,
        );
        assert_eq!(
            handle.read_content().unwrap(),
            Some(ClipboardContent::Files(files))
        );
        assert_eq!(
            (reads.files.get(), reads.images.get(), reads.text.get()),
            (1, 0, 0)
        );
    }
    #[test]
    fn custom_provider_prefers_image_and_reads_it_only_once() {
        let image = ImageContent::from_bytes(b"png", ImageFormat::Png);
        let (handle, reads) = handle(vec![], Some(image.clone()), false);
        assert_eq!(
            handle.read_content().unwrap(),
            Some(ClipboardContent::Image(image))
        );
        assert_eq!(
            (reads.files.get(), reads.images.get(), reads.text.get()),
            (1, 1, 0)
        );
    }
    #[test]
    fn custom_provider_falls_back_to_text_only_after_optional_formats() {
        let (handle, reads) = handle(vec![], None, false);
        assert_eq!(
            handle.read_content().unwrap(),
            Some(ClipboardContent::Text("literal text fallback".into()))
        );
        assert_eq!(
            (reads.files.get(), reads.images.get(), reads.text.get()),
            (1, 1, 1)
        );
    }
    #[test]
    fn image_provider_failure_is_not_downgraded_to_text() {
        let (handle, reads) = handle(vec![], None, true);
        assert!(handle.read_content().is_err());
        assert_eq!(reads.text.get(), 0);
    }
}
