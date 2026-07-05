//! Animation frame types for AsciiCanvas.

use std::collections::HashMap;

use super::{AsciiCanvasBuffer, AsciiCell};
use crate::style::{Color, Style};

/// A single frame in an animation sequence.
#[derive(Clone, Debug)]
pub struct AnimationFrame {
    /// The pixel/cell buffer for this frame.
    pub buffer: AsciiCanvasBuffer,
    /// Optional duration in milliseconds (None = static/infinite).
    pub duration_ms: Option<u64>,
    /// Metadata tags for interactive lookup (e.g., "direction" -> "left").
    pub tags: HashMap<String, String>,
}

impl AnimationFrame {
    /// Create a new frame from a buffer.
    pub fn new(buffer: AsciiCanvasBuffer) -> Self {
        Self {
            buffer,
            duration_ms: None,
            tags: HashMap::new(),
        }
    }

    /// Set the duration for this frame.
    pub fn duration(mut self, ms: u64) -> Self {
        self.duration_ms = Some(ms);
        self
    }

    /// Add a metadata tag.
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Get buffer width.
    pub fn width(&self) -> u16 {
        self.buffer.width()
    }

    /// Get buffer height.
    pub fn height(&self) -> u16 {
        self.buffer.height()
    }
}

/// A sequence of frames for animation or interactive display.
#[derive(Clone, Debug)]
pub struct FrameSequence {
    frames: Vec<AnimationFrame>,
    width: u16,
    height: u16,
}

impl FrameSequence {
    /// Create an empty sequence.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            width: 0,
            height: 0,
        }
    }

    /// Add a frame to the sequence.
    pub fn push(&mut self, frame: AnimationFrame) {
        if self.frames.is_empty() {
            self.width = frame.width();
            self.height = frame.height();
        }
        self.frames.push(frame);
    }

    /// Get frame by index.
    pub fn get(&self, idx: usize) -> Option<&AnimationFrame> {
        self.frames.get(idx)
    }

    /// Get mutable frame by index.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut AnimationFrame> {
        self.frames.get_mut(idx)
    }

    /// Find first frame index with matching tag.
    pub fn find_by_tag(&self, key: &str, value: &str) -> Option<usize> {
        self.frames
            .iter()
            .position(|f| f.tags.get(key).map(|v| v == value).unwrap_or(false))
    }

    /// Find all frame indices with matching tag.
    pub fn find_all_by_tag(&self, key: &str, value: &str) -> Vec<usize> {
        self.frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.tags.get(key).map(|v| v == value).unwrap_or(false))
            .map(|(i, _)| i)
            .collect()
    }

    /// Total number of frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Check if sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Get sequence width (from first frame).
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Get sequence height (from first frame).
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Iterate over frames.
    pub fn iter(&self) -> impl Iterator<Item = &AnimationFrame> {
        self.frames.iter()
    }

    /// Collect all unique colors (both foreground and background) used across
    /// every frame, in order of first appearance.
    ///
    /// The returned list contains all distinct colors that appear as either
    /// `fg` or `bg` on any cell.  Use this to discover the palette an asset
    /// uses so you can build a remapping with [`crate::prelude::AsciiCanvas::color_map`].
    ///
    /// **Note:** if the same color value is used in both fg and bg, it appears
    /// only once in the returned list.  When you need to map the same color
    /// differently per channel, use [`collect_fg_colors`](Self::collect_fg_colors)
    /// and [`collect_bg_colors`](Self::collect_bg_colors) instead.
    pub fn collect_colors(&self) -> Vec<Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for frame in &self.frames {
            for cell in frame.buffer.cells() {
                for color in [cell.style.fg, cell.style.bg]
                    .into_iter()
                    .flatten()
                    .map(crate::style::Paint::color)
                {
                    if seen.insert(color) {
                        out.push(color);
                    }
                }
            }
        }
        out
    }

    /// Collect all unique **foreground** colors used across every frame, in
    /// order of first appearance.
    ///
    /// Use with [`crate::prelude::AsciiCanvas::fg_color_map`] when fg and bg share the same
    /// color value but need different replacements.
    pub fn collect_fg_colors(&self) -> Vec<Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for frame in &self.frames {
            for cell in frame.buffer.cells() {
                if let Some(color) = cell.style.fg.map(crate::style::Paint::color)
                    && seen.insert(color)
                {
                    out.push(color);
                }
            }
        }
        out
    }

    /// Collect all unique **background** colors used across every frame, in
    /// order of first appearance.
    ///
    /// Use with [`crate::prelude::AsciiCanvas::bg_color_map`] when fg and bg share the same
    /// color value but need different replacements.
    pub fn collect_bg_colors(&self) -> Vec<Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for frame in &self.frames {
            for cell in frame.buffer.cells() {
                if let Some(color) = cell.style.bg.map(crate::style::Paint::color)
                    && seen.insert(color)
                {
                    out.push(color);
                }
            }
        }
        out
    }

    /// Load from JSON format (like the ghost animation file).
    ///
    /// Expected format:
    /// ```json
    /// {
    ///   "width": 16,
    ///   "height": 10,
    ///   "frames": [
    ///     {
    ///       "content": ["line1", "line2"],
    ///       "foreground": {"0,0": "#FF0000"},
    ///       "tags": {"direction": "center"}
    ///     }
    ///   ]
    /// }
    /// ```
    pub fn from_json(json: &str) -> Result<Self, FrameParseError> {
        parse_json_frames(json)
    }
}

impl Default for FrameSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<AnimationFrame> for FrameSequence {
    fn from_iter<I: IntoIterator<Item = AnimationFrame>>(iter: I) -> Self {
        let mut seq = Self::new();
        for frame in iter {
            seq.push(frame);
        }
        seq
    }
}

/// Errors that can occur when parsing frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameParseError {
    /// The input is not valid JSON.
    InvalidJson,
    /// No frames were found in the JSON data.
    MissingFrames,
    /// A frame has an invalid or missing structure.
    InvalidFrameFormat(String),
    /// A color value could not be parsed.
    InvalidColorFormat(String),
}

impl std::fmt::Display for FrameParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "Invalid JSON format"),
            Self::MissingFrames => write!(f, "No frames found in JSON"),
            Self::InvalidFrameFormat(msg) => write!(f, "Invalid frame format: {}", msg),
            Self::InvalidColorFormat(msg) => write!(f, "Invalid color format: {}", msg),
        }
    }
}

impl std::error::Error for FrameParseError {}

// JSON parsing implementation (minimal, no external deps)
fn parse_json_frames(json: &str) -> Result<FrameSequence, FrameParseError> {
    let mut sequence = FrameSequence::new();

    // Extract frames array
    let frame_blocks = extract_json_blocks(json, "\"frames\"");
    if frame_blocks.is_empty() {
        return Err(FrameParseError::MissingFrames);
    }

    for block in frame_blocks {
        let frame = parse_single_frame(&block)?;
        sequence.push(frame);
    }

    Ok(sequence)
}

fn parse_single_frame(block: &str) -> Result<AnimationFrame, FrameParseError> {
    // Extract content array
    let content = extract_string_array(block, "\"content\"");
    if content.is_empty() {
        return Err(FrameParseError::InvalidFrameFormat(
            "Missing or empty content array".to_string(),
        ));
    }

    let width = content
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let height = content.len() as u16;

    let mut buffer = AsciiCanvasBuffer::new(width, height);

    // Fill buffer with characters
    for (y, line) in content.iter().enumerate() {
        for (x, ch) in line.chars().enumerate() {
            buffer.set(x as u16, y as u16, AsciiCell::new(ch));
        }
    }

    // Apply foreground color map. Supports two formats:
    // 1. Flat:   "foreground": { "0,0": "#FF0000" }
    // 2. Nested: "colors": { "foreground": "{\"0,0\":\"#FF0000\"}" }
    //    (ASCII Motion export format where foreground is a stringified JSON map)
    let fg_map = extract_json_string_value(block, "\"foreground\":").or_else(|| {
        let colors_block = extract_json_object(block, "\"colors\"")?;
        extract_json_string_value(&colors_block, "\"foreground\"")
    });
    if let Some(map_str) = fg_map {
        apply_color_map(&mut buffer, &map_str, width, height, ColorChannel::Fg)?;
    }

    // Apply background color map (same two formats as foreground).
    let bg_map = extract_json_string_value(block, "\"background\":").or_else(|| {
        let colors_block = extract_json_object(block, "\"colors\"")?;
        extract_json_string_value(&colors_block, "\"background\"")
    });
    if let Some(map_str) = bg_map {
        apply_color_map(&mut buffer, &map_str, width, height, ColorChannel::Bg)?;
    }

    let mut frame = AnimationFrame::new(buffer);

    // Parse tags if present
    if let Some(tags_str) = extract_json_object(block, "\"tags\"") {
        frame.tags = parse_simple_json_object(&tags_str);
    }

    // Parse title as a fallback tag source
    if let Some(title) = extract_json_value(block, "\"title\"") {
        frame.tags.entry("title".to_string()).or_insert(title);
    }

    // Parse duration if present
    if let Some(dur_str) = extract_json_value(block, "\"duration\"")
        && let Ok(ms) = dur_str.parse::<u64>()
    {
        frame.duration_ms = Some(ms);
    }

    Ok(frame)
}

/// Which color channel to write when applying a JSON color map.
enum ColorChannel {
    Fg,
    Bg,
}

fn apply_color_map(
    buffer: &mut AsciiCanvasBuffer,
    map: &str,
    width: u16,
    height: u16,
    channel: ColorChannel,
) -> Result<(), FrameParseError> {
    let mut iter = map.chars().peekable();
    while let Some(key) = read_json_string(&mut iter) {
        let Some(value) = read_json_string(&mut iter) else {
            break;
        };
        if let Some(color) = Color::try_hex(value.trim()) {
            if let Some((x, y)) = parse_coord(&key)
                && x < width
                && y < height
            {
                let idx = (y as usize).saturating_mul(width as usize) + x as usize;
                if let Some(cell) = buffer.cells().get(idx).copied() {
                    let new_style = match channel {
                        ColorChannel::Fg => Style {
                            fg: Some(color.into()),
                            ..cell.style
                        },
                        ColorChannel::Bg => Style {
                            bg: Some(color.into()),
                            ..cell.style
                        },
                    };
                    buffer.set(
                        x,
                        y,
                        AsciiCell {
                            style: new_style,
                            ..cell
                        },
                    );
                }
            }
        } else {
            return Err(FrameParseError::InvalidColorFormat(value));
        }
    }
    Ok(())
}

fn parse_coord(input: &str) -> Option<(u16, u16)> {
    let mut parts = input.split(',');
    let x = parts.next()?.trim().parse::<u16>().ok()?;
    let y = parts.next()?.trim().parse::<u16>().ok()?;
    Some((x, y))
}

fn read_json_string(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    while let Some(&ch) = iter.peek() {
        iter.next();
        if ch == '"' {
            break;
        }
    }

    let mut out = String::new();
    let mut escape = false;
    while let Some(ch) = iter.next() {
        if escape {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = iter.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u16::from_str_radix(&hex, 16)
                        && let Some(c) = char::from_u32(code as u32)
                    {
                        out.push(c);
                    }
                }
                _ => out.push(ch),
            }
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn extract_json_blocks(haystack: &str, key: &str) -> Vec<String> {
    let idx = haystack.find(key);
    if idx.is_none() {
        return Vec::new();
    }
    let rest = &haystack[idx.expect("idx is Some; is_none() guard returned early above")..];
    let array_start = rest.find('[').unwrap_or(0);
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut block = String::new();
    let mut blocks = Vec::new();
    for ch in rest[array_start..].chars() {
        if escape {
            block.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            block.push(ch);
            escape = true;
            continue;
        }
        if ch == '"' {
            block.push(ch);
            in_string = !in_string;
            continue;
        }
        if in_string {
            block.push(ch);
            continue;
        }
        if ch == '{' {
            depth += 1;
            block.push(ch);
            continue;
        }
        if ch == '}' {
            depth -= 1;
            block.push(ch);
            if depth == 0 {
                blocks.push(block.clone());
                block.clear();
            }
            continue;
        }
        if depth > 0 {
            block.push(ch);
        }
        if depth == 0 && ch == ']' {
            break;
        }
    }
    blocks
}

fn extract_string_array(haystack: &str, key: &str) -> Vec<String> {
    let idx = haystack.find(key);
    if idx.is_none() {
        return Vec::new();
    }
    let idx = idx.unwrap();
    let rest = &haystack[idx + key.len()..];
    let start = rest.find('[').unwrap_or(0);
    let rest = &rest[start + 1..];
    let end = rest.find(']').unwrap_or(rest.len());
    let list = &rest[..end];

    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_string = false;
    let mut escape = false;
    for ch in list.chars() {
        if escape {
            cur.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            if in_string {
                out.push(cur.clone());
                cur.clear();
                in_string = false;
            } else {
                in_string = true;
            }
            continue;
        }
        if in_string {
            cur.push(ch);
        }
    }
    out
}

fn extract_json_string_value(haystack: &str, key: &str) -> Option<String> {
    let idx = haystack.find(key)?;
    let rest = &haystack[idx + key.len()..];
    let mut start_byte = None;
    for (i, ch) in rest.char_indices() {
        if ch == '"' {
            start_byte = Some(i + ch.len_utf8());
            break;
        }
    }
    let start = start_byte?;
    let mut iter = rest[start..].chars().peekable();
    let mut out = String::new();
    let mut escape = false;
    while let Some(ch) = iter.next() {
        if escape {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = iter.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u16::from_str_radix(&hex, 16)
                        && let Some(c) = char::from_u32(code as u32)
                    {
                        out.push(c);
                    }
                }
                _ => out.push(ch),
            }
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    Some(out)
}

fn extract_json_object(haystack: &str, key: &str) -> Option<String> {
    let idx = haystack.find(key)?;
    let rest = &haystack[idx + key.len()..];
    let start = rest.find('{')?;
    let rest = &rest[start..];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut result = String::new();
    for ch in rest.chars() {
        if escape {
            result.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            result.push(ch);
            escape = true;
            continue;
        }
        if ch == '"' {
            result.push(ch);
            in_string = !in_string;
            continue;
        }
        if in_string {
            result.push(ch);
            continue;
        }
        if ch == '{' {
            depth += 1;
            result.push(ch);
            continue;
        }
        if ch == '}' {
            depth -= 1;
            result.push(ch);
            if depth == 0 {
                break;
            }
            continue;
        }
        result.push(ch);
    }
    Some(result)
}

fn extract_json_value(haystack: &str, key: &str) -> Option<String> {
    let idx = haystack.find(key)?;
    let rest = &haystack[idx + key.len()..];
    // Skip whitespace and colon
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let rest = rest.trim_start();

    // Extract value (number, string, or boolean)
    if let Some(inner) = rest.strip_prefix('"') {
        // String value
        let iter = inner.chars();
        let mut out = String::new();
        let mut escape = false;
        for ch in iter {
            if escape {
                out.push(ch);
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                break;
            }
            out.push(ch);
        }
        Some(out)
    } else {
        // Number or boolean - read until comma, brace, or whitespace
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c == ']' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn parse_simple_json_object(json: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut iter = json.chars().peekable();

    // Skip opening brace
    while let Some(&ch) = iter.peek() {
        if ch == '{' {
            iter.next();
            break;
        }
        iter.next();
    }

    loop {
        // Skip whitespace and commas
        while let Some(&ch) = iter.peek() {
            if ch == '}' {
                return map;
            }
            if !ch.is_whitespace() && ch != ',' {
                break;
            }
            iter.next();
        }

        // Read key
        let key = match read_json_string(&mut iter) {
            Some(k) => k,
            None => break,
        };

        // Skip colon
        while let Some(&ch) = iter.peek() {
            iter.next();
            if ch == ':' {
                break;
            }
        }

        // Read value
        let value = match read_json_string(&mut iter) {
            Some(v) => v,
            None => break,
        };

        map.insert(key, value);
    }

    map
}

/// Builder for constructing frame sequences programmatically.
pub struct FrameSequenceBuilder {
    sequence: FrameSequence,
}

impl FrameSequenceBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            sequence: FrameSequence::new(),
        }
    }

    /// Add a frame from lines of text.
    pub fn add_frame(mut self, lines: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let lines: Vec<String> = lines.into_iter().map(|s| s.as_ref().to_string()).collect();

        let width = lines.iter().map(|l| l.len()).max().unwrap_or(0) as u16;
        let height = lines.len() as u16;
        let mut buffer = AsciiCanvasBuffer::new(width, height);

        for (y, line) in lines.iter().enumerate() {
            for (x, ch) in line.chars().enumerate() {
                buffer.set(x as u16, y as u16, AsciiCell::new(ch));
            }
        }

        self.sequence.push(AnimationFrame::new(buffer));
        self
    }

    /// Add an existing frame.
    pub fn push_frame(mut self, frame: AnimationFrame) -> Self {
        self.sequence.push(frame);
        self
    }

    /// Build the sequence.
    pub fn build(self) -> FrameSequence {
        self.sequence
    }
}

impl Default for FrameSequenceBuilder {
    fn default() -> Self {
        Self::new()
    }
}
