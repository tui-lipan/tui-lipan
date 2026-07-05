#[cfg(all(
    feature = "clipboard",
    feature = "clipboard-images",
    not(target_arch = "wasm32")
))]
use std::io::Cursor;

use crate::clipboard::error::{ClipboardError, ClipboardOperation};

/// Supported image formats for clipboard operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG format (lossless, larger file size).
    Png,
    /// JPEG format (lossy, smaller file size).
    Jpeg,
}

impl ImageFormat {
    /// Returns the MIME type for this image format.
    pub const fn mime_type(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

/// Image content read from or written to the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageContent {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type of the image (e.g., "image/png", "image/jpeg").
    pub mime: &'static str,
    /// Optional source filename for attachments created from local files.
    pub filename: Option<String>,
}

impl ImageContent {
    /// Creates new image content from raw bytes, encoding as base64.
    pub fn from_bytes(bytes: &[u8], format: ImageFormat) -> Self {
        use base64::{Engine as _, engine::general_purpose};
        Self {
            data: general_purpose::STANDARD.encode(bytes),
            mime: format.mime_type(),
            filename: None,
        }
    }

    /// Returns this image content with source filename metadata attached.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Decodes the base64 data back to raw bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD.decode(&self.data)
    }
}

/// Abstraction over clipboard backends.
pub trait ClipboardProvider {
    /// Read text from the system clipboard.
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError>;
    /// Write text to the system clipboard.
    fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError>;

    /// Update any provider-side cache used to satisfy sync clipboard reads.
    fn set_clipboard_text_cache(&mut self, _text: String) {}

    /// Read text from the primary selection, if supported.
    fn read_primary_selection_text(&mut self) -> Result<String, ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::ReadPrimarySelection,
        ))
    }

    /// Write text to the primary selection, if supported.
    fn write_primary_selection_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::WritePrimarySelection,
        ))
    }

    /// Returns true when primary selection is supported.
    fn supports_primary_selection(&self) -> bool {
        false
    }

    /// Read an image from the system clipboard.
    /// Returns the image as base64-encoded data.
    fn read_clipboard_image(&mut self) -> Result<ImageContent, ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::ReadImageClipboard,
        ))
    }

    /// Write an image to the system clipboard.
    /// Accepts base64-encoded image data.
    fn write_clipboard_image(&mut self, _content: &ImageContent) -> Result<(), ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::WriteImageClipboard,
        ))
    }
}

/// Clipboard provider that reports all operations as unsupported.
///
/// Used as the default when the `clipboard` feature is disabled.
#[cfg(not(feature = "clipboard"))]
pub(crate) struct NoOpClipboardProvider;

#[cfg(not(feature = "clipboard"))]
impl ClipboardProvider for NoOpClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::ReadClipboard,
        ))
    }

    fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError::unsupported(
            ClipboardOperation::WriteClipboard,
        ))
    }
}

/// Arboard-backed system clipboard provider.
#[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
pub(crate) struct SystemClipboardProvider {
    clipboard: Option<arboard::Clipboard>,
}

#[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
impl SystemClipboardProvider {
    pub fn new() -> Self {
        Self { clipboard: None }
    }

    fn ensure_clipboard(
        &mut self,
        operation: ClipboardOperation,
    ) -> Result<&mut arboard::Clipboard, ClipboardError> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|err| ClipboardError::provider(operation, err.to_string()))?,
            );
        }

        self.clipboard
            .as_mut()
            .ok_or_else(|| ClipboardError::provider(operation, "init"))
    }
}

#[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
impl Default for SystemClipboardProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
impl ClipboardProvider for SystemClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        let clipboard = self.ensure_clipboard(ClipboardOperation::ReadClipboard)?;
        clipboard.get_text().map_err(|err| {
            ClipboardError::provider(ClipboardOperation::ReadClipboard, err.to_string())
        })
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let clipboard = self.ensure_clipboard(ClipboardOperation::WriteClipboard)?;
        clipboard.set_text(text.to_string()).map_err(|err| {
            ClipboardError::provider(ClipboardOperation::WriteClipboard, err.to_string())
        })
    }

    fn read_primary_selection_text(&mut self) -> Result<String, ClipboardError> {
        #[cfg(target_os = "linux")]
        {
            use arboard::{GetExtLinux, LinuxClipboardKind};
            let clipboard = self.ensure_clipboard(ClipboardOperation::ReadPrimarySelection)?;
            clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text()
                .map_err(|err| {
                    ClipboardError::provider(
                        ClipboardOperation::ReadPrimarySelection,
                        err.to_string(),
                    )
                })
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(ClipboardError::unsupported(
                ClipboardOperation::ReadPrimarySelection,
            ))
        }
    }

    fn write_primary_selection_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        #[cfg(target_os = "linux")]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            let clipboard = self.ensure_clipboard(ClipboardOperation::WritePrimarySelection)?;
            clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(_text.to_string())
                .map_err(|err| {
                    ClipboardError::provider(
                        ClipboardOperation::WritePrimarySelection,
                        err.to_string(),
                    )
                })
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(ClipboardError::unsupported(
                ClipboardOperation::WritePrimarySelection,
            ))
        }
    }

    fn supports_primary_selection(&self) -> bool {
        cfg!(target_os = "linux")
    }

    #[cfg(feature = "clipboard-images")]
    fn read_clipboard_image(&mut self) -> Result<ImageContent, ClipboardError> {
        #[cfg(target_os = "linux")]
        if let Some(content) = read_wayland_png_clipboard()? {
            return Ok(content);
        }

        let clipboard = self.ensure_clipboard(ClipboardOperation::ReadImageClipboard)?;

        let image_data = clipboard.get_image().map_err(|err| {
            ClipboardError::provider(ClipboardOperation::ReadImageClipboard, err.to_string())
        })?;

        let width = image_data.width;
        let height = image_data.height;
        let rgba_bytes = image_data.bytes.into_owned();

        let mut png_buffer = Cursor::new(Vec::new());
        {
            let image_buffer = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(
                width as u32,
                height as u32,
                rgba_bytes,
            )
            .ok_or_else(|| {
                ClipboardError::provider(
                    ClipboardOperation::ReadImageClipboard,
                    "invalid image buffer dimensions",
                )
            })?;

            image_buffer
                .write_to(&mut png_buffer, image::ImageFormat::Png)
                .map_err(|err| {
                    ClipboardError::provider(
                        ClipboardOperation::ReadImageClipboard,
                        format!("PNG encode error: {}", err),
                    )
                })?;
        }

        Ok(ImageContent::from_bytes(
            png_buffer.into_inner().as_slice(),
            ImageFormat::Png,
        ))
    }

    #[cfg(feature = "clipboard-images")]
    fn write_clipboard_image(&mut self, content: &ImageContent) -> Result<(), ClipboardError> {
        let clipboard = self.ensure_clipboard(ClipboardOperation::WriteImageClipboard)?;

        let bytes = content.to_bytes().map_err(|err| {
            ClipboardError::provider(
                ClipboardOperation::WriteImageClipboard,
                format!("base64 decode error: {}", err),
            )
        })?;

        let img = image::load_from_memory(&bytes).map_err(|err| {
            ClipboardError::provider(
                ClipboardOperation::WriteImageClipboard,
                format!("image decode error: {}", err),
            )
        })?;

        let rgba = img.to_rgba8();
        let width = rgba.width() as usize;
        let height = rgba.height() as usize;
        let pixels: Vec<u8> = rgba.into_raw();

        clipboard
            .set_image(arboard::ImageData {
                width,
                height,
                bytes: std::borrow::Cow::Owned(pixels),
            })
            .map_err(|err| {
                ClipboardError::provider(ClipboardOperation::WriteImageClipboard, err.to_string())
            })
    }
}

#[cfg(all(
    feature = "clipboard",
    feature = "clipboard-images",
    target_os = "linux"
))]
fn read_wayland_png_clipboard() -> Result<Option<ImageContent>, ClipboardError> {
    use std::io::Read as _;

    use wl_clipboard_rs::paste::{ClipboardType, Error, MimeType, Seat, get_contents};

    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(None);
    }

    let (mut reader, _) = match get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(ImageFormat::Png.mime_type()),
    ) {
        Ok(result) => result,
        Err(
            Error::NoSeats
            | Error::ClipboardEmpty
            | Error::NoMimeType
            | Error::SocketOpenError(_)
            | Error::WaylandConnection(_)
            | Error::MissingProtocol { .. }
            | Error::PrimarySelectionUnsupported
            | Error::SeatNotFound,
        ) => return Ok(None),
        Err(err) => {
            return Err(ClipboardError::provider(
                ClipboardOperation::ReadImageClipboard,
                err.to_string(),
            ));
        }
    };

    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|err| {
        ClipboardError::provider(ClipboardOperation::ReadImageClipboard, err.to_string())
    })?;

    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(ImageContent::from_bytes(&bytes, ImageFormat::Png)))
}

#[cfg(all(target_arch = "wasm32", feature = "web"))]
#[derive(Default)]
pub(crate) struct WebClipboardProvider {
    cache: std::rc::Rc<std::cell::RefCell<Option<String>>>,
}

#[cfg(all(target_arch = "wasm32", feature = "web"))]
impl WebClipboardProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(all(target_arch = "wasm32", feature = "web"))]
impl ClipboardProvider for WebClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        if let Some(cached) = self.cache.borrow().clone() {
            return Ok(cached);
        }

        Err(ClipboardError::provider(
            ClipboardOperation::ReadClipboard,
            "web clipboard read requires a primed cache from a paste gesture",
        ))
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let window = web_sys::window().ok_or_else(|| {
            ClipboardError::provider(ClipboardOperation::WriteClipboard, "window is unavailable")
        })?;
        let navigator = window.navigator();
        let clipboard = navigator.clipboard();
        let promise = clipboard.write_text(text);

        let cache = std::rc::Rc::clone(&self.cache);
        let text = text.to_string();
        let fut = wasm_bindgen_futures::JsFuture::from(promise);
        wasm_bindgen_futures::spawn_local(async move {
            match fut.await {
                Ok(_) => {
                    *cache.borrow_mut() = Some(text);
                }
                Err(err) => {
                    web_sys::console::warn_1(&err);
                }
            }
        });
        Ok(())
    }

    fn set_clipboard_text_cache(&mut self, text: String) {
        *self.cache.borrow_mut() = Some(text);
    }

    fn supports_primary_selection(&self) -> bool {
        false
    }
}
