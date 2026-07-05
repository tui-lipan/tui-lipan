mod commands;
pub(crate) mod error;
mod handle;
mod osc52;
mod provider;
mod service;

pub use commands::ClipboardCommand;
pub use error::ClipboardError;
pub use handle::ClipboardHandle;
pub(crate) use osc52::write_osc52;
#[cfg(not(any(feature = "clipboard", all(target_arch = "wasm32", feature = "web"))))]
pub(crate) use provider::NoOpClipboardProvider;
#[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
pub(crate) use provider::SystemClipboardProvider;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub(crate) use provider::WebClipboardProvider;
pub use provider::{ClipboardProvider, ImageContent, ImageFormat};
pub(crate) use service::default_clipboard_reporter;
pub use service::{ClipboardConfig, ClipboardReporter, ClipboardService, PasteShiftInsertBehavior};

/// Create a no-op `Rc<ClipboardService>` for use in tests.
pub(crate) fn test_clipboard() -> std::rc::Rc<ClipboardService> {
    struct Noop;
    impl ClipboardProvider for Noop {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            Ok(String::new())
        }
        fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
            Ok(())
        }
    }
    std::rc::Rc::new(ClipboardService::new(
        Box::new(Noop),
        default_clipboard_reporter(),
    ))
}
