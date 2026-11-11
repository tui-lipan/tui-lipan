//! Dragging out of the app: files and text from a widget to other applications.
//!
//! A widget offers a drag with [`Element::host_drag`](crate::Element::host_drag), or per item
//! through [`List::host_drag`](crate::widgets::List::host_drag) and the widgets built on it. When
//! the user starts dragging over it, in a host terminal that speaks the protocol, the runtime does
//! the rest: it offers the data, shows a label beside the pointer, answers the drop target, and,
//! for a drop on another machine, sends the files' contents. The app hears how it ended through
//! [`HostDrag::on_finished`].

use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use super::host::HostChannel;
use super::{
    DataStreamWriter, DndError, DragImageFormat, DragPosition, DropOperation, DropOperations,
    EntryKind, ErrorCode, ProgramCommand, TEXT_PLAIN, TEXT_PLAIN_UTF8, TerminalEvent, URI_LIST,
    file_uri, format_uri_list,
};
use crate::callback::Callback;

/// What a widget offers when the user drags it out of the app.
#[derive(Clone, Default)]
pub struct HostDrag {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) text: Option<String>,
    pub(crate) operations: DropOperations,
    pub(crate) image: Option<DragImage>,
    pub(crate) on_finished: Option<Callback<HostDragFinished>>,
}

impl fmt::Debug for HostDrag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostDrag")
            .field("files", &self.files)
            .field("text", &self.text)
            .field("operations", &self.operations)
            .field("image", &self.image)
            .finish_non_exhaustive()
    }
}

impl HostDrag {
    /// Drag these files and directories. Paths must be absolute and on this machine.
    ///
    /// Other applications receive them as a `text/uri-list` and as their paths in plain text.
    pub fn files<P: Into<PathBuf>>(paths: impl IntoIterator<Item = P>) -> Self {
        Self {
            files: paths
                .into_iter()
                .map(Into::into)
                .filter(|path: &PathBuf| path.is_absolute())
                .collect(),
            operations: DropOperations::COPY,
            ..Self::default()
        }
    }

    /// Drag text.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            operations: DropOperations::COPY,
            ..Self::default()
        }
    }

    /// Offer this text too, instead of the files' paths.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// What a drop target may do: copy (the default), move, or either. Offering a move means the
    /// app removes the originals when [`HostDrag::on_finished`] reports one.
    pub fn operations(mut self, operations: DropOperations) -> Self {
        self.operations = operations;
        self
    }

    /// The text shown beside the pointer while dragging, in the default style. Without one, it is
    /// the file name, the number of files, or the start of the text. Use [`Self::image`] for a
    /// different size, a transparent background, a picture, or nothing.
    pub fn label(self, label: impl Into<String>) -> Self {
        self.image(DragImage::text(label))
    }

    /// What is shown beside the pointer while dragging.
    pub fn image(mut self, image: DragImage) -> Self {
        self.image = Some(image);
        self
    }

    /// Hear how the drag ended.
    pub fn on_finished(mut self, callback: Callback<HostDragFinished>) -> Self {
        self.on_finished = Some(callback);
        self
    }

    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.text.as_deref().is_none_or(str::is_empty)
    }

    fn default_label(&self) -> String {
        match self.files.as_slice() {
            [one] => one
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| one.to_string_lossy().into_owned()),
            [] => {
                let text = self.text.as_deref().unwrap_or("");
                let line = text.lines().next().unwrap_or("");
                let mut label: String = line.chars().take(40).collect();
                if label.len() < line.len() || text.lines().nth(1).is_some() {
                    label.push('…');
                }
                label
            }
            many => format!("{} files", many.len()),
        }
    }
}

/// What is shown beside the pointer while dragging; see [`HostDrag::image`].
///
/// Text is drawn by the host terminal in its own font. A picture is a PNG or raw sRGB pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragImage(DragImageKind);

#[derive(Clone, Debug, PartialEq, Eq)]
enum DragImageKind {
    Text {
        /// `None` is the default label.
        text: Option<String>,
        /// The font size relative to the terminal's, as a fraction.
        scale: (u32, u32),
        opaque: bool,
    },
    Pixels {
        format: DragImageFormat,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Nothing,
}

impl Default for DragImage {
    fn default() -> Self {
        Self(DragImageKind::Text {
            text: None,
            scale: (1, 1),
            opaque: true,
        })
    }
}

impl DragImage {
    /// The default label, which [`DragImage::scale`] and [`DragImage::transparent`] can restyle.
    pub fn label() -> Self {
        Self::default()
    }

    /// This text, drawn by the terminal at its normal font size on an opaque background.
    pub fn text(text: impl Into<String>) -> Self {
        Self(DragImageKind::Text {
            text: Some(text.into()),
            scale: (1, 1),
            opaque: true,
        })
    }

    /// Draw text this many times the terminal's font size, such as `1.5`. Pictures ignore it.
    pub fn scale(mut self, scale: f32) -> Self {
        if let DragImageKind::Text {
            scale: fraction, ..
        } = &mut self.0
            && scale.is_finite()
            && scale > 0.0
        {
            *fraction = (((scale * 100.0).round() as u32).max(1), 100);
        }
        self
    }

    /// Draw text without a background, for a symbol or an emoji as the image. Pictures ignore it.
    pub fn transparent(mut self) -> Self {
        if let DragImageKind::Text { opaque, .. } = &mut self.0 {
            *opaque = false;
        }
        self
    }

    /// A PNG file's bytes. Bytes that are not a PNG are not shown, and the default label is used
    /// instead.
    pub fn png(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        // The size travels with the image, read from the PNG's header chunk.
        let size = |range: std::ops::Range<usize>| {
            bytes
                .get(range)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_be_bytes)
        };
        match (bytes.get(..16), size(16..20), size(20..24)) {
            (Some(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some(width), Some(height))
                if width > 0 && height > 0 =>
            {
                Self(DragImageKind::Pixels {
                    format: DragImageFormat::Png,
                    width,
                    height,
                    pixels: bytes,
                })
            }
            _ => Self::default(),
        }
    }

    /// Raw sRGB pixels, three bytes each, row by row. Pixels that do not fill `width × height`
    /// exactly are not shown, and the default label is used instead.
    pub fn rgb(width: u32, height: u32, pixels: impl Into<Vec<u8>>) -> Self {
        Self::pixels(DragImageFormat::Rgb, width, height, pixels.into(), 3)
    }

    /// Raw sRGB pixels with alpha, four bytes each, row by row. Pixels that do not fill
    /// `width × height` exactly are not shown, and the default label is used instead.
    pub fn rgba(width: u32, height: u32, pixels: impl Into<Vec<u8>>) -> Self {
        Self::pixels(DragImageFormat::Rgba, width, height, pixels.into(), 4)
    }

    /// Nothing beside the pointer.
    pub fn none() -> Self {
        Self(DragImageKind::Nothing)
    }

    fn pixels(
        format: DragImageFormat,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        size: u64,
    ) -> Self {
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|count| count.checked_mul(size));
        if width == 0 || height == 0 || expected != Some(pixels.len() as u64) {
            return Self::default();
        }
        Self(DragImageKind::Pixels {
            format,
            width,
            height,
            pixels,
        })
    }

    /// The image as the protocol sends it, or nothing for no image.
    fn encode(&self, default_label: impl FnOnce() -> String, out: &mut Vec<u8>) {
        let (format, width, height, opacity, bytes) = match &self.0 {
            DragImageKind::Nothing => return,
            DragImageKind::Text {
                text,
                scale,
                opaque,
            } => (
                DragImageFormat::Text,
                scale.0,
                scale.1,
                if *opaque { 1024 } else { 0 },
                text.clone().unwrap_or_else(default_label).into_bytes(),
            ),
            DragImageKind::Pixels {
                format,
                width,
                height,
                pixels,
            } => (*format, *width, *height, 1024, pixels.clone()),
        };
        stream(
            &ProgramCommand::PresentImage {
                image: 1,
                format,
                width,
                height,
                opacity,
                bytes: Vec::new(),
                end: true,
            },
            &bytes,
            out,
        );
    }
}

/// How a drag out of the app ended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostDragFinished {
    /// What the drop target did: [`DropOperation::None`] when the drag was canceled or refused.
    pub operation: DropOperation,
    /// The files that were dragged.
    pub files: Vec<PathBuf>,
}

/// Where a drag began within the widget that offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostDragStart {
    /// Cell column, from the widget's left edge.
    pub x: u16,
    /// Cell row, from the widget's top edge.
    pub y: u16,
    /// The widget's width in cells.
    pub width: u16,
    /// The widget's height in cells.
    pub height: u16,
}

/// Decides, when a drag begins over a widget, what it offers; `None` offers nothing there.
#[derive(Clone)]
pub struct HostDragSource(Rc<dyn Fn(HostDragStart) -> Option<HostDrag>>);

impl HostDragSource {
    /// A source that asks `offer` each time a drag begins.
    pub fn new(offer: impl Fn(HostDragStart) -> Option<HostDrag> + 'static) -> Self {
        Self(Rc::new(offer))
    }

    pub(crate) fn offer(&self, start: HostDragStart) -> Option<HostDrag> {
        (self.0)(start)
    }
}

impl fmt::Debug for HostDragSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostDragSource")
    }
}

impl PartialEq for HostDragSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Decides what an item of a list offers when a drag begins on it, by the item's index.
#[derive(Clone)]
pub struct HostDragItems(Rc<dyn Fn(usize) -> Option<HostDrag>>);

impl HostDragItems {
    /// Items that ask `offer` with the index of the item a drag begins on.
    pub fn new(offer: impl Fn(usize) -> Option<HostDrag> + 'static) -> Self {
        Self(Rc::new(offer))
    }

    pub(crate) fn offer(&self, index: usize) -> Option<HostDrag> {
        (self.0)(index)
    }
}

impl fmt::Debug for HostDragItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostDragItems")
    }
}

impl PartialEq for HostDragItems {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// What a host event did to the drag the runtime runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DragOutcome {
    /// Not the runtime's: the app hears it.
    Ignored,
    /// Handled. `started` when the host began the drag, which ends the pointer gesture under it.
    Consumed { started: bool },
}

/// The drag out of the app the runtime is running.
#[derive(Default)]
pub(crate) struct DragOut {
    session: Option<Session>,
}

struct Session {
    drag: HostDrag,
    data: Vec<Vec<u8>>,
    operation: DropOperation,
    remote: Option<Remote>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(remote) = &self.remote {
            remote.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl DragOut {
    /// Handle a host event. `find` names what is offered at a point of the window, if anything.
    pub(crate) fn host_event(
        &mut self,
        event: &TerminalEvent,
        find: impl FnOnce(DragPosition) -> Option<HostDrag>,
        channel: &HostChannel,
    ) -> DragOutcome {
        if let TerminalEvent::DragGesture { position } = event {
            // A new gesture: whatever drag came before is over.
            self.finish(DropOperation::None);
            let Some(drag) = find(*position).filter(|drag| !drag.is_empty()) else {
                return DragOutcome::Ignored;
            };
            self.session = Some(start(drag, channel));
            return DragOutcome::Consumed { started: false };
        }
        let Some(session) = &mut self.session else {
            return DragOutcome::Ignored;
        };
        match event {
            TerminalEvent::DragStarted => return DragOutcome::Consumed { started: true },
            TerminalEvent::DragFailed(_) => self.finish(DropOperation::None),
            TerminalEvent::DragAccepted { .. } => {
                if session.operation == DropOperation::None {
                    session.operation = session.drag.operations.preferred();
                }
            }
            TerminalEvent::DragActionChanged { operation } => session.operation = *operation,
            TerminalEvent::DragDropped => {}
            TerminalEvent::DragDataRequested { mime_index } => {
                let index = *mime_index as usize;
                let mut out = Vec::new();
                match session.data.get(index) {
                    Some(data) => {
                        let command = ProgramCommand::DragData {
                            mime_index: *mime_index,
                            bytes: Vec::new(),
                            end: true,
                        };
                        if let Some(meta) = command.data_meta() {
                            let mut writer = DataStreamWriter::new(meta);
                            writer.write(data, &mut out);
                            writer.finish(&mut out);
                        }
                    }
                    None => ProgramCommand::DragDataError {
                        mime_index: *mime_index,
                        error: DndError::new(ErrorCode::NotFound),
                    }
                    .encode(None, &mut out),
                }
                channel.push(&out);
            }
            TerminalEvent::RemoteEntryRequested { entry } => {
                let remote = session
                    .remote
                    .get_or_insert_with(|| Remote::spawn(session.drag.files.clone(), channel));
                remote.request(*entry);
            }
            TerminalEvent::DragFinished { canceled } => {
                let operation = if *canceled {
                    DropOperation::None
                } else {
                    session.operation
                };
                self.finish(operation);
            }
            _ => return DragOutcome::Ignored,
        }
        DragOutcome::Consumed { started: false }
    }

    fn finish(&mut self, operation: DropOperation) {
        if let Some(session) = self.session.take()
            && let Some(callback) = &session.drag.on_finished
        {
            callback.emit(HostDragFinished {
                operation,
                files: session.drag.files.clone(),
            });
        }
    }
}

/// Offer `drag` to the host and start the drag.
fn start(drag: HostDrag, channel: &HostChannel) -> Session {
    let mut mimes: Vec<std::sync::Arc<str>> = Vec::new();
    let mut data = Vec::new();
    if !drag.files.is_empty() {
        // A directory's URI ends with `/`, which a receiver on another machine relies on.
        let uris: Vec<String> = drag
            .files
            .iter()
            .filter_map(|path| {
                let uri = file_uri(path)?;
                Some(if path.is_dir() {
                    format!("{uri}/")
                } else {
                    uri
                })
            })
            .collect();
        mimes.push(URI_LIST.into());
        data.push(format_uri_list(&uris));
    }
    let text = drag.text.clone().or_else(|| {
        (!drag.files.is_empty()).then(|| {
            drag.files
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n")
        })
    });
    if let Some(text) = text {
        for mime in [TEXT_PLAIN_UTF8, TEXT_PLAIN] {
            mimes.push(mime.into());
            data.push(text.clone().into_bytes());
        }
    }

    let mut out = Vec::new();
    ProgramCommand::Offer {
        operations: drag.operations,
        mimes,
    }
    .encode(None, &mut out);
    // Sent ahead: some platforms need the data before the drag starts.
    for (index, bytes) in data.iter().enumerate() {
        stream(
            &ProgramCommand::PresentData {
                mime_index: index as u32,
                bytes: Vec::new(),
                end: true,
            },
            bytes,
            &mut out,
        );
    }
    drag.image
        .clone()
        .unwrap_or_default()
        .encode(|| drag.default_label(), &mut out);
    ProgramCommand::StartDrag.encode(None, &mut out);
    channel.push(&out);
    Session {
        operation: DropOperation::None,
        drag,
        data,
        remote: None,
    }
}

fn stream(command: &ProgramCommand, bytes: &[u8], out: &mut Vec<u8>) {
    if let Some(meta) = command.data_meta() {
        let mut writer = DataStreamWriter::new(meta);
        writer.write(bytes, out);
        writer.finish(out);
    }
}

/// The most the background reader lets wait for the host before it pauses.
const REMOTE_BACKLOG: usize = 8 << 20;
/// How much of a file is read at a time.
const READ_CHUNK: usize = 64 << 10;

/// Sends the dragged files' contents to a drop target on another machine, off the UI thread.
struct Remote {
    requests: mpsc::Sender<u32>,
    cancel: Arc<AtomicBool>,
}

impl Remote {
    fn spawn(files: Vec<PathBuf>, channel: &HostChannel) -> Self {
        let (requests, receive) = mpsc::channel::<u32>();
        let cancel = Arc::new(AtomicBool::new(false));
        let sink = channel.background_sink();
        let stop = Arc::clone(&cancel);
        let spawned = std::thread::Builder::new()
            .name("tui-lipan-drag-out".into())
            .spawn(move || {
                let mut handles = 1u32;
                while let Ok(entry) = receive.recv() {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(path) = files.get((entry as usize).wrapping_sub(1)) else {
                        sink.send(&encoded(&ProgramCommand::DragError(DndError::new(
                            ErrorCode::NotFound,
                        ))));
                        continue;
                    };
                    let mut writer = EntryWriter {
                        sink: &sink,
                        stop: &stop,
                        handles: &mut handles,
                    };
                    if let Err(error) = writer.send_tree(entry, path) {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let error = DndError::with_description(
                            ErrorCode::from_io(&error),
                            error.to_string(),
                        );
                        sink.send(&encoded(&ProgramCommand::DragError(error)));
                    }
                }
            });
        if spawned.is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
        Self { requests, cancel }
    }

    fn request(&self, entry: u32) {
        let _ = self.requests.send(entry);
    }
}

fn encoded(command: &ProgramCommand) -> Vec<u8> {
    let mut out = Vec::new();
    command.encode(None, &mut out);
    out
}

/// Where the background reader's escape codes go: an outbox the runner drains, and a wake.
#[derive(Clone)]
pub(crate) struct BackgroundSink {
    pub(crate) outbox: Arc<Mutex<Vec<u8>>>,
    pub(crate) wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl BackgroundSink {
    fn send(&self, bytes: &[u8]) {
        if let Ok(mut outbox) = self.outbox.lock() {
            outbox.extend_from_slice(bytes);
        }
        if let Some(wake) = &self.wake {
            wake();
        }
    }

    fn backlog(&self) -> usize {
        self.outbox.lock().map_or(0, |outbox| outbox.len())
    }
}

struct EntryWriter<'a> {
    sink: &'a BackgroundSink,
    stop: &'a AtomicBool,
    handles: &'a mut u32,
}

impl EntryWriter<'_> {
    /// Send an entry of the URI list, and every child of a directory, breadth first.
    fn send_tree(&mut self, entry: u32, path: &Path) -> std::io::Result<()> {
        let mut queue = std::collections::VecDeque::from([(None, path.to_path_buf())]);
        while let Some((child, path)) = queue.pop_front() {
            if self.stop.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("drag ended"));
            }
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                let target = std::fs::read_link(&path)?;
                self.send_entry(entry, child, EntryKind::Symlink, |write| {
                    write(target.to_string_lossy().as_bytes());
                    Ok(())
                })?;
            } else if meta.is_dir() {
                *self.handles = self.handles.saturating_add(1).max(2);
                let handle = *self.handles;
                // A directory goes whole or not at all: an entry that cannot be read fails the
                // drag rather than leave a silently incomplete copy. Only kinds the protocol has
                // no place for (sockets, devices, pipes) are left out.
                let mut names: Vec<(String, PathBuf)> = Vec::new();
                for entry in std::fs::read_dir(&path)? {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    if kind.is_file() || kind.is_dir() || kind.is_symlink() {
                        names.push((
                            entry.file_name().to_string_lossy().into_owned(),
                            entry.path(),
                        ));
                    }
                }
                names.sort();
                let listing = names
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("\0");
                self.send_entry(entry, child, EntryKind::Directory(handle), |write| {
                    write(listing.as_bytes());
                    Ok(())
                })?;
                for (index, (_, child_path)) in names.into_iter().enumerate() {
                    queue.push_back((Some((handle, index as u32 + 1)), child_path));
                }
            } else if meta.is_file() {
                let mut file = std::fs::File::open(&path)?;
                self.send_entry(entry, child, EntryKind::File, |write| {
                    let mut buffer = vec![0; READ_CHUNK];
                    loop {
                        let read = file.read(&mut buffer)?;
                        if read == 0 {
                            return Ok(());
                        }
                        write(&buffer[..read]);
                    }
                })?;
            }
        }
        Ok(())
    }

    fn send_entry(
        &mut self,
        entry: u32,
        child: Option<(u32, u32)>,
        kind: EntryKind,
        body: impl FnOnce(&mut dyn FnMut(&[u8])) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let command = ProgramCommand::RemoteEntry {
            entry,
            child,
            kind,
            bytes: Vec::new(),
            end: true,
        };
        let Some(meta) = command.data_meta() else {
            return Ok(());
        };
        let mut writer = DataStreamWriter::new(meta);
        let sink = self.sink;
        let stop = self.stop;
        let mut stopped = false;
        let result = body(&mut |bytes| {
            // The host reads as fast as it can; past a backlog, wait for it.
            while sink.backlog() > REMOTE_BACKLOG && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if stop.load(Ordering::Relaxed) {
                stopped = true;
                return;
            }
            let mut out = Vec::new();
            writer.write(bytes, &mut out);
            sink.send(&out);
        });
        if stopped {
            return Err(std::io::Error::other("drag ended"));
        }
        result?;
        let mut out = Vec::new();
        writer.finish(&mut out);
        sink.send(&out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::{CommandDecoder, OscScanner};

    fn commands(bytes: &[u8]) -> Vec<ProgramCommand> {
        let mut decoder = CommandDecoder::default();
        OscScanner::default()
            .scan(bytes)
            .into_iter()
            .flat_map(|(_, body)| decoder.push_body(&body))
            .map(|tagged| tagged.command)
            .collect()
    }

    fn sent_bytes(sent: &[ProgramCommand]) -> Vec<u8> {
        let mut out = Vec::new();
        for command in sent {
            command.encode(None, &mut out);
        }
        out
    }

    fn channel() -> HostChannel {
        let channel = HostChannel::default();
        channel.set_supported(true);
        channel
    }

    fn gesture() -> TerminalEvent {
        TerminalEvent::DragGesture {
            position: DragPosition::default(),
        }
    }

    #[test]
    fn a_gesture_over_nothing_is_left_to_the_app() {
        let channel = channel();
        let mut out = DragOut::default();
        assert_eq!(
            out.host_event(&gesture(), |_| None, &channel),
            DragOutcome::Ignored
        );
        assert_eq!(
            out.host_event(&TerminalEvent::DragStarted, |_| None, &channel),
            DragOutcome::Ignored,
            "no drag of the runtime's is going"
        );
    }

    #[test]
    fn files_are_offered_as_uris_and_paths_with_a_label() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo one.png");
        std::fs::write(&file, b"png").unwrap();
        let channel = channel();
        let mut out = DragOut::default();
        let outcome = out.host_event(
            &gesture(),
            |_| Some(HostDrag::files([file.clone(), dir.path().to_path_buf()])),
            &channel,
        );
        assert_eq!(outcome, DragOutcome::Consumed { started: false });
        let sent = commands(&channel.take_output());
        let ProgramCommand::Offer { operations, mimes } = &sent[0] else {
            panic!("{sent:?}");
        };
        assert_eq!(*operations, DropOperations::COPY);
        assert_eq!(
            mimes.iter().map(|m| m.as_ref()).collect::<Vec<_>>(),
            [URI_LIST, TEXT_PLAIN_UTF8, TEXT_PLAIN]
        );
        let uri_list: Vec<u8> = sent
            .iter()
            .filter_map(|command| match command {
                ProgramCommand::PresentData {
                    mime_index: 0,
                    bytes,
                    ..
                } => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        let uris = crate::dnd::parse_uri_list(&uri_list);
        assert_eq!(uris.len(), 2);
        assert!(uris[0].ends_with("photo%20one.png"));
        assert!(
            uris[1].ends_with('/'),
            "a directory's URI ends with a slash"
        );
        let label: Vec<u8> = sent
            .iter()
            .filter_map(|command| match command {
                ProgramCommand::PresentImage {
                    format: DragImageFormat::Text,
                    bytes,
                    ..
                } => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(label, b"2 files");
        assert_eq!(sent.last(), Some(&ProgramCommand::StartDrag));
    }

    fn image_of(drag: HostDrag) -> Vec<ProgramCommand> {
        let channel = channel();
        let mut out = DragOut::default();
        out.host_event(&gesture(), |_| Some(drag), &channel);
        commands(&channel.take_output())
            .into_iter()
            .filter(|command| matches!(command, ProgramCommand::PresentImage { .. }))
            .collect()
    }

    /// The image's format, width, height and opacity, and its bytes joined.
    type Image = (Option<(DragImageFormat, u32, u32, u32)>, Vec<u8>);

    fn joined(images: &[ProgramCommand]) -> Image {
        let mut head = None;
        let mut bytes = Vec::new();
        for image in images {
            if let ProgramCommand::PresentImage {
                format,
                width,
                height,
                opacity,
                bytes: piece,
                ..
            } = image
            {
                head = Some((*format, *width, *height, *opacity));
                bytes.extend(piece);
            }
        }
        (head, bytes)
    }

    #[test]
    fn the_image_beside_the_pointer_is_the_apps_to_choose() {
        let drag = || HostDrag::text("hi");
        assert_eq!(
            joined(&image_of(drag())),
            (Some((DragImageFormat::Text, 1, 1, 1024)), b"hi".to_vec()),
            "the default label"
        );
        assert_eq!(
            joined(&image_of(
                drag().image(DragImage::label().scale(1.5).transparent())
            )),
            (Some((DragImageFormat::Text, 150, 100, 0)), b"hi".to_vec())
        );
        assert_eq!(
            joined(&image_of(drag().image(DragImage::text("★").scale(2.0)))),
            (
                Some((DragImageFormat::Text, 200, 100, 1024)),
                "★".as_bytes().to_vec()
            )
        );
        assert_eq!(
            joined(&image_of(drag().image(DragImage::rgba(1, 2, vec![9; 8])))),
            (Some((DragImageFormat::Rgba, 1, 2, 1024)), vec![9; 8])
        );
        assert_eq!(
            joined(&image_of(drag().image(DragImage::rgb(2, 2, vec![9; 5])))),
            (Some((DragImageFormat::Text, 1, 1, 1024)), b"hi".to_vec()),
            "pixels that do not fit fall back to the label"
        );
        assert_eq!(
            joined(&image_of(drag().image(DragImage::rgba(
                u32::MAX,
                u32::MAX,
                vec![0; 4]
            ))))
            .0,
            Some((DragImageFormat::Text, 1, 1, 1024)),
            "a size too large to count falls back too"
        );
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(16u32.to_be_bytes());
        png.extend(8u32.to_be_bytes());
        assert_eq!(
            joined(&image_of(drag().image(DragImage::png(png)))).0,
            Some((DragImageFormat::Png, 16, 8, 1024)),
            "a PNG's size comes from its header"
        );
        assert_eq!(
            joined(&image_of(drag().image(DragImage::png(b"\x89PNG".to_vec())))).0,
            Some((DragImageFormat::Text, 1, 1, 1024)),
            "bytes that are not a PNG fall back to the label"
        );
        assert!(image_of(drag().image(DragImage::none())).is_empty());
    }

    #[test]
    fn the_drag_reports_how_it_ended() {
        let channel = channel();
        let finished = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = Rc::clone(&finished);
        let drag = HostDrag::files(["/tmp/x"])
            .operations(DropOperations::EITHER)
            .on_finished(Callback::new(move |done: HostDragFinished| {
                seen.borrow_mut().push(done.operation)
            }));
        let mut out = DragOut::default();
        out.host_event(&gesture(), |_| Some(drag.clone()), &channel);
        assert_eq!(
            out.host_event(&TerminalEvent::DragStarted, |_| None, &channel),
            DragOutcome::Consumed { started: true }
        );
        out.host_event(
            &TerminalEvent::DragActionChanged {
                operation: DropOperation::Move,
            },
            |_| None,
            &channel,
        );
        out.host_event(
            &TerminalEvent::DragFinished { canceled: false },
            |_| None,
            &channel,
        );
        out.host_event(&gesture(), |_| Some(drag.clone()), &channel);
        out.host_event(
            &TerminalEvent::DragFinished { canceled: true },
            |_| None,
            &channel,
        );
        assert_eq!(
            *finished.borrow(),
            [DropOperation::Move, DropOperation::None]
        );
    }

    #[test]
    fn a_requested_type_is_sent_again() {
        let channel = channel();
        let mut out = DragOut::default();
        out.host_event(&gesture(), |_| Some(HostDrag::text("hello")), &channel);
        channel.take_output();
        out.host_event(
            &TerminalEvent::DragDataRequested { mime_index: 1 },
            |_| None,
            &channel,
        );
        let sent = commands(&channel.take_output());
        let data: Vec<u8> = sent
            .iter()
            .filter_map(|command| match command {
                ProgramCommand::DragData {
                    mime_index: 1,
                    bytes,
                    ..
                } => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(data, b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_read_fails_the_drag_rather_than_arrive_incomplete() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("secret.txt"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            // Running with privileges that read it anyway: nothing to test.
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let channel = channel();
        let mut out = DragOut::default();
        out.host_event(
            &gesture(),
            |_| Some(HostDrag::files([dir.path().to_path_buf()])),
            &channel,
        );
        channel.take_output();
        out.host_event(
            &TerminalEvent::RemoteEntryRequested { entry: 1 },
            |_| None,
            &channel,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut sent = Vec::new();
        while !sent
            .iter()
            .any(|c| matches!(c, ProgramCommand::DragError(_)))
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
            sent = commands(&[sent_bytes(&sent), channel.take_output()].concat());
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                sent.last(),
                Some(ProgramCommand::DragError(error)) if error.code == ErrorCode::PermissionDenied
            ),
            "{sent:?}"
        );
    }

    #[test]
    fn a_directory_goes_to_another_machine_whole() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), vec![b'b'; 200_000]).unwrap();
        let channel = channel();
        let mut out = DragOut::default();
        out.host_event(
            &gesture(),
            |_| Some(HostDrag::files([dir.path().to_path_buf()])),
            &channel,
        );
        channel.take_output();
        out.host_event(
            &TerminalEvent::RemoteEntryRequested { entry: 1 },
            |_| None,
            &channel,
        );
        // Every entry ends with an empty message; four entries are coming.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut sent = Vec::new();
        let ended = |sent: &[ProgramCommand]| {
            sent.iter()
                .filter(|c| matches!(c, ProgramCommand::RemoteEntry { end: true, .. }))
                .count()
        };
        while ended(&sent) < 4 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
            sent = commands(&[sent_bytes(&sent), channel.take_output()].concat());
        }
        // Each entry's pieces, joined, in the order the entries began.
        type Entry = (Option<(u32, u32)>, EntryKind, Vec<u8>);
        let mut complete: Vec<Entry> = Vec::new();
        for command in sent {
            if let ProgramCommand::RemoteEntry {
                child, kind, bytes, ..
            } = command
            {
                match complete.iter_mut().find(|(c, _, _)| *c == child) {
                    Some((_, _, data)) => data.extend(bytes),
                    None => complete.push((child, kind, bytes)),
                }
            }
        }
        let (_, root_kind, listing) = &complete[0];
        let EntryKind::Directory(root) = root_kind else {
            panic!("{complete:?}");
        };
        assert_eq!(listing, b"a.txt\0sub");
        assert_eq!(
            complete[1],
            (Some((*root, 1)), EntryKind::File, b"alpha".to_vec())
        );
        let EntryKind::Directory(sub) = complete[2].1 else {
            panic!("{complete:?}");
        };
        assert_eq!(complete[2].0, Some((*root, 2)));
        assert_eq!(complete[3].0, Some((sub, 1)));
        assert_eq!(complete[3].2.len(), 200_000);
    }
}
