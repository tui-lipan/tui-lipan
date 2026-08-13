use std::path::{Path, PathBuf};
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
        if self.config.enable_osc52 {
            write_osc52(text);
        }

        let clipboard_result = self.service.write_clipboard_text(text);

        if self.config.enable_primary_selection && self.service.supports_primary_selection() {
            // Best-effort; don't fail the overall copy if primary selection fails.
            let _ = self.service.write_primary_selection_text(text);
        }

        if self.config.enable_osc52 {
            Ok(())
        } else {
            clipboard_result
        }
    }

    /// Relay text to the outer terminal through OSC 52 without touching the native provider.
    ///
    /// Terminal hosts use this after applying their own trust/configuration policy to an OSC 52
    /// request parsed from a child. Unlike [`copy`](Self::copy), this method is unconditional.
    pub fn relay_osc52(&self, text: &str) {
        write_osc52(text);
    }

    /// Apply a clipboard-store request received from a child through OSC 52.
    ///
    /// Returns `Ok(false)` without touching any clipboard when OSC 52 is disabled in the app
    /// configuration. Enabled requests use the same native-plus-outer-terminal behavior as
    /// [`copy`](Self::copy).
    pub fn accept_osc52_store(&self, text: &str) -> Result<bool, super::error::ClipboardError> {
        if !self.config.enable_osc52 {
            return Ok(false);
        }
        self.copy(text)?;
        Ok(true)
    }

    /// Read text from the system clipboard.
    pub fn read(&self) -> Result<String, super::error::ClipboardError> {
        self.service.read_clipboard_text()
    }

    /// Copy a list of files to the system clipboard.
    ///
    /// Pasting into a file manager, a file dialog, or a browser upload target
    /// yields the actual files rather than their paths as text. This is the
    /// closest a terminal application can get to dragging files out: a TUI
    /// cannot start a native drag, because the OS drag protocols (XDND,
    /// `wl_data_device`, `NSDraggingSession`, OLE) belong to the terminal
    /// emulator's window, not to the process drawing inside it.
    ///
    /// Paths are resolved to absolute form first, so relative paths are taken
    /// against the current working directory. A path that does not exist is a
    /// [`ClipboardError::InvalidInput`](crate::ClipboardError::InvalidInput)
    /// naming that path - the underlying platform clipboards drop unresolvable
    /// entries silently, which would otherwise copy a shorter list than asked
    /// for without saying so.
    ///
    /// Unlike [`Self::copy`] this never emits OSC 52: that escape carries plain
    /// text only and cannot express a file list, so it would silently downgrade
    /// the copy to a path string. Over SSH, copy the path with [`Self::copy`]
    /// instead and accept that it lands as text.
    ///
    /// # Example
    ///
    /// ```ignore
    /// match ctx.clipboard().copy_files(&["src/main.rs"]) {
    ///     Ok(()) => ctx.toast().success("File copied - paste into a file manager"),
    ///     Err(e) => ctx.toast().error(e.to_string()),
    /// }
    /// ```
    pub fn copy_files<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Result<(), super::error::ClipboardError> {
        use super::error::{ClipboardError, ClipboardOperation};

        if paths.is_empty() {
            return Err(ClipboardError::invalid_input(
                ClipboardOperation::WriteFileClipboard,
                "no paths to copy",
            ));
        }

        let resolved = paths
            .iter()
            .map(|path| {
                let path = path.as_ref();
                path.canonicalize().map_err(|err| {
                    ClipboardError::invalid_input(
                        ClipboardOperation::WriteFileClipboard,
                        format!("{}: {}", path.display(), err),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.service.write_clipboard_files(&resolved)
    }

    /// Read a file list from the system clipboard.
    ///
    /// An empty vector means the clipboard holds no file list; a provider that
    /// cannot read file lists at all returns
    /// [`ClipboardError::Unsupported`](crate::ClipboardError::Unsupported).
    pub fn read_files(&self) -> Result<Vec<PathBuf>, super::error::ClipboardError> {
        self.service.read_clipboard_files()
    }

    /// Returns true when this provider can exchange file lists.
    ///
    /// Useful for hiding a "copy file" affordance on the web backend or in
    /// builds without the `clipboard` feature, rather than surfacing an error
    /// after the user asks for it.
    pub fn supports_files(&self) -> bool {
        self.service.supports_file_clipboard()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::clipboard::error::{ClipboardError, ClipboardOperation};
    use crate::clipboard::provider::ClipboardProvider;
    use crate::clipboard::service::default_clipboard_reporter;

    #[derive(Default)]
    struct Recorded {
        files_written: Vec<Vec<PathBuf>>,
        texts_written: Vec<String>,
        files_on_clipboard: Vec<PathBuf>,
        supports_files: bool,
    }

    struct RecordingProvider(Rc<RefCell<Recorded>>);

    impl ClipboardProvider for RecordingProvider {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            Ok(String::new())
        }

        fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.0.borrow_mut().texts_written.push(text.to_string());
            Ok(())
        }

        fn read_clipboard_files(&mut self) -> Result<Vec<PathBuf>, ClipboardError> {
            Ok(self.0.borrow().files_on_clipboard.clone())
        }

        fn write_clipboard_files(&mut self, paths: &[PathBuf]) -> Result<(), ClipboardError> {
            self.0.borrow_mut().files_written.push(paths.to_vec());
            Ok(())
        }

        fn supports_file_clipboard(&self) -> bool {
            self.0.borrow().supports_files
        }
    }

    fn handle_with(recorded: Rc<RefCell<Recorded>>) -> ClipboardHandle {
        let service = ClipboardService::new(
            Box::new(RecordingProvider(Rc::clone(&recorded))),
            default_clipboard_reporter(),
        );
        ClipboardHandle::new(Rc::new(service), ClipboardConfig::default())
    }

    /// A provider that only implements the two required methods must report
    /// file operations as unsupported rather than failing to compile.
    struct MinimalProvider;

    impl ClipboardProvider for MinimalProvider {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            Ok(String::new())
        }

        fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
            Ok(())
        }
    }

    struct FailingProvider;

    impl ClipboardProvider for FailingProvider {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            Ok(String::new())
        }

        fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
            Err(ClipboardError::provider(
                ClipboardOperation::WriteClipboard,
                "unavailable",
            ))
        }
    }

    #[test]
    fn osc52_copy_succeeds_when_the_native_provider_is_unavailable() {
        let service =
            ClipboardService::new(Box::new(FailingProvider), default_clipboard_reporter());
        let handle = ClipboardHandle::new(Rc::new(service), ClipboardConfig::default());

        assert!(handle.copy("remote text").is_ok());
    }

    #[test]
    fn native_copy_failure_is_reported_when_osc52_is_disabled() {
        let service =
            ClipboardService::new(Box::new(FailingProvider), default_clipboard_reporter());
        let handle = ClipboardHandle::new(
            Rc::new(service),
            ClipboardConfig {
                enable_osc52: false,
                ..ClipboardConfig::default()
            },
        );

        assert!(handle.copy("local text").is_err());
    }

    #[test]
    fn disabled_osc52_store_does_not_touch_the_native_provider() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let service = ClipboardService::new(
            Box::new(RecordingProvider(Rc::clone(&recorded))),
            default_clipboard_reporter(),
        );
        let handle = ClipboardHandle::new(
            Rc::new(service),
            ClipboardConfig {
                enable_osc52: false,
                ..ClipboardConfig::default()
            },
        );

        assert_eq!(handle.accept_osc52_store("blocked").unwrap(), false);
        assert!(recorded.borrow().texts_written.is_empty());
    }

    #[test]
    fn enabled_osc52_store_updates_the_native_provider() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));

        assert_eq!(handle.accept_osc52_store("accepted").unwrap(), true);
        assert_eq!(recorded.borrow().texts_written, ["accepted"]);
    }

    #[test]
    fn copy_files_rejects_empty_input_without_touching_provider() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));

        let err = handle.copy_files::<&str>(&[]).unwrap_err();

        assert!(matches!(
            err,
            ClipboardError::InvalidInput {
                operation: ClipboardOperation::WriteFileClipboard,
                ..
            }
        ));
        assert!(recorded.borrow().files_written.is_empty());
    }

    /// The platform clipboards silently drop paths they cannot resolve, so a
    /// partially valid list would copy fewer files than requested with no
    /// error. The handle must reject the whole call instead.
    #[test]
    fn copy_files_rejects_missing_path_without_partial_write() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));

        let err = handle
            .copy_files(&["Cargo.toml", "definitely/not/here.txt"])
            .unwrap_err();

        match err {
            ClipboardError::InvalidInput { message, .. } => {
                assert!(
                    message.contains("definitely/not/here.txt"),
                    "error should name the offending path, got: {message}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
        assert!(
            recorded.borrow().files_written.is_empty(),
            "nothing should reach the clipboard when one path is bad"
        );
    }

    #[test]
    fn copy_files_resolves_relative_paths_to_absolute() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));

        handle.copy_files(&["Cargo.toml"]).unwrap();

        let written = &recorded.borrow().files_written;
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].len(), 1);
        assert!(written[0][0].is_absolute());
        assert!(written[0][0].ends_with("Cargo.toml"));
    }

    /// OSC 52 carries plain text only; emitting it here would silently
    /// downgrade a file copy to a path string.
    #[test]
    fn copy_files_does_not_also_write_text() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));

        handle.copy_files(&["Cargo.toml"]).unwrap();

        assert!(recorded.borrow().texts_written.is_empty());
    }

    #[test]
    fn read_files_passes_provider_result_through() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        recorded.borrow_mut().files_on_clipboard = vec![PathBuf::from("/tmp/a"), "/tmp/b".into()];
        let handle = handle_with(Rc::clone(&recorded));

        assert_eq!(
            handle.read_files().unwrap(),
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
    }

    #[test]
    fn supports_files_reflects_provider_capability() {
        let recorded = Rc::new(RefCell::new(Recorded::default()));
        let handle = handle_with(Rc::clone(&recorded));
        assert!(!handle.supports_files());

        recorded.borrow_mut().supports_files = true;
        assert!(handle.supports_files());
    }

    #[test]
    fn provider_defaults_report_file_clipboard_unsupported() {
        let mut provider = MinimalProvider;

        assert!(!provider.supports_file_clipboard());
        assert!(matches!(
            provider.read_clipboard_files(),
            Err(ClipboardError::Unsupported {
                operation: ClipboardOperation::ReadFileClipboard
            })
        ));
        assert!(matches!(
            provider.write_clipboard_files(&[]),
            Err(ClipboardError::Unsupported {
                operation: ClipboardOperation::WriteFileClipboard
            })
        ));
    }
}
