use crate::app::input::keymap::{Action, BindingMatch, BindingMode, Keymap};
use crate::clipboard::{
    ClipboardCommand, ClipboardConfig, ClipboardError, ClipboardService, ImageContent, write_osc52,
};
use crate::core::event::KeyEvent;
use crate::ui::capabilities::ClipboardContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ClipboardDispatchOutcome {
    pub handled: bool,
    pub copied: bool,
    pub mutated: bool,
}

impl ClipboardDispatchOutcome {
    pub(crate) fn not_handled() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImagePasteContent {
    Image(ImageContent),
    SuppressTextFallback,
    None,
}

pub(crate) fn read_image_paste_content(
    clipboard: &ClipboardService,
    config: &ClipboardConfig,
) -> ImagePasteContent {
    match clipboard.read_clipboard_image() {
        Ok(content) => {
            let byte_count = content.data.len() * 3 / 4; // approx decoded size
            if config.paste_max_image_bytes == 0 || byte_count <= config.paste_max_image_bytes {
                return ImagePasteContent::Image(content);
            }
            // Oversized image: fall through to text paste.
        }
        Err(_) => {
            // No raw image in clipboard (Unsupported or provider error, e.g. on Linux a
            // file manager copies a file:// URI as text rather than raw pixels) - fall
            // through to text path below.
        }
    }

    match clipboard.read_clipboard_text() {
        Ok(text) => {
            if let Some(content) = try_load_image_from_text(&text, config.paste_max_image_bytes) {
                return ImagePasteContent::Image(content);
            }

            if looks_like_html(&text) || looks_like_vector(&text) {
                return ImagePasteContent::SuppressTextFallback;
            }

            ImagePasteContent::None
        }
        Err(_) => ImagePasteContent::None,
    }
}

pub(crate) fn dispatch_clipboard(
    key: KeyEvent,
    keymap: &Keymap,
    context: &mut dyn ClipboardContext,
    clipboard: &ClipboardService,
    config: &ClipboardConfig,
) -> ClipboardDispatchOutcome {
    let matches = keymap.matches(key);
    let clipboard_matches: Vec<ClipboardBinding> = matches
        .into_iter()
        .filter_map(ClipboardBinding::from_match)
        .collect();

    if clipboard_matches.is_empty() {
        return ClipboardDispatchOutcome::not_handled();
    }

    if context.block_copy_cut()
        && clipboard_matches
            .iter()
            .any(|binding| binding.command.is_copy_or_cut())
    {
        return ClipboardDispatchOutcome {
            handled: true,
            copied: false,
            mutated: false,
        };
    }

    let selection_text = context.selection_text();
    let has_selection = selection_text
        .as_ref()
        .map(|text| !text.is_empty())
        .unwrap_or(false);
    let can_copy = context.can_copy() && has_selection;
    let can_cut = context.can_cut() && has_selection;
    let can_paste = context.can_paste();

    let is_performable = |command: ClipboardCommand| {
        if command.is_image() {
            // Image paste uses can_paste; image copy is always non-performable here
            // (no context API to detect image availability for copy)
            return matches!(command, ClipboardCommand::PasteImage) && can_paste;
        }
        match command {
            ClipboardCommand::Copy => can_copy,
            ClipboardCommand::Cut => can_cut,
            ClipboardCommand::Paste | ClipboardCommand::PasteFromSelection => can_paste,
            ClipboardCommand::CopyImage | ClipboardCommand::PasteImage => unreachable!(),
        }
    };

    let has_performable_binding = clipboard_matches
        .iter()
        .any(|binding| binding.mode == BindingMode::Performable);

    if has_performable_binding {
        if let Some(binding) = clipboard_matches.iter().find(|binding| {
            binding.mode == BindingMode::Performable && is_performable(binding.command)
        }) {
            let command_outcome = perform_command(
                binding.command,
                context,
                clipboard,
                config,
                selection_text.as_deref(),
            );
            return ClipboardDispatchOutcome {
                handled: true,
                copied: command_outcome.copied,
                mutated: command_outcome.mutated,
            };
        }

        return ClipboardDispatchOutcome::not_handled();
    }

    let Some(binding) = clipboard_matches.first() else {
        return ClipboardDispatchOutcome::not_handled();
    };

    let command_outcome = if is_performable(binding.command) {
        perform_command(
            binding.command,
            context,
            clipboard,
            config,
            selection_text.as_deref(),
        )
    } else {
        ClipboardCommandOutcome::default()
    };

    ClipboardDispatchOutcome {
        handled: true,
        copied: command_outcome.copied,
        mutated: command_outcome.mutated,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ClipboardCommandOutcome {
    copied: bool,
    mutated: bool,
}

pub(crate) fn dispatch_text_paste(
    text: &str,
    context: &mut dyn ClipboardContext,
    max_bytes: usize,
) -> bool {
    if !context.can_paste() {
        return false;
    }

    let text = truncate_paste(text, max_bytes);
    if context.handle_text_paste(&text) {
        return true;
    }
    context.insert_text(&text)
}

struct ClipboardBinding {
    command: ClipboardCommand,
    mode: BindingMode,
}

impl ClipboardBinding {
    fn from_match(binding: BindingMatch) -> Option<Self> {
        let command = match binding.action {
            Action::Copy => ClipboardCommand::Copy,
            Action::Cut => ClipboardCommand::Cut,
            Action::Paste => ClipboardCommand::Paste,
            Action::PasteFromSelection => ClipboardCommand::PasteFromSelection,
            Action::CopyImage => ClipboardCommand::CopyImage,
            Action::PasteImage => ClipboardCommand::PasteImage,
            _ => return None,
        };

        Some(Self {
            command,
            mode: binding.mode,
        })
    }
}

fn perform_command(
    command: ClipboardCommand,
    context: &mut dyn ClipboardContext,
    clipboard: &ClipboardService,
    config: &ClipboardConfig,
    selection_text: Option<&str>,
) -> ClipboardCommandOutcome {
    match command {
        ClipboardCommand::Copy => {
            let Some(text) = selection_text.filter(|text| !text.is_empty()) else {
                return ClipboardCommandOutcome::default();
            };
            ClipboardCommandOutcome {
                copied: write_to_clipboard(text, clipboard, config),
                mutated: false,
            }
        }
        ClipboardCommand::Cut => {
            let Some(text) = selection_text.filter(|text| !text.is_empty()) else {
                return ClipboardCommandOutcome::default();
            };
            write_to_clipboard(text, clipboard, config);
            ClipboardCommandOutcome {
                copied: false,
                mutated: context.delete_selection(),
            }
        }
        ClipboardCommand::Paste => {
            if !context.can_paste() {
                return ClipboardCommandOutcome::default();
            }

            let mut mutated = false;

            // Smart image detection: if this context can handle images and the clipboard
            // contains an image, dispatch it instead of falling through to text paste.
            if context.accepts_image() {
                match read_image_paste_content(clipboard, config) {
                    ImagePasteContent::Image(content) => {
                        return ClipboardCommandOutcome {
                            copied: false,
                            mutated: context.insert_image(&content),
                        };
                    }
                    ImagePasteContent::SuppressTextFallback => {
                        return ClipboardCommandOutcome::default();
                    }
                    ImagePasteContent::None => {}
                }
            }

            match clipboard.read_clipboard_text() {
                Ok(text) => {
                    mutated = dispatch_text_paste(&text, context, config.paste_max_bytes);
                }
                Err(err) => clipboard.report_error(err),
            }
            ClipboardCommandOutcome {
                copied: false,
                mutated,
            }
        }
        ClipboardCommand::PasteFromSelection => {
            if !context.can_paste() {
                return ClipboardCommandOutcome::default();
            }

            let mut text = None;
            if config.enable_primary_selection && clipboard.supports_primary_selection() {
                match clipboard.read_primary_selection_text() {
                    Ok(value) => text = Some(value),
                    Err(ClipboardError::Unsupported { .. }) => {}
                    Err(err) => clipboard.report_error(err),
                }
            }

            if text.is_none() {
                match clipboard.read_clipboard_text() {
                    Ok(value) => text = Some(value),
                    Err(err) => {
                        clipboard.report_error(err);
                        return ClipboardCommandOutcome::default();
                    }
                }
            }

            let mut mutated = false;
            if let Some(text) = text {
                mutated = dispatch_text_paste(&text, context, config.paste_max_bytes);
            }
            ClipboardCommandOutcome {
                copied: false,
                mutated,
            }
        }
        ClipboardCommand::CopyImage => {
            // Copy the image from the context if the widget provides one.
            if let Some(content) = context.get_image()
                && let Err(err) = clipboard.write_clipboard_image(&content)
                && !matches!(err, ClipboardError::Unsupported { .. })
            {
                clipboard.report_error(err);
            }
            ClipboardCommandOutcome::default()
        }
        ClipboardCommand::PasteImage => {
            if !context.can_paste() {
                return ClipboardCommandOutcome::default();
            }

            let mut mutated = false;
            match clipboard.read_clipboard_image() {
                Ok(content) => {
                    // Enforce max image size if configured
                    let byte_count = content.data.len() * 3 / 4; // approx decoded size
                    if config.paste_max_image_bytes > 0 && byte_count > config.paste_max_image_bytes
                    {
                        return ClipboardCommandOutcome::default();
                    }
                    mutated = context.insert_image(&content);
                }
                Err(ClipboardError::Unsupported { .. }) => {}
                Err(err) => clipboard.report_error(err),
            }
            ClipboardCommandOutcome {
                copied: false,
                mutated,
            }
        }
    }
}

/// Writes `text` to the system clipboard, optionally emits OSC52, and writes to the primary
/// selection when configured. Shared by both Copy and Cut commands.
pub(crate) fn write_to_clipboard(
    text: &str,
    clipboard: &ClipboardService,
    config: &ClipboardConfig,
) -> bool {
    let mut wrote = false;

    match clipboard.write_clipboard_text(text) {
        Ok(()) => wrote = true,
        Err(err) => clipboard.report_error(err),
    }

    if config.enable_osc52 {
        write_osc52(text);
        wrote = true;
    }

    if config.enable_primary_selection && clipboard.supports_primary_selection() {
        match clipboard.write_primary_selection_text(text) {
            Ok(()) => wrote = true,
            Err(ClipboardError::Unsupported { .. }) => {}
            Err(err) => clipboard.report_error(err),
        }
    }

    wrote
}

/// Returns true when `text` looks like HTML clipboard content (e.g. the `text/html` MIME type
/// that browsers place on the clipboard alongside `image/png` when copying an image).
///
/// Clipboard managers often discard the raw binary `image/png` target but keep the `text/html`
/// representation. Without this check that HTML would be inserted verbatim as text.
fn looks_like_html(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<!DOCTYPE")
        || trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<HTML")
        || trimmed.starts_with("<meta ")
        || trimmed.starts_with("<META ")
}

/// Returns true when `text` looks like SVG or XML vector content (e.g. the `image/svg+xml` or
/// `text/plain` MIME targets that design tools place on the clipboard when copying vector shapes).
///
/// Clipboard managers often preserve these text representations even when the raw raster image
/// is discarded. Inserting raw SVG/XML markup into an image-accepting widget is never useful,
/// so callers suppress this silently.
fn looks_like_vector(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<svg")
        || trimmed.starts_with("<SVG")
        || trimmed.starts_with("<?xml")
        || trimmed.starts_with("<?XML")
}
/// path pointing to a supported image file (PNG, JPEG, GIF, WebP).
///
/// Returns `None` when the text is not a recognisable image path or when the file cannot be read
/// or decoded. The caller falls back to plain text insertion in that case.
fn try_load_image_from_text(
    text: &str,
    max_image_bytes: usize,
) -> Option<crate::clipboard::ImageContent> {
    use crate::clipboard::{ImageContent, ImageFormat};

    // Strip a single `file://` or `file:///` prefix (the latter is the URI spec form on Linux).
    // Also handle a trailing newline that some tools append.
    let text = text.trim();
    let path_str = if let Some(rest) = text.strip_prefix("file://") {
        // `file:///home/…` → strip the third `/` so we get an absolute path
        if rest.starts_with('/') {
            rest
        } else {
            // `file://hostname/…` - not a local file, ignore
            return None;
        }
    } else if text.starts_with('/') {
        text
    } else {
        return None;
    };

    // Percent-decode the path (spaces become %20, etc.)
    let decoded_path: std::borrow::Cow<str> = percent_decode_path(path_str);
    let path = std::path::Path::new(decoded_path.as_ref());

    // Only accept recognised image extensions to avoid loading arbitrary files.
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        // GIF / WebP: load via `image` crate, re-encode as PNG for a uniform representation.
        #[cfg(feature = "image")]
        "gif" | "webp" => ImageFormat::Png,
        _ => return None,
    };

    let bytes = std::fs::read(path).ok()?;

    if max_image_bytes > 0 && bytes.len() > max_image_bytes {
        return None;
    }

    // For non-PNG/JPEG formats (e.g. GIF), decode and re-encode as PNG via the `image` crate.
    #[cfg(feature = "image")]
    let final_bytes = match ext.as_str() {
        "gif" | "webp" => {
            let img = image::load_from_memory(&bytes).ok()?;
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
            buf.into_inner()
        }
        _ => bytes,
    };
    #[cfg(not(feature = "image"))]
    let final_bytes = bytes;

    Some(ImageContent::from_bytes(&final_bytes, format))
}

/// Decode percent-encoded characters in a file-system path (e.g. `%20` → space).
fn percent_decode_path(input: &str) -> std::borrow::Cow<'_, str> {
    if !input.contains('%') {
        return std::borrow::Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(a), Some(b)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{a}{b}"), 16) {
                    out.push(byte as char);
                    continue;
                }
                out.push('%');
                out.push(a);
                out.push(b);
            } else {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

fn truncate_paste(text: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }

    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClipboardProvider;
    use crate::app::input::keymap::{BindingMode, binding_for_test, keymap_for_test};
    use crate::clipboard::error::ClipboardOperation;
    use crate::clipboard::{ClipboardConfig, ClipboardError, ClipboardService};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct MockProvider {
        clipboard: Option<String>,
        primary: Option<String>,
        fail_read: bool,
        fail_write: bool,
        image: Option<crate::clipboard::ImageContent>,
    }

    impl ClipboardProvider for MockProvider {
        fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
            if self.fail_read {
                return Err(ClipboardError::provider(
                    ClipboardOperation::ReadClipboard,
                    "read failed",
                ));
            }
            Ok(self.clipboard.clone().unwrap_or_default())
        }

        fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            if self.fail_write {
                return Err(ClipboardError::provider(
                    ClipboardOperation::WriteClipboard,
                    "write failed",
                ));
            }
            self.clipboard = Some(text.to_string());
            Ok(())
        }

        fn read_primary_selection_text(&mut self) -> Result<String, ClipboardError> {
            Ok(self.primary.clone().unwrap_or_default())
        }

        fn write_primary_selection_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
            Ok(())
        }

        fn supports_primary_selection(&self) -> bool {
            true
        }

        fn read_clipboard_image(
            &mut self,
        ) -> Result<crate::clipboard::ImageContent, ClipboardError> {
            match &self.image {
                Some(img) => Ok(img.clone()),
                None => Err(ClipboardError::unsupported(
                    ClipboardOperation::ReadImageClipboard,
                )),
            }
        }
    }

    #[derive(Default)]
    struct MockContext {
        selection: Option<String>,
        can_copy: bool,
        can_cut: bool,
        can_paste: bool,
        accepts_image: bool,
        inserted: Vec<String>,
        inserted_images: Vec<crate::clipboard::ImageContent>,
        deleted: bool,
    }

    impl ClipboardContext for MockContext {
        fn selection_text(&self) -> Option<String> {
            self.selection.clone()
        }

        fn can_copy(&self) -> bool {
            self.can_copy
        }

        fn can_cut(&self) -> bool {
            self.can_cut
        }

        fn can_paste(&self) -> bool {
            self.can_paste
        }

        fn delete_selection(&mut self) -> bool {
            self.deleted = true;
            true
        }

        fn insert_text(&mut self, text: &str) -> bool {
            self.inserted.push(text.to_string());
            true
        }

        fn insert_image(&mut self, content: &crate::clipboard::ImageContent) -> bool {
            self.inserted_images.push(content.clone());
            true
        }

        fn accepts_image(&self) -> bool {
            self.accepts_image
        }
    }

    #[test]
    fn performable_copy_only_consumes_with_selection() {
        let bindings = vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )];
        let keymap = keymap_for_test(bindings);

        let provider = Box::new(MockProvider::default());
        let errors: Rc<RefCell<Vec<ClipboardError>>> = Rc::new(RefCell::new(Vec::new()));
        let reporter = {
            let errors = errors.clone();
            Rc::new(move |err| errors.borrow_mut().push(err))
        };
        let clipboard = ClipboardService::new(provider, reporter);
        let config = ClipboardConfig {
            enable_osc52: false,
            enable_primary_selection: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            selection: None,
            can_copy: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );
        assert!(!outcome.handled, "should not consume without selection");

        ctx.selection = Some("copy me".to_string());
        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );
        assert!(outcome.handled, "should consume with selection");
        assert!(errors.borrow().is_empty());
    }

    #[test]
    fn performable_binding_blocks_fallback() {
        let bindings = vec![
            binding_for_test("ctrl-c", Action::Copy, BindingMode::Performable),
            binding_for_test("ctrl-c", Action::Paste, BindingMode::Always),
        ];
        let keymap = keymap_for_test(bindings);

        let provider = Box::new(MockProvider::default());
        let clipboard = ClipboardService::new(provider, Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            enable_primary_selection: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            selection: None,
            can_copy: true,
            can_paste: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(
            !outcome.handled,
            "performable binding should allow fallthrough"
        );
        assert!(ctx.inserted.is_empty());
    }

    #[test]
    fn failed_copy_is_handled_but_not_reported_as_copied() {
        let bindings = vec![binding_for_test(
            "ctrl-c",
            Action::Copy,
            BindingMode::Performable,
        )];
        let keymap = keymap_for_test(bindings);

        let provider = MockProvider {
            fail_write: true,
            ..Default::default()
        };
        let errors: Rc<RefCell<Vec<ClipboardError>>> = Rc::new(RefCell::new(Vec::new()));
        let reporter = {
            let errors = errors.clone();
            Rc::new(move |err| errors.borrow_mut().push(err))
        };
        let clipboard = ClipboardService::new(Box::new(provider), reporter);
        let config = ClipboardConfig {
            enable_osc52: false,
            enable_primary_selection: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            selection: Some("copy me".to_string()),
            can_copy: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled);
        assert!(!outcome.copied);
        assert!(!outcome.mutated);
        assert_eq!(errors.borrow().len(), 1);
    }

    #[test]
    fn clipboard_errors_are_reported() {
        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Performable,
        )];
        let keymap = keymap_for_test(bindings);

        let provider = MockProvider {
            fail_read: true,
            ..Default::default()
        };

        let errors: Rc<RefCell<Vec<ClipboardError>>> = Rc::new(RefCell::new(Vec::new()));
        let reporter = {
            let errors = errors.clone();
            Rc::new(move |err| errors.borrow_mut().push(err))
        };
        let clipboard = ClipboardService::new(Box::new(provider), reporter);
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled);
        assert_eq!(errors.borrow().len(), 1);
    }

    #[test]
    fn performable_cut_requires_selection() {
        let bindings = vec![binding_for_test(
            "ctrl-x",
            Action::Cut,
            BindingMode::Performable,
        )];
        let keymap = keymap_for_test(bindings);

        let provider = Box::new(MockProvider::default());
        let clipboard = ClipboardService::new(provider, Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            selection: None,
            can_copy: true,
            can_cut: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );
        assert!(!outcome.handled, "cut should not consume without selection");

        ctx.selection = Some("cut".to_string());
        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );
        assert!(outcome.handled, "cut should consume with selection");
        assert!(ctx.deleted);
    }

    #[test]
    fn paste_image_calls_insert_image() {
        use crate::clipboard::{ImageContent, ImageFormat};

        let bindings = vec![binding_for_test(
            "ctrl-shift-i",
            Action::PasteImage,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        let image = ImageContent::from_bytes(b"fake-png-bytes", ImageFormat::Png);
        let provider = MockProvider {
            image: Some(image.clone()),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('i'),
                mods: KeyMods {
                    ctrl: true,
                    shift: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "paste-image should be handled");
        assert_eq!(ctx.inserted_images.len(), 1);
        assert_eq!(ctx.inserted_images[0], image);
    }

    #[test]
    fn paste_image_rejected_when_oversized() {
        use crate::clipboard::{ImageContent, ImageFormat};

        let bindings = vec![binding_for_test(
            "ctrl-shift-i",
            Action::PasteImage,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        // base64 of "fake-png-bytes" (~14 bytes raw, ~20 base64) - will exceed 10-byte limit
        let image = ImageContent::from_bytes(b"fake-png-bytes", ImageFormat::Png);
        let provider = MockProvider {
            image: Some(image),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            paste_max_image_bytes: 10, // very small limit
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('i'),
                mods: KeyMods {
                    ctrl: true,
                    shift: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "key event was consumed");
        assert!(
            ctx.inserted_images.is_empty(),
            "oversized image should be rejected"
        );
    }

    #[test]
    fn paste_image_no_op_when_no_image_in_clipboard() {
        let bindings = vec![binding_for_test(
            "ctrl-shift-i",
            Action::PasteImage,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        // Provider returns Unsupported (no image in clipboard)
        let provider = MockProvider {
            image: None,
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('i'),
                mods: KeyMods {
                    ctrl: true,
                    shift: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "key event was consumed");
        assert!(
            ctx.inserted_images.is_empty(),
            "no image should be inserted when clipboard has none"
        );
    }

    #[test]
    fn paste_auto_detects_image_when_context_accepts_image() {
        use crate::clipboard::{ImageContent, ImageFormat};

        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        let image = ImageContent::from_bytes(b"fake-png-bytes", ImageFormat::Png);
        let provider = MockProvider {
            image: Some(image.clone()),
            clipboard: Some("some text".to_string()),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            accepts_image: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "paste should be handled");
        assert_eq!(ctx.inserted_images.len(), 1, "image should be dispatched");
        assert_eq!(ctx.inserted_images[0], image);
        assert!(
            ctx.inserted.is_empty(),
            "text should not be inserted when image was dispatched"
        );
    }

    #[test]
    fn paste_falls_back_to_text_when_no_image_in_clipboard() {
        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        // No image in clipboard
        let provider = MockProvider {
            image: None,
            clipboard: Some("hello text".to_string()),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            accepts_image: true,
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "paste should be handled");
        assert!(
            ctx.inserted_images.is_empty(),
            "no image should be dispatched when clipboard has no image"
        );
        assert_eq!(ctx.inserted, vec!["hello text"]);
    }

    #[test]
    fn paste_falls_back_to_text_when_context_does_not_accept_image() {
        use crate::clipboard::{ImageContent, ImageFormat};

        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        // Image is in clipboard, but context does not accept images
        let image = ImageContent::from_bytes(b"fake-png-bytes", ImageFormat::Png);
        let provider = MockProvider {
            image: Some(image),
            clipboard: Some("text content".to_string()),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            accepts_image: false, // context does not accept images
            ..MockContext::default()
        };

        let outcome = dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert!(outcome.handled, "paste should be handled");
        assert!(
            ctx.inserted_images.is_empty(),
            "image should not be dispatched when context does not accept images"
        );
        assert_eq!(ctx.inserted, vec!["text content"]);
    }

    #[test]
    fn paste_suppresses_svg_vector_text_when_context_accepts_image() {
        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        let svg_payloads = [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="10"/></svg>"#,
            r#"<SVG xmlns="http://www.w3.org/2000/svg"></SVG>"#,
            r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>"#,
            "  \n<svg viewBox=\"0 0 100 100\"/>",
        ];

        for payload in svg_payloads {
            let provider = MockProvider {
                image: None,
                clipboard: Some(payload.to_string()),
                ..Default::default()
            };
            let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
            let config = ClipboardConfig {
                enable_osc52: false,
                ..Default::default()
            };

            let mut ctx = MockContext {
                can_paste: true,
                accepts_image: true,
                ..MockContext::default()
            };

            let outcome = dispatch_clipboard(
                KeyEvent {
                    code: KeyCode::Char('v'),
                    mods: KeyMods {
                        ctrl: true,
                        ..KeyMods::default()
                    },
                },
                &keymap,
                &mut ctx,
                &clipboard,
                &config,
            );

            assert!(
                outcome.handled,
                "paste should be handled for payload: {payload}"
            );
            assert!(
                ctx.inserted.is_empty(),
                "SVG vector text should be suppressed for payload: {payload}"
            );
            assert!(
                ctx.inserted_images.is_empty(),
                "no image should be inserted for SVG payload: {payload}"
            );
        }
    }

    #[test]
    fn paste_allows_vector_text_when_context_does_not_accept_image() {
        let bindings = vec![binding_for_test(
            "ctrl-v",
            Action::Paste,
            BindingMode::Always,
        )];
        let keymap = keymap_for_test(bindings);

        let provider = MockProvider {
            image: None,
            clipboard: Some(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#.to_string()),
            ..Default::default()
        };
        let clipboard = ClipboardService::new(Box::new(provider), Rc::new(|_| {}));
        let config = ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        };

        let mut ctx = MockContext {
            can_paste: true,
            accepts_image: false, // plain text context - SVG should pass through as text
            ..MockContext::default()
        };

        dispatch_clipboard(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            &keymap,
            &mut ctx,
            &clipboard,
            &config,
        );

        assert_eq!(
            ctx.inserted,
            vec![r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#],
            "SVG should be inserted as text when context does not accept images"
        );
    }
}
