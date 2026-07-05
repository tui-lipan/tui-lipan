use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, DataChange, EventKind, ModifyKind};
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;

use crate::style::presets::preset_by_name;
use crate::style::{
    Color, DiffPalette, DocumentPalette, DocumentViewPalette, FileIconPalette, GitStatusPalette,
    HexAreaPalette, InputPalette, Paint, ScrollbarPalette, SplitterPalette, StatusPalette, Style,
    SurfacePalette, SyntaxPalette, TerminalPalette, TextAreaPalette, Theme,
};

const SUPPORTED_PRESET_NAMES: &[&str] = &[
    "one_dark",
    "dracula",
    "nord",
    "gruvbox",
    "catppuccin",
    "ansi",
    "tokyo_night",
    "solarized_dark",
    "monokai",
];

fn theme_reload_error(message: impl Into<String>) -> crate::Error {
    crate::Error::ThemeReload {
        message: message.into(),
    }
}

fn normalize_color_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn parse_function_args<'a>(value: &'a str, function_name: &str) -> Option<&'a str> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    if !lower.starts_with(function_name) {
        return None;
    }
    let rest = value[function_name.len()..].trim_start();
    if !(rest.starts_with('(') && rest.ends_with(')')) {
        return None;
    }
    Some(rest[1..rest.len() - 1].trim())
}

/// TOML/serialized color representation used for theme reloading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
struct ColorSpec(Color);

impl From<ColorSpec> for Color {
    fn from(value: ColorSpec) -> Self {
        value.0
    }
}

impl From<ColorSpec> for Paint {
    fn from(value: ColorSpec) -> Self {
        Self::Solid(value.0)
    }
}

impl TryFrom<String> for ColorSpec {
    type Error = crate::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for ColorSpec {
    type Err = crate::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err(theme_reload_error(
                "invalid color ``; expected hex, ANSI name, indexed(<0-255>), or rgb(r,g,b)",
            ));
        }

        if let Some(color) = Color::try_hex(value) {
            return Ok(Self(color));
        }

        if Paint::try_hex(value).is_some() {
            return Err(theme_reload_error(format!(
                "invalid color `{value}`; alpha hex is only supported in paint-capable style fields"
            )));
        }

        if parse_function_args(value, "rgba").is_some() {
            return Err(theme_reload_error(format!(
                "invalid color `{value}`; rgba() is only supported in paint-capable style fields"
            )));
        }

        let normalized = normalize_color_name(value);
        let named = match normalized.as_str() {
            "reset" => Some(Color::Reset),
            "backdrop" => Some(Color::Backdrop),
            "transparent" => Some(Color::Transparent),
            "black" => Some(Color::Black),
            "red" => Some(Color::Red),
            "green" => Some(Color::Green),
            "yellow" => Some(Color::Yellow),
            "blue" => Some(Color::Blue),
            "magenta" => Some(Color::Magenta),
            "cyan" => Some(Color::Cyan),
            "gray" | "grey" => Some(Color::Gray),
            "darkgray" | "darkgrey" => Some(Color::DarkGray),
            "lightred" => Some(Color::LightRed),
            "lightgreen" => Some(Color::LightGreen),
            "lightyellow" => Some(Color::LightYellow),
            "lightblue" => Some(Color::LightBlue),
            "lightmagenta" => Some(Color::LightMagenta),
            "lightcyan" => Some(Color::LightCyan),
            "white" => Some(Color::White),
            _ => None,
        };
        if let Some(color) = named {
            return Ok(Self(color));
        }

        if let Some(inner) = parse_function_args(value, "indexed") {
            let parsed = inner.parse::<u8>().map_err(|_| {
                theme_reload_error(format!(
                    "invalid indexed color `{value}`; expected indexed(<0-255>)"
                ))
            })?;
            return Ok(Self(Color::Indexed(parsed)));
        }

        if let Some(inner) = parse_function_args(value, "rgb") {
            let channels: Vec<_> = inner.split(',').map(str::trim).collect();
            if channels.len() != 3 {
                return Err(theme_reload_error(format!(
                    "invalid rgb color `{value}`; expected rgb(r,g,b)"
                )));
            }
            let r = channels[0].parse::<u8>().map_err(|_| {
                theme_reload_error(format!(
                    "invalid rgb color `{value}`; red channel must be 0-255"
                ))
            })?;
            let g = channels[1].parse::<u8>().map_err(|_| {
                theme_reload_error(format!(
                    "invalid rgb color `{value}`; green channel must be 0-255"
                ))
            })?;
            let b = channels[2].parse::<u8>().map_err(|_| {
                theme_reload_error(format!(
                    "invalid rgb color `{value}`; blue channel must be 0-255"
                ))
            })?;
            return Ok(Self(Color::Rgb(r, g, b)));
        }

        Err(theme_reload_error(format!(
            "invalid color `{value}`; expected hex, ANSI name, indexed(<0-255>), or rgb(r,g,b)"
        )))
    }
}

/// TOML/serialized paint representation used for style-channel theme reloading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
struct PaintSpec(Paint);

impl From<PaintSpec> for Paint {
    fn from(value: PaintSpec) -> Self {
        value.0
    }
}

impl TryFrom<String> for PaintSpec {
    type Error = crate::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for PaintSpec {
    type Err = crate::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err(theme_reload_error(
                "invalid paint ``; expected hex, ANSI name, indexed(<0-255>), rgb(r,g,b), or rgba(r,g,b,a)",
            ));
        }

        if let Some(paint) = Paint::try_hex(value) {
            return Ok(Self(paint));
        }

        if let Some(inner) = parse_function_args(value, "rgba") {
            let channels: Vec<_> = inner.split(',').map(str::trim).collect();
            if channels.len() != 4 {
                return Err(theme_reload_error(format!(
                    "invalid rgba paint `{value}`; expected rgba(r,g,b,a)"
                )));
            }
            let r = parse_rgba_rgb_channel(value, channels[0], "red")?;
            let g = parse_rgba_rgb_channel(value, channels[1], "green")?;
            let b = parse_rgba_rgb_channel(value, channels[2], "blue")?;
            let alpha = parse_rgba_alpha(value, channels[3])?;
            return Ok(Self(Paint::rgba(r, g, b, alpha)));
        }

        if let Ok(color) = ColorSpec::from_str(value) {
            return Ok(Self(Paint::Solid(color.into())));
        }

        Err(theme_reload_error(format!(
            "invalid paint `{value}`; expected hex, ANSI name, indexed(<0-255>), rgb(r,g,b), or rgba(r,g,b,a)"
        )))
    }
}

fn parse_rgba_rgb_channel(value: &str, channel: &str, name: &str) -> Result<u8, crate::Error> {
    channel.parse::<u8>().map_err(|_| {
        theme_reload_error(format!(
            "invalid rgba paint `{value}`; {name} channel must be 0-255"
        ))
    })
}

fn parse_rgba_alpha(value: &str, alpha: &str) -> Result<u8, crate::Error> {
    if alpha.contains('.') {
        let parsed = alpha.parse::<f32>().map_err(|_| {
            theme_reload_error(format!(
                "invalid rgba paint `{value}`; alpha must be 0-255 or 0.0-1.0"
            ))
        })?;
        if !(0.0..=1.0).contains(&parsed) {
            return Err(theme_reload_error(format!(
                "invalid rgba paint `{value}`; alpha float must be between 0.0 and 1.0"
            )));
        }
        Ok((parsed * 255.0).round() as u8)
    } else {
        let parsed = alpha.parse::<u16>().map_err(|_| {
            theme_reload_error(format!(
                "invalid rgba paint `{value}`; alpha must be 0-255 or 0.0-1.0"
            ))
        })?;
        if parsed > 255 {
            return Err(theme_reload_error(format!(
                "invalid rgba paint `{value}`; alpha integer must be between 0 and 255"
            )));
        }
        Ok(parsed as u8)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct TomlTint {
    color: ColorSpec,
    alpha: f32,
}

/// TOML patch of [`Style`].
///
/// Keep fields in sync with `Style` so new style channels are reloadable.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlStyle {
    fg: Option<PaintSpec>,
    bg: Option<PaintSpec>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    reverse: Option<bool>,
    strikethrough: Option<bool>,
    underline_color: Option<PaintSpec>,
    dim_amount: Option<f32>,
    tint: Option<TomlTint>,
}

impl TomlStyle {
    fn apply(self, mut base: Style) -> Style {
        if let Some(fg) = self.fg {
            base.fg = Some(fg.into());
        }
        if let Some(bg) = self.bg {
            base.bg = Some(bg.into());
        }
        if let Some(bold) = self.bold {
            base.bold = Some(bold);
        }
        if let Some(dim) = self.dim {
            base.dim = Some(dim);
        }
        if let Some(italic) = self.italic {
            base.italic = Some(italic);
        }
        if let Some(underline) = self.underline {
            base.underline = Some(underline);
        }
        if let Some(reverse) = self.reverse {
            base.reverse = Some(reverse);
        }
        if let Some(strikethrough) = self.strikethrough {
            base.strikethrough = Some(strikethrough);
        }
        if let Some(underline_color) = self.underline_color {
            base.underline_color = Some(underline_color.into());
        }
        if let Some(dim_amount) = self.dim_amount {
            base.dim_amount = Some(dim_amount.clamp(0.0, 1.0));
        }
        if let Some(tint) = self.tint {
            base.tint = Some((tint.color.into(), tint.alpha.clamp(0.0, 1.0)));
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlSurfacePalette {
    panel: Option<ColorSpec>,
    element: Option<ColorSpec>,
    menu: Option<ColorSpec>,
    backdrop: Option<ColorSpec>,
}

impl TomlSurfacePalette {
    fn apply(self, mut base: SurfacePalette) -> SurfacePalette {
        if let Some(color) = self.panel {
            base.panel = color.into();
        }
        if let Some(color) = self.element {
            base.element = color.into();
        }
        if let Some(color) = self.menu {
            base.menu = color.into();
        }
        if let Some(color) = self.backdrop {
            base.backdrop = color.into();
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlStatusPalette {
    success: Option<ColorSpec>,
    warning: Option<ColorSpec>,
    error: Option<ColorSpec>,
    info: Option<ColorSpec>,
}

impl TomlStatusPalette {
    fn apply(self, mut base: StatusPalette) -> StatusPalette {
        if let Some(color) = self.success {
            base.success = color.into();
        }
        if let Some(color) = self.warning {
            base.warning = color.into();
        }
        if let Some(color) = self.error {
            base.error = color.into();
        }
        if let Some(color) = self.info {
            base.info = color.into();
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlFileIconPalette {
    azure: Option<ColorSpec>,
    blue: Option<ColorSpec>,
    cyan: Option<ColorSpec>,
    green: Option<ColorSpec>,
    grey: Option<ColorSpec>,
    orange: Option<ColorSpec>,
    purple: Option<ColorSpec>,
    red: Option<ColorSpec>,
    yellow: Option<ColorSpec>,
}

impl TomlFileIconPalette {
    fn apply(self, mut base: FileIconPalette) -> FileIconPalette {
        if let Some(color) = self.azure {
            base.azure = color.into();
        }
        if let Some(color) = self.blue {
            base.blue = color.into();
        }
        if let Some(color) = self.cyan {
            base.cyan = color.into();
        }
        if let Some(color) = self.green {
            base.green = color.into();
        }
        if let Some(color) = self.grey {
            base.grey = color.into();
        }
        if let Some(color) = self.orange {
            base.orange = color.into();
        }
        if let Some(color) = self.purple {
            base.purple = color.into();
        }
        if let Some(color) = self.red {
            base.red = color.into();
        }
        if let Some(color) = self.yellow {
            base.yellow = color.into();
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlGitStatusPalette {
    modified: Option<ColorSpec>,
    added: Option<ColorSpec>,
    deleted: Option<ColorSpec>,
    renamed: Option<ColorSpec>,
    untracked: Option<ColorSpec>,
    conflicted: Option<ColorSpec>,
}

impl TomlGitStatusPalette {
    fn apply(self, mut base: GitStatusPalette) -> GitStatusPalette {
        if let Some(color) = self.modified {
            base.modified = color.into();
        }
        if let Some(color) = self.added {
            base.added = color.into();
        }
        if let Some(color) = self.deleted {
            base.deleted = color.into();
        }
        if let Some(color) = self.renamed {
            base.renamed = color.into();
        }
        if let Some(color) = self.untracked {
            base.untracked = color.into();
        }
        if let Some(color) = self.conflicted {
            base.conflicted = color.into();
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlDiffPalette {
    context: Option<TomlStyle>,
    added: Option<TomlStyle>,
    removed: Option<TomlStyle>,
    empty: Option<TomlStyle>,
    added_word: Option<TomlStyle>,
    removed_word: Option<TomlStyle>,
    added_marker: Option<TomlStyle>,
    removed_marker: Option<TomlStyle>,
    context_line_number: Option<TomlStyle>,
    added_line_number: Option<TomlStyle>,
    removed_line_number: Option<TomlStyle>,
    context_separator_style: Option<TomlStyle>,
    patch_header: Option<TomlStyle>,
}

impl TomlDiffPalette {
    fn apply(self, mut base: DiffPalette) -> DiffPalette {
        if let Some(style) = self.context {
            base.context = style.apply(base.context);
        }
        if let Some(style) = self.added {
            base.added = style.apply(base.added);
        }
        if let Some(style) = self.removed {
            base.removed = style.apply(base.removed);
        }
        if let Some(style) = self.empty {
            base.empty = style.apply(base.empty);
        }
        if let Some(style) = self.added_word {
            base.added_word = style.apply(base.added_word);
        }
        if let Some(style) = self.removed_word {
            base.removed_word = style.apply(base.removed_word);
        }
        if let Some(style) = self.added_marker {
            base.added_marker = style.apply(base.added_marker);
        }
        if let Some(style) = self.removed_marker {
            base.removed_marker = style.apply(base.removed_marker);
        }
        if let Some(style) = self.context_line_number {
            base.context_line_number = style.apply(base.context_line_number);
        }
        if let Some(style) = self.added_line_number {
            base.added_line_number = style.apply(base.added_line_number);
        }
        if let Some(style) = self.removed_line_number {
            base.removed_line_number = style.apply(base.removed_line_number);
        }
        if let Some(style) = self.context_separator_style {
            base.context_separator_style = style.apply(base.context_separator_style);
        }
        if let Some(style) = self.patch_header {
            base.patch_header = style.apply(base.patch_header);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlDocumentPalette {
    heading_styles: Option<[TomlStyle; 6]>,
    code_inline: Option<TomlStyle>,
    code_block: Option<TomlStyle>,
    emphasis: Option<TomlStyle>,
    strong: Option<TomlStyle>,
    strikethrough: Option<TomlStyle>,
    link: Option<TomlStyle>,
    blockquote_bar: Option<TomlStyle>,
    table_border: Option<TomlStyle>,
    table_header: Option<TomlStyle>,
    hr: Option<TomlStyle>,
    list_item: Option<TomlStyle>,
    list_enumeration: Option<TomlStyle>,
    diagram_node_fill_style: Option<TomlStyle>,
    diagram_node_border_style: Option<TomlStyle>,
    diagram_node_label_style: Option<TomlStyle>,
    diagram_edge_style: Option<TomlStyle>,
    diagram_muted_style: Option<TomlStyle>,
}

impl TomlDocumentPalette {
    fn apply(self, mut base: DocumentPalette) -> DocumentPalette {
        if let Some(styles) = self.heading_styles {
            for (i, style) in styles.into_iter().enumerate() {
                base.heading_styles[i] = style.apply(base.heading_styles[i]);
            }
        }
        if let Some(style) = self.code_inline {
            base.code_inline = style.apply(base.code_inline);
        }
        if let Some(style) = self.code_block {
            base.code_block = style.apply(base.code_block);
        }
        if let Some(style) = self.emphasis {
            base.emphasis = style.apply(base.emphasis);
        }
        if let Some(style) = self.strong {
            base.strong = style.apply(base.strong);
        }
        if let Some(style) = self.strikethrough {
            base.strikethrough = style.apply(base.strikethrough);
        }
        if let Some(style) = self.link {
            base.link = style.apply(base.link);
        }
        if let Some(style) = self.blockquote_bar {
            base.blockquote_bar = style.apply(base.blockquote_bar);
        }
        if let Some(style) = self.table_border {
            base.table_border = style.apply(base.table_border);
        }
        if let Some(style) = self.table_header {
            base.table_header = style.apply(base.table_header);
        }
        if let Some(style) = self.hr {
            base.hr = style.apply(base.hr);
        }
        if let Some(style) = self.list_item {
            base.list_item = style.apply(base.list_item);
        }
        if let Some(style) = self.list_enumeration {
            base.list_enumeration = style.apply(base.list_enumeration);
        }
        if let Some(style) = self.diagram_node_fill_style {
            base.diagram_node_fill_style = style.apply(base.diagram_node_fill_style);
        }
        if let Some(style) = self.diagram_node_border_style {
            base.diagram_node_border_style = style.apply(base.diagram_node_border_style);
        }
        if let Some(style) = self.diagram_node_label_style {
            base.diagram_node_label_style = style.apply(base.diagram_node_label_style);
        }
        if let Some(style) = self.diagram_edge_style {
            base.diagram_edge_style = style.apply(base.diagram_edge_style);
        }
        if let Some(style) = self.diagram_muted_style {
            base.diagram_muted_style = style.apply(base.diagram_muted_style);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlSyntaxPalette {
    comment: Option<TomlStyle>,
    keyword: Option<TomlStyle>,
    string: Option<TomlStyle>,
    number: Option<TomlStyle>,
    constant: Option<TomlStyle>,
    function: Option<TomlStyle>,
    builtin: Option<TomlStyle>,
    type_name: Option<TomlStyle>,
    variable: Option<TomlStyle>,
    parameter: Option<TomlStyle>,
    operator: Option<TomlStyle>,
}

impl TomlSyntaxPalette {
    fn apply(self, mut base: SyntaxPalette) -> SyntaxPalette {
        if let Some(style) = self.comment {
            base.comment = style.apply(base.comment);
        }
        if let Some(style) = self.keyword {
            base.keyword = style.apply(base.keyword);
        }
        if let Some(style) = self.string {
            base.string = style.apply(base.string);
        }
        if let Some(style) = self.number {
            base.number = style.apply(base.number);
        }
        if let Some(style) = self.constant {
            base.constant = style.apply(base.constant);
        }
        if let Some(style) = self.function {
            base.function = style.apply(base.function);
        }
        if let Some(style) = self.builtin {
            base.builtin = style.apply(base.builtin);
        }
        if let Some(style) = self.type_name {
            base.type_name = style.apply(base.type_name);
        }
        if let Some(style) = self.variable {
            base.variable = style.apply(base.variable);
        }
        if let Some(style) = self.parameter {
            base.parameter = style.apply(base.parameter);
        }
        if let Some(style) = self.operator {
            base.operator = style.apply(base.operator);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlInputPalette {
    focus: Option<TomlStyle>,
}

impl TomlInputPalette {
    fn apply(self, mut base: InputPalette) -> InputPalette {
        if let Some(style) = self.focus {
            base.focus = style.apply(base.focus);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlTextAreaPalette {
    focus: Option<TomlStyle>,
}

impl TomlTextAreaPalette {
    fn apply(self, mut base: TextAreaPalette) -> TextAreaPalette {
        if let Some(style) = self.focus {
            base.focus = style.apply(base.focus);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlDocumentViewPalette {
    focus: Option<TomlStyle>,
}

impl TomlDocumentViewPalette {
    fn apply(self, mut base: DocumentViewPalette) -> DocumentViewPalette {
        if let Some(style) = self.focus {
            base.focus = style.apply(base.focus);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlHexAreaPalette {
    focus: Option<TomlStyle>,
    cursor: Option<TomlStyle>,
}

impl TomlHexAreaPalette {
    fn apply(self, mut base: HexAreaPalette) -> HexAreaPalette {
        if let Some(style) = self.focus {
            base.focus = style.apply(base.focus);
        }
        if let Some(style) = self.cursor {
            base.cursor = style.apply(base.cursor);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlTerminalPalette {
    focus: Option<TomlStyle>,
}

impl TomlTerminalPalette {
    fn apply(self, mut base: TerminalPalette) -> TerminalPalette {
        if let Some(style) = self.focus {
            base.focus = style.apply(base.focus);
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlScrollbarPalette {
    track: Option<ColorSpec>,
    thumb: Option<ColorSpec>,
    thumb_focus: Option<ColorSpec>,
}

impl TomlScrollbarPalette {
    fn apply(self, mut base: ScrollbarPalette) -> ScrollbarPalette {
        if let Some(color) = self.track {
            base.track = Some(color.into());
        }
        if let Some(color) = self.thumb {
            base.thumb = color.into();
        }
        if let Some(color) = self.thumb_focus {
            base.thumb_focus = Some(color.into());
        }
        base
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct TomlSplitterPalette {
    hover: Option<ColorSpec>,
    active: Option<ColorSpec>,
}

impl TomlSplitterPalette {
    fn apply(self, mut base: SplitterPalette) -> SplitterPalette {
        if let Some(color) = self.hover {
            base.hover = color.into();
        }
        if let Some(color) = self.active {
            base.active = color.into();
        }
        base
    }
}

/// TOML overlay definition for theme reload.
#[derive(Clone, Debug, Default, Deserialize)]
struct TomlTheme {
    extends: Option<String>,

    primary: Option<TomlStyle>,
    accent: Option<TomlStyle>,
    selection: Option<TomlStyle>,
    text_selection: Option<TomlStyle>,
    focus: Option<TomlStyle>,
    hover: Option<TomlStyle>,
    border: Option<TomlStyle>,
    muted: Option<TomlStyle>,

    surface: Option<TomlSurfacePalette>,
    status: Option<TomlStatusPalette>,
    border_active: Option<ColorSpec>,

    file_icons: Option<TomlFileIconPalette>,
    git_status: Option<TomlGitStatusPalette>,
    diff: Option<TomlDiffPalette>,
    document: Option<TomlDocumentPalette>,
    syntax: Option<TomlSyntaxPalette>,
    input: Option<TomlInputPalette>,
    text_area: Option<TomlTextAreaPalette>,
    document_view: Option<TomlDocumentViewPalette>,
    hex_area: Option<TomlHexAreaPalette>,
    terminal: Option<TomlTerminalPalette>,
    scrollbar: Option<TomlScrollbarPalette>,
    splitter: Option<TomlSplitterPalette>,
}

impl TomlTheme {
    /// Convert a TOML theme overlay into a concrete [`Theme`].
    ///
    /// Precedence: explicit fields in TOML > `extends` preset > `fallback`.
    fn into_theme(self, fallback: Theme) -> crate::Result<Theme> {
        let mut theme = if let Some(extends) = self.extends.as_deref() {
            preset_by_name(extends).ok_or_else(|| {
                theme_reload_error(format!(
                    "unknown theme preset `{extends}`; supported presets: {}",
                    SUPPORTED_PRESET_NAMES.join(", ")
                ))
            })?
        } else {
            fallback
        };

        if let Some(style) = self.primary {
            theme.primary = style.apply(theme.primary);
        }
        if let Some(style) = self.accent {
            theme.accent = style.apply(theme.accent);
        }
        if let Some(style) = self.selection {
            theme.selection = style.apply(theme.selection);
        }
        if let Some(style) = self.text_selection {
            theme.text_selection = style.apply(theme.text_selection);
        }
        if let Some(style) = self.focus {
            theme.focus = style.apply(theme.focus);
        }
        if let Some(style) = self.hover {
            theme.hover = style.apply(theme.hover);
        }
        if let Some(style) = self.border {
            theme.border = style.apply(theme.border);
        }
        if let Some(style) = self.muted {
            theme.muted = style.apply(theme.muted);
        }

        if let Some(surface) = self.surface {
            theme.surface = surface.apply(theme.surface);
        }
        if let Some(status) = self.status {
            theme.status = status.apply(theme.status);
        }
        if let Some(color) = self.border_active {
            theme.border_active = color.into();
        }

        if let Some(file_icons) = self.file_icons {
            theme.file_icons = file_icons.apply(theme.file_icons);
        }
        if let Some(git_status) = self.git_status {
            theme.git_status = git_status.apply(theme.git_status);
        }
        if let Some(diff) = self.diff {
            theme.diff = diff.apply(theme.diff);
        }
        if let Some(document) = self.document {
            theme.document = document.apply(theme.document);
        }
        if let Some(syntax) = self.syntax {
            theme.syntax = syntax.apply(theme.syntax);
        }
        if let Some(input) = self.input {
            theme.input = input.apply(theme.input);
        }
        if let Some(text_area) = self.text_area {
            theme.text_area = text_area.apply(theme.text_area);
        }
        if let Some(document_view) = self.document_view {
            theme.document_view = document_view.apply(theme.document_view);
        }
        if let Some(hex_area) = self.hex_area {
            theme.hex_area = hex_area.apply(theme.hex_area);
        }
        if let Some(terminal) = self.terminal {
            theme.terminal = terminal.apply(theme.terminal);
        }
        if let Some(scrollbar) = self.scrollbar {
            theme.scrollbar = scrollbar.apply(theme.scrollbar);
        }
        if let Some(splitter) = self.splitter {
            theme.splitter = splitter.apply(theme.splitter);
        }

        Ok(theme)
    }
}

/// Load a theme from TOML and overlay it on the provided fallback.
pub fn load_theme_from_toml(path: &Path, fallback: Theme) -> crate::Result<Theme> {
    let text = std::fs::read_to_string(path)?;
    let overlay = toml::from_str::<TomlTheme>(&text).map_err(|err| {
        theme_reload_error(format!(
            "failed to parse theme TOML `{}`: {err}",
            path.display()
        ))
    })?;
    overlay.into_theme(fallback)
}

fn is_relevant_kind(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Any
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Modify(ModifyKind::Data(DataChange::Any))
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Create(CreateKind::Any)
            | EventKind::Create(_)
    )
}

fn event_matches_target(event: &notify::Event, target: &Path) -> bool {
    if event.paths.iter().any(|p| p == target) {
        return true;
    }

    let Some(target_file_name) = target.file_name() else {
        return false;
    };

    event.paths.iter().any(|path| {
        path.file_name()
            .is_some_and(|file_name| file_name == target_file_name)
    })
}

/// File watcher that emits updated [`Theme`] values on relevant file changes.
pub struct ThemeWatcher {
    _watcher: notify::RecommendedWatcher,
    event_rx: Receiver<notify::Result<notify::Event>>,
    path: PathBuf,
    base: Theme,
    last_emit: Cell<Option<Instant>>,
    pending_themes: RefCell<VecDeque<Theme>>,
    pending_errors: RefCell<VecDeque<String>>,
}

impl ThemeWatcher {
    /// Start watching a TOML theme file.
    pub fn new(path: impl Into<PathBuf>, base: Theme) -> crate::Result<Self> {
        let path = path.into();
        let watch_target = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.clone());

        let (event_tx, event_rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(event_tx).map_err(|err| {
            theme_reload_error(format!("failed to initialize theme watcher: {err}"))
        })?;

        watcher
            .watch(&watch_target, RecursiveMode::NonRecursive)
            .map_err(|err| {
                theme_reload_error(format!(
                    "failed to watch `{}` for theme reload: {err}",
                    watch_target.display()
                ))
            })?;

        Ok(Self {
            _watcher: watcher,
            event_rx,
            path,
            base,
            last_emit: Cell::new(None),
            pending_themes: RefCell::new(VecDeque::new()),
            pending_errors: RefCell::new(VecDeque::new()),
        })
    }

    fn drain_events(&self) {
        while let Ok(event_result) = self.event_rx.try_recv() {
            let event = match event_result {
                Ok(event) => event,
                Err(err) => {
                    crate::debug::internal_log!("[tui-lipan] theme-reload watcher error: {err}");
                    self.pending_errors
                        .borrow_mut()
                        .push_back(format!("theme watcher error: {err}"));
                    continue;
                }
            };

            if !is_relevant_kind(&event.kind) || !event_matches_target(&event, &self.path) {
                continue;
            }

            let now = Instant::now();
            if let Some(last) = self.last_emit.get()
                && now.duration_since(last) < Duration::from_millis(150)
            {
                continue;
            }
            self.last_emit.set(Some(now));

            match load_theme_from_toml(&self.path, self.base.clone()) {
                Ok(theme) => {
                    self.pending_themes.borrow_mut().push_back(theme);
                }
                Err(err) => {
                    crate::debug::internal_log!(
                        "[tui-lipan] theme-reload parse error for `{}`: {err}",
                        self.path.display()
                    );
                    self.pending_errors.borrow_mut().push_back(format!(
                        "theme parse error for `{}`: {err}",
                        self.path.display()
                    ));
                }
            }
        }
    }

    /// Try receiving a non-fatal watcher/parse error message without blocking.
    pub fn try_recv_error(&self) -> Option<String> {
        self.drain_events();
        self.pending_errors.borrow_mut().pop_front()
    }

    /// Try receiving the latest reloaded theme without blocking.
    pub fn try_recv(&self) -> Option<Theme> {
        self.drain_events();
        self.pending_themes.borrow_mut().pop_front()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tui-lipan-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn color_spec_parses_hex_and_named() {
        assert_eq!(
            ColorSpec::from_str("#F80").expect("hex should parse"),
            ColorSpec(Color::Rgb(0xFF, 0x88, 0x00))
        );
        assert_eq!(
            ColorSpec::from_str("light cyan").expect("named should parse"),
            ColorSpec(Color::LightCyan)
        );
        assert_eq!(
            ColorSpec::from_str("dark_gray").expect("underscored name should parse"),
            ColorSpec(Color::DarkGray)
        );
    }

    #[test]
    fn color_spec_parses_indexed_and_rgb_forms() {
        assert_eq!(
            ColorSpec::from_str("indexed(42)").expect("indexed should parse"),
            ColorSpec(Color::Indexed(42))
        );
        assert_eq!(
            ColorSpec::from_str("rgb(1, 2, 3)").expect("rgb should parse"),
            ColorSpec(Color::Rgb(1, 2, 3))
        );
    }

    #[test]
    fn color_spec_rejects_invalid_values() {
        let err = ColorSpec::from_str("nope").expect_err("invalid color should fail");
        let message = err.to_string();
        assert!(
            message.contains("invalid color"),
            "expected invalid color message, got: {message}"
        );
    }

    #[test]
    fn paint_spec_parses_rgba_hex_and_function_forms() {
        assert_eq!(
            PaintSpec::from_str("#01020380").expect("rgba hex should parse"),
            PaintSpec(Paint::Alpha {
                color: Color::Rgb(1, 2, 3),
                alpha: 0x80,
            })
        );
        assert_eq!(
            PaintSpec::from_str("rgba(1, 2, 3, 128)").expect("integer alpha should parse"),
            PaintSpec(Paint::Alpha {
                color: Color::Rgb(1, 2, 3),
                alpha: 128,
            })
        );
        assert_eq!(
            PaintSpec::from_str("rgba(1, 2, 3, 0.5)").expect("float alpha should parse"),
            PaintSpec(Paint::Alpha {
                color: Color::Rgb(1, 2, 3),
                alpha: 128,
            })
        );
        assert_eq!(
            PaintSpec::from_str("blue").expect("named color should parse as solid paint"),
            PaintSpec(Paint::Solid(Color::Blue))
        );
    }

    #[test]
    fn paint_spec_rejects_invalid_alpha() {
        for value in [
            "rgba(1, 2, 3, 300)",
            "rgba(1, 2, 3, 1.5)",
            "rgba(1, 2, 3, nope)",
        ] {
            let err = PaintSpec::from_str(value).expect_err("invalid alpha should fail");
            let message = err.to_string();
            assert!(
                message.contains("alpha"),
                "expected alpha-specific error for {value}, got: {message}"
            );
        }
    }

    #[test]
    fn color_spec_rejects_alpha_with_clear_message() {
        let err = ColorSpec::from_str("#11223344").expect_err("alpha color should fail");
        let message = err.to_string();
        assert!(
            message.contains("paint-capable style fields"),
            "expected paint-capable field guidance, got: {message}"
        );

        let err = ColorSpec::from_str("rgba(1, 2, 3, 0.5)").expect_err("rgba color should fail");
        let message = err.to_string();
        assert!(
            message.contains("paint-capable style fields"),
            "expected rgba guidance, got: {message}"
        );
    }

    #[test]
    fn toml_theme_explicit_fields_override_extends_and_fallback() {
        let fallback = Theme::ansi();
        let overlay = toml::from_str::<TomlTheme>(
            r##"
extends = "dracula"
border_active = "#112233"

[primary]
fg = "#010203"
"##,
        )
        .expect("theme TOML should parse");

        let theme = overlay
            .into_theme(fallback)
            .expect("overlay should succeed");

        assert_eq!(
            theme.primary.fg,
            Some(Paint::Solid(Color::Rgb(0x01, 0x02, 0x03)))
        );
        assert_eq!(theme.primary.bg, Theme::dracula().primary.bg);
        assert_eq!(theme.border_active, Color::Rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn toml_theme_without_extends_uses_fallback() {
        let fallback = Theme::nord();
        let overlay = toml::from_str::<TomlTheme>(
            r##"
[status]
success = "#ABCDEF"
"##,
        )
        .expect("theme TOML should parse");

        let theme = overlay
            .into_theme(fallback.clone())
            .expect("overlay should succeed");
        assert_eq!(theme.primary, fallback.primary);
        assert_eq!(theme.status.success, Color::Rgb(0xAB, 0xCD, 0xEF));
    }

    #[test]
    fn toml_theme_document_overrides_apply_diagram_styles() {
        let overlay = toml::from_str::<TomlTheme>(
            r##"
[document.diagram_node_fill_style]
bg = "#101112"

[document.diagram_node_border_style]
fg = "#202122"

[document.diagram_node_label_style]
fg = "#303132"
bold = true

[document.diagram_edge_style]
fg = "#404142"
dim = true
"##,
        )
        .expect("theme TOML should parse");

        let theme = overlay
            .into_theme(Theme::default())
            .expect("overlay should succeed");

        assert_eq!(
            theme.document.diagram_node_fill_style.bg,
            Some(Paint::Solid(Color::Rgb(0x10, 0x11, 0x12)))
        );
        assert_eq!(
            theme.document.diagram_node_border_style.fg,
            Some(Paint::Solid(Color::Rgb(0x20, 0x21, 0x22)))
        );
        assert_eq!(
            theme.document.diagram_node_label_style.fg,
            Some(Paint::Solid(Color::Rgb(0x30, 0x31, 0x32)))
        );
        assert_eq!(theme.document.diagram_node_label_style.bold, Some(true));
        assert_eq!(
            theme.document.diagram_edge_style.fg,
            Some(Paint::Solid(Color::Rgb(0x40, 0x41, 0x42)))
        );
        assert_eq!(theme.document.diagram_edge_style.dim, Some(true));
    }

    #[test]
    fn toml_style_overlay_preserves_alpha_paint() {
        let overlay = toml::from_str::<TomlTheme>(
            r##"
[primary]
bg = "#101015CC"

[selection]
fg = "rgba(250, 240, 230, 0.5)"

[text_selection]
bg = "#20304080"
"##,
        )
        .expect("theme TOML should parse");

        let theme = overlay
            .into_theme(Theme::default())
            .expect("overlay should succeed");

        assert_eq!(
            theme.primary.bg,
            Some(Paint::Alpha {
                color: Color::Rgb(0x10, 0x10, 0x15),
                alpha: 0xCC,
            })
        );
        assert_eq!(
            theme.selection.fg,
            Some(Paint::Alpha {
                color: Color::Rgb(250, 240, 230),
                alpha: 128,
            })
        );
        assert_eq!(
            theme.text_selection.bg,
            Some(Paint::Alpha {
                color: Color::Rgb(0x20, 0x30, 0x40),
                alpha: 0x80,
            })
        );
    }

    #[test]
    fn color_only_palette_fields_reject_alpha() {
        let err = toml::from_str::<TomlTheme>(
            r##"
[status]
success = "#11223344"
"##,
        )
        .expect_err("color-only palette alpha should fail");
        let message = err.to_string();
        assert!(
            message.contains("paint-capable style fields"),
            "expected color-only alpha guidance, got: {message}"
        );
    }

    #[test]
    fn toml_theme_unknown_extends_lists_supported_presets() {
        let overlay =
            toml::from_str::<TomlTheme>(r#"extends = "unknown""#).expect("theme TOML should parse");
        let err = overlay
            .into_theme(Theme::default())
            .expect_err("unknown extends should fail");
        let message = err.to_string();

        assert!(
            message.contains("supported presets"),
            "expected supported presets in error, got: {message}"
        );
        assert!(
            message.contains("one_dark")
                && message.contains("catppuccin")
                && message.contains("ansi"),
            "expected preset list in error, got: {message}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn theme_watcher_emits_theme_after_file_write() {
        let dir = unique_temp_dir("theme-reload-watch");
        fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("theme.toml");

        fs::write(
            &path,
            r##"
[accent]
fg = "#112233"
"##,
        )
        .expect("initial theme file should be written");

        let watcher = ThemeWatcher::new(path.clone(), Theme::one_dark())
            .expect("theme watcher should initialize");

        thread::sleep(Duration::from_millis(75));

        fs::write(
            &path,
            r##"
[accent]
fg = "#445566"
"##,
        )
        .expect("updated theme file should be written");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut observed = None;
        while std::time::Instant::now() < deadline {
            if let Some(err) = watcher.try_recv_error() {
                panic!("watcher reported unexpected error: {err}");
            }

            if let Some(theme) = watcher.try_recv() {
                observed = Some(theme);
                break;
            }

            thread::sleep(Duration::from_millis(20));
        }

        let _ = fs::remove_dir_all(&dir);

        let theme = observed.expect("watcher should emit updated theme within timeout");
        assert_eq!(
            theme.accent.fg,
            Some(Paint::Solid(Color::Rgb(0x44, 0x55, 0x66)))
        );
    }
}
