//! asciinema cast v2 writer.
//!
//! A terminal recording is text, not video: a small JSON header followed by one
//! `[time, "o", data]` line per output chunk, plus `[time, "r", "COLSxROWS"]` when the
//! terminal changes size and `[time, "m", label]` for a marker a player can jump to. Because [`CapturedFrame::to_ansi_diff`]
//! already emits exactly that `data` - the ANSI needed to turn one frame into the
//! next - recording a tui-lipan app is mostly bookkeeping.
//!
//! The result is a fraction of a video's size, scrubbable, selectable as text, and
//! playable in a browser by asciinema-player.
//!
//! JSON is written by hand rather than through `serde_json`, so recording needs no
//! optional feature and no dependency that could clash with an application's
//! pinned lockfile.

use std::fmt::Write as _;
use std::path::Path;

use super::CapturedFrame;
use crate::Result;

/// Terminal type advertised to players, so colours are interpreted correctly.
const CAST_TERM: &str = "xterm-256color";

/// An asciinema cast v2 recording, built frame by frame.
///
/// # Example
///
/// ```rust
/// use tui_lipan::CastRecording;
///
/// let mut recording = CastRecording::new(80, 24).title("Demo");
/// recording.push_output(0.0, "\x1b[2Jhello".to_owned());
/// recording.push_marker(0.5, "greeting shown");
/// recording.push_resize(1.0, 100, 30);
/// let cast = recording.to_cast();
/// assert!(cast.starts_with("{\"version\":2"));
/// assert!(cast.contains("[0.500000, \"m\", \"greeting shown\"]"));
/// assert!(cast.contains("[1.000000, \"r\", \"100x30\"]"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CastRecording {
    width: u16,
    height: u16,
    /// The terminal size as of the last event: the header's size until a resize.
    current_width: u16,
    current_height: u16,
    title: Option<String>,
    events: Vec<CastEvent>,
    last_frame: Option<CapturedFrame>,
}

/// One event: seconds since recording start, its asciicast code, and its data.
#[derive(Clone, Debug, PartialEq)]
struct CastEvent {
    time: f64,
    kind: CastEventKind,
    data: String,
}

/// The asciicast v2 event types this writer emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CastEventKind {
    /// Terminal output (`"o"`).
    Output,
    /// A new terminal size, as `COLSxROWS` (`"r"`).
    Resize,
    /// A labelled marker (`"m"`).
    Marker,
}

impl CastEventKind {
    fn code(self) -> &'static str {
        match self {
            Self::Output => "o",
            Self::Resize => "r",
            Self::Marker => "m",
        }
    }
}

impl CastRecording {
    /// Start a recording for a terminal of `width` x `height` cells.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            current_width: width.max(1),
            current_height: height.max(1),
            title: None,
            events: Vec::new(),
            last_frame: None,
        }
    }

    /// Set the title stored in the cast header.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Append a frame at `time_secs`, encoded as a diff against the previous one.
    ///
    /// Frames identical to the previous frame are dropped rather than written as
    /// empty events: a player derives timing from event timestamps, so a still
    /// stretch costs nothing but the gap before the next change.
    ///
    /// A frame whose size differs from the terminal's is preceded by a resize
    /// event (see [`Self::push_resize`]) and drawn as a full repaint, so a
    /// recording of a window that grows or shrinks plays back at each size.
    ///
    /// Returns whether an event was recorded.
    pub fn push_frame(&mut self, time_secs: f64, frame: &CapturedFrame) -> bool {
        if self.last_frame.as_ref() == Some(frame) {
            return false;
        }
        self.push_resize(time_secs, frame.width, frame.height);
        let data = frame.to_ansi_diff(self.last_frame.as_ref());
        self.last_frame = Some(frame.clone());
        self.push_output(time_secs, data);
        true
    }

    /// Append raw terminal output at `time_secs`.
    ///
    /// Prefer [`Self::push_frame`]; this exists for output a captured frame cannot
    /// express, such as a closing message.
    pub fn push_output(&mut self, time_secs: f64, data: String) {
        self.push_event(time_secs, CastEventKind::Output, data);
    }

    /// Record that the terminal became `width` x `height` cells at `time_secs`.
    ///
    /// [`Self::push_frame`] calls this itself when a frame's size changes; call
    /// it directly only alongside [`Self::push_output`]. The header keeps the
    /// size the recording started at. Does nothing, and returns `false`, when
    /// the size is unchanged.
    pub fn push_resize(&mut self, time_secs: f64, width: u16, height: u16) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.current_width, self.current_height) {
            return false;
        }
        self.current_width = width;
        self.current_height = height;
        self.push_event(
            time_secs,
            CastEventKind::Resize,
            format!("{width}x{height}"),
        );
        true
    }

    /// Add a marker labelled `label` at `time_secs`, which players list as a
    /// chapter or a point to jump to. It does not change the screen.
    pub fn push_marker(&mut self, time_secs: f64, label: impl Into<String>) {
        self.push_event(time_secs, CastEventKind::Marker, label.into());
    }

    fn push_event(&mut self, time_secs: f64, kind: CastEventKind, data: String) {
        self.events.push(CastEvent {
            time: time_secs.max(0.0),
            kind,
            data,
        });
    }

    /// Extend the recording to `time_secs` without changing the screen.
    ///
    /// Dropping identical frames means a recording otherwise *ends* at its last
    /// visual change, so a player would cut away the moment the last thing
    /// happened. This writes a zero-length output event to hold the final frame
    /// for the intended duration.
    ///
    /// Does nothing when the recording already extends past `time_secs`.
    pub fn mark_time(&mut self, time_secs: f64) {
        if time_secs > self.duration_secs() {
            self.push_output(time_secs, String::new());
        }
    }

    /// Number of recorded events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` when nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Timestamp of the last event, in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.events.last().map(|event| event.time).unwrap_or(0.0)
    }

    /// Render the recording as asciinema cast v2 text.
    ///
    /// No wall-clock `timestamp` field is emitted, so the same run always produces
    /// the same bytes and a committed recording stays diffable.
    pub fn to_cast(&self) -> String {
        let mut out = String::with_capacity(self.events.len() * 64 + 128);

        out.push_str("{\"version\":2,\"width\":");
        let _ = write!(out, "{}", self.width);
        out.push_str(",\"height\":");
        let _ = write!(out, "{}", self.height);
        if let Some(title) = self.title.as_deref() {
            out.push_str(",\"title\":\"");
            escape_json_into(&mut out, title);
            out.push('"');
        }
        out.push_str(",\"env\":{\"TERM\":\"");
        out.push_str(CAST_TERM);
        out.push_str("\"}}\n");

        for event in &self.events {
            out.push('[');
            write_time(&mut out, event.time);
            out.push_str(", \"");
            out.push_str(event.kind.code());
            out.push_str("\", \"");
            escape_json_into(&mut out, &event.data);
            out.push_str("\"]\n");
        }

        out
    }

    /// Write the recording to `path`.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.to_cast()).map_err(crate::Error::from)
    }
}

/// Write a timestamp with fixed precision.
///
/// Six decimals matches asciinema's own output and keeps frame ordering stable at
/// any practical frame rate.
fn write_time(out: &mut String, time: f64) {
    let _ = write!(out, "{time:.6}");
}

/// Append `value` to `out` as the inside of a JSON string.
///
/// ANSI data is mostly control bytes, so this has to escape the full C0 range -
/// an unescaped `\x1b` is invalid JSON and every player would reject the file.
fn escape_json_into(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || ch == '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CapturedCell, CellModifiers};
    use crate::style::{Color, Rect};

    fn frame(width: u16, height: u16, fill: &str) -> CapturedFrame {
        let cells = (0..usize::from(width) * usize::from(height))
            .map(|_| CapturedCell {
                symbol: fill.to_owned(),
                fg: Color::White,
                bg: Color::Black,
                underline_color: Color::Reset,
                modifiers: CellModifiers::default(),
            })
            .collect();
        CapturedFrame {
            viewport: Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            width,
            height,
            cells,
            cursor: None,
            images: Vec::new(),
        }
    }

    /// Replay a cast's event stream into a text grid.
    ///
    /// Handles exactly the sequences [`CapturedFrame::to_ansi_diff`] emits: erase,
    /// cursor addressing, SGR, and cursor visibility. A recording is only useful
    /// if it reconstructs, so this exercises the format end to end rather than
    /// trusting that the JSON looked right.
    fn replay(cast: &str) -> Vec<String> {
        let mut lines = cast.lines();
        let header = lines.next().expect("header");
        let width: usize = extract_number(header, "\"width\":");
        let height: usize = extract_number(header, "\"height\":");

        let (mut width, mut height) = (width, height);
        let mut grid = vec![vec![' '; width]; height];
        let (mut cy, mut cx) = (0usize, 0usize);

        for line in lines {
            let (code, data) = decode_event(line);
            match code.as_str() {
                "o" => {}
                "r" => {
                    let (cols, rows) = data.split_once('x').expect("COLSxROWS");
                    width = cols.parse().expect("columns");
                    height = rows.parse().expect("rows");
                    grid = vec![vec![' '; width]; height];
                    continue;
                }
                _ => continue,
            }
            let mut chars = data.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch != '\u{1b}' {
                    if cy < height && cx < width {
                        grid[cy][cx] = ch;
                    }
                    cx += 1;
                    continue;
                }
                if chars.peek() != Some(&'[') {
                    continue;
                }
                chars.next();
                let mut params = String::new();
                let mut final_byte = ' ';
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        final_byte = ch;
                        break;
                    }
                    params.push(ch);
                }
                match final_byte {
                    'J' if params == "2" || params == "3" => {
                        grid = vec![vec![' '; width]; height];
                    }
                    'H' => {
                        let nums: Vec<usize> = params
                            .split(';')
                            .filter_map(|part| part.parse().ok())
                            .collect();
                        let (row, col) = match nums.as_slice() {
                            [row, col] => (*row, *col),
                            _ => (1, 1),
                        };
                        cy = row.saturating_sub(1);
                        cx = col.saturating_sub(1);
                    }
                    // SGR and cursor visibility do not move the write head.
                    _ => {}
                }
            }
        }

        grid.into_iter()
            .map(|row| row.into_iter().collect::<String>().trim_end().to_owned())
            .collect()
    }

    /// Pull an integer that follows `key` in the header line.
    fn extract_number(header: &str, key: &str) -> usize {
        let rest = &header[header.find(key).expect("key present") + key.len()..];
        rest.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("number")
    }

    /// Extract the code and unescaped data from one `[t, "code", "..."]` line.
    fn decode_event(line: &str) -> (String, String) {
        let code_start = line.find(", \"").expect("event code") + 3;
        let code_end = code_start + line[code_start..].find('"').expect("code ends");
        let code = line[code_start..code_end].to_owned();
        let start = code_end + 4;
        let body = &line[start..line.len() - 2];

        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).expect("hex escape");
                    out.push(char::from_u32(code).expect("valid char"));
                }
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            }
        }
        (code, out)
    }

    #[test]
    fn a_replayed_cast_reconstructs_the_final_frame() {
        let mut recording = CastRecording::new(6, 2);
        recording.push_frame(0.0, &frame(6, 2, "a"));
        recording.push_frame(1.0, &frame(6, 2, "b"));

        let replayed = replay(&recording.to_cast());
        assert_eq!(
            replayed,
            vec!["bbbbbb".to_owned(), "bbbbbb".to_owned()],
            "the last frame must survive a full encode/replay round trip"
        );
    }

    #[test]
    fn a_replayed_partial_diff_keeps_unchanged_cells() {
        let mut first = frame(4, 1, "x");
        let mut second = first.clone();
        second.cells[2].symbol = "Z".to_owned();

        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &first);
        recording.push_frame(1.0, &second);

        // Only one cell changed, so the second event must be a targeted write
        // that leaves its neighbours intact after replay.
        assert_eq!(replay(&recording.to_cast()), vec!["xxZx".to_owned()]);
        first.cells[2].symbol = "Z".to_owned();
        assert_eq!(first, second);
    }

    #[test]
    fn header_declares_v2_and_dimensions() {
        let cast = CastRecording::new(120, 40).title("Demo").to_cast();
        let header = cast.lines().next().expect("header line");
        assert!(header.contains("\"version\":2"), "{header}");
        assert!(header.contains("\"width\":120"), "{header}");
        assert!(header.contains("\"height\":40"), "{header}");
        assert!(header.contains("\"title\":\"Demo\""), "{header}");
    }

    #[test]
    fn escapes_ansi_control_bytes_so_the_json_stays_valid() {
        let mut recording = CastRecording::new(10, 2);
        recording.push_output(0.0, "\x1b[31mred\x1b[0m".to_owned());
        let cast = recording.to_cast();

        assert!(
            cast.contains("\\u001b[31mred"),
            "escape byte must be \\u001b encoded: {cast}"
        );
        assert!(
            !cast.contains('\u{1b}'),
            "no raw escape byte may survive into the file"
        );
    }

    #[test]
    fn escapes_quotes_and_backslashes() {
        let mut recording = CastRecording::new(10, 2);
        recording.push_output(0.0, r#"say "hi" \ bye"#.to_owned());
        let cast = recording.to_cast();
        assert!(cast.contains(r#"say \"hi\" \\ bye"#), "{cast}");
    }

    #[test]
    fn first_frame_is_a_full_repaint_and_later_frames_are_diffs() {
        let mut recording = CastRecording::new(4, 1);
        assert!(recording.push_frame(0.0, &frame(4, 1, "a")));
        assert!(recording.push_frame(1.0, &frame(4, 1, "b")));

        let cast = recording.to_cast();
        let lines: Vec<&str> = cast.lines().collect();
        assert_eq!(lines.len(), 3, "header + two events: {cast}");
        // A full repaint clears the screen; an incremental update must not.
        assert!(
            lines[1].contains("[2J"),
            "first event repaints: {}",
            lines[1]
        );
        assert!(
            !lines[2].contains("[2J"),
            "second event should be a diff: {}",
            lines[2]
        );
    }

    #[test]
    fn a_frame_at_a_new_size_resizes_the_terminal_before_drawing() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        recording.push_frame(1.0, &frame(6, 2, "b"));

        let cast = recording.to_cast();
        let lines: Vec<&str> = cast.lines().collect();
        assert!(
            lines[0].contains("\"width\":4"),
            "the header keeps the starting size"
        );
        assert_eq!(lines[2], "[1.000000, \"r\", \"6x2\"]", "{cast}");
        assert!(
            lines[3].contains("[2J"),
            "a new size repaints: {}",
            lines[3]
        );
        assert_eq!(
            replay(&cast),
            vec!["bbbbbb".to_owned(), "bbbbbb".to_owned()]
        );

        // Shrinking back leaves nothing of the larger size behind.
        recording.push_frame(2.0, &frame(3, 1, "c"));
        let cast = recording.to_cast();
        assert!(cast.contains("[2.000000, \"r\", \"3x1\"]"), "{cast}");
        assert_eq!(replay(&cast), vec!["ccc".to_owned()]);
    }

    #[test]
    fn a_first_frame_unlike_the_header_size_resizes_at_once() {
        let mut recording = CastRecording::new(80, 24);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        let cast = recording.to_cast();
        assert_eq!(cast.lines().nth(1), Some("[0.000000, \"r\", \"4x1\"]"));
        assert_eq!(replay(&cast), vec!["aaaa".to_owned()]);
    }

    #[test]
    fn a_resize_to_the_current_size_records_nothing() {
        let mut recording = CastRecording::new(4, 1);
        assert!(!recording.push_resize(0.0, 4, 1));
        assert!(recording.push_resize(0.5, 5, 2));
        assert!(!recording.push_resize(1.0, 5, 2));
        assert_eq!(recording.len(), 1);
        // Frames of the same size as a resize the caller already recorded add none.
        recording.push_frame(1.0, &frame(5, 2, "x"));
        assert_eq!(recording.len(), 2);
        assert!(recording.to_cast().contains("[0.500000, \"r\", \"5x2\"]"));
    }

    #[test]
    fn a_marker_is_a_labelled_event_that_leaves_the_screen_alone() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        recording.push_marker(1.5, "tests \"started\"");
        let cast = recording.to_cast();
        assert_eq!(
            cast.lines().nth(2),
            Some(r#"[1.500000, "m", "tests \"started\""]"#),
            "{cast}"
        );
        assert!((recording.duration_secs() - 1.5).abs() < f64::EPSILON);
        assert_eq!(replay(&cast), vec!["aaaa".to_owned()]);
    }

    #[test]
    fn identical_frames_are_dropped() {
        let mut recording = CastRecording::new(4, 1);
        assert!(recording.push_frame(0.0, &frame(4, 1, "a")));
        assert!(!recording.push_frame(0.5, &frame(4, 1, "a")));
        assert!(!recording.push_frame(1.0, &frame(4, 1, "a")));
        assert_eq!(recording.len(), 1, "a still stretch costs no events");
    }

    #[test]
    fn mark_time_holds_the_final_frame() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        assert!((recording.duration_secs() - 0.0).abs() < f64::EPSILON);

        recording.mark_time(3.0);
        assert!(
            (recording.duration_secs() - 3.0).abs() < f64::EPSILON,
            "the recording should run to the marked time"
        );
        assert_eq!(recording.len(), 2);
    }

    #[test]
    fn mark_time_never_shortens_a_recording() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        recording.push_frame(5.0, &frame(4, 1, "b"));
        recording.mark_time(2.0);
        assert!((recording.duration_secs() - 5.0).abs() < f64::EPSILON);
        assert_eq!(recording.len(), 2, "no event added for an earlier time");
    }

    #[test]
    fn duration_tracks_the_last_event() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.0, &frame(4, 1, "a"));
        recording.push_frame(2.5, &frame(4, 1, "b"));
        assert!((recording.duration_secs() - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn event_lines_are_arrays_with_an_output_marker() {
        let mut recording = CastRecording::new(4, 1);
        recording.push_frame(0.25, &frame(4, 1, "x"));
        let line = recording
            .to_cast()
            .lines()
            .nth(1)
            .expect("event")
            .to_owned();
        assert!(line.starts_with("[0.250000, \"o\", \""), "{line}");
        assert!(line.ends_with("\"]"), "{line}");
    }
}
