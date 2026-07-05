use std::fmt::Write;

use crate::capture::CapturedFrame;
#[cfg(feature = "ui-snapshot-png")]
use crate::capture::PngOptions;
use crate::core::element::Key;
#[cfg(feature = "ui-snapshot-json")]
use crate::style::Color;
use crate::style::Rect;
#[cfg(feature = "ui-snapshot-json")]
use crate::widgets::CheckboxState;

use super::describe::UiWidgetDesc;
#[cfg(feature = "ui-snapshot-json")]
use super::options::UiSnapshotFormatOptions;

/// Combined visual + semantic UI snapshot for agent/design review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSnapshot {
    /// Layout viewport used for capture.
    pub viewport: Rect,
    /// Rendered pixel buffer.
    pub frame: CapturedFrame,
    /// Semantic widget descriptions (includes overlays).
    pub widgets: Vec<UiWidgetDesc>,
    /// Reconciliation key of the focused widget, if any.
    pub focus_key: Option<Key>,
    /// Reconciliation key of the hovered widget, if any.
    pub hover_key: Option<Key>,
}

impl UiSnapshot {
    /// Returns a markdown report suitable for agents and design review.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# UI Snapshot ({}x{})",
            self.viewport.w, self.viewport.h
        );
        out.push('\n');

        out.push_str("## Focus\n\n");
        match &self.focus_key {
            Some(key) => {
                let _ = writeln!(out, "- focus_key: {}", markdown_inline(key.as_ref()));
            }
            None => out.push_str("- focus_key: _(none)_\n"),
        }
        match &self.hover_key {
            Some(key) => {
                let _ = writeln!(out, "- hover_key: {}", markdown_inline(key.as_ref()));
            }
            None => out.push_str("- hover_key: _(none)_\n"),
        }
        out.push('\n');

        out.push_str("## Widgets\n\n");
        if self.widgets.is_empty() {
            out.push_str("_(none)_\n\n");
        } else {
            for widget in &self.widgets {
                append_widget_markdown(&mut out, widget);
            }
            out.push('\n');
        }

        out.push_str("## Render\n\n```\n");
        out.push_str(&self.frame.to_fixed_grid());
        out.push_str("\n```\n");
        out
    }

    /// Returns JSON when the `ui-snapshot-json` feature is enabled.
    #[cfg(feature = "ui-snapshot-json")]
    pub fn to_json(&self) -> String {
        self.to_json_with_options(&UiSnapshotFormatOptions::default())
    }

    /// Returns pretty-printed JSON when the `ui-snapshot-json` feature is enabled.
    #[cfg(feature = "ui-snapshot-json")]
    pub fn to_json_pretty(&self) -> String {
        let view = UiSnapshotJsonView::from_snapshot(self, &UiSnapshotFormatOptions::default());
        serde_json::to_string_pretty(&view).unwrap_or_else(|_| "{}".to_string())
    }

    /// Returns JSON with custom format options when the feature is enabled.
    #[cfg(feature = "ui-snapshot-json")]
    pub fn to_json_with_options(&self, options: &UiSnapshotFormatOptions) -> String {
        let view = UiSnapshotJsonView::from_snapshot(self, options);
        serde_json::to_string(&view).unwrap_or_else(|_| "{}".to_string())
    }

    /// Returns PNG bytes when the `ui-snapshot-png` feature is enabled.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn to_png(&self, options: &PngOptions) -> Vec<u8> {
        self.frame.to_png(options)
    }

    /// Returns PNG bytes or an encoder error when the `ui-snapshot-png` feature is enabled.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn try_to_png(&self, options: &PngOptions) -> crate::Result<Vec<u8>> {
        self.frame.try_to_png(options)
    }

    /// Returns PNG bytes with default rendering options when the feature is enabled.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn to_png_default(&self) -> Vec<u8> {
        self.to_png(&PngOptions::default())
    }

    /// Returns PNG bytes with default rendering options, surfacing encoder errors.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn try_to_png_default(&self) -> crate::Result<Vec<u8>> {
        self.try_to_png(&PngOptions::default())
    }
}

fn append_widget_markdown(out: &mut String, widget: &UiWidgetDesc) {
    let key = widget
        .key
        .as_ref()
        .map(|k| format!(" key={}", markdown_inline(k.as_ref())))
        .unwrap_or_default();
    let flags = widget_flags(widget);
    let _ = writeln!(
        out,
        "- **{}**{} @ ({},{}) {}x{}{flags}",
        widget.kind, key, widget.rect.x, widget.rect.y, widget.rect.w, widget.rect.h
    );
    if let Some(title) = &widget.title {
        let _ = writeln!(out, "  - title: {}", markdown_inline(title));
    }
    if let Some(label) = &widget.label {
        let _ = writeln!(out, "  - label: {}", markdown_inline(label));
    }
    if let Some(placeholder) = &widget.placeholder {
        let _ = writeln!(out, "  - placeholder: {}", markdown_inline(placeholder));
    }
    if widget.value_masked {
        out.push_str("  - value: _(masked)_\n");
    } else if let Some(value) = &widget.value {
        let _ = writeln!(out, "  - value: {}", markdown_inline(value));
    }
    if let Some(state) = widget.checkbox_state {
        let _ = writeln!(out, "  - checkbox_state: {state:?}");
    }
    if let Some(selected) = widget.selected_index {
        let _ = writeln!(out, "  - selected_index: {selected}");
    }
    if let Some(offset) = widget.scroll_offset {
        let _ = writeln!(out, "  - scroll_offset: {offset}");
    }
    if let Some(labels) = &widget.item_labels {
        let total = widget
            .total_items
            .map(|t| format!(" (total_items={t})"))
            .unwrap_or_default();
        let _ = writeln!(out, "  - item_labels{total}:");
        for label in labels {
            let _ = writeln!(out, "    - {}", markdown_inline(label));
        }
    }
    if let Some(count) = widget.child_count {
        let _ = writeln!(out, "  - child_count: {count}");
    }
}

fn markdown_inline(text: &str) -> String {
    let text = text.replace('\r', "\\r").replace('\n', "\\n");
    if !text.contains('`') {
        return format!("`{text}`");
    }
    let fence_len = text.chars().filter(|&c| c == '`').count().max(1) + 1;
    let fence = "`".repeat(fence_len);
    format!("{fence}{text}{fence}")
}

#[cfg(feature = "ui-snapshot-json")]
fn color_to_json_wire(color: &Color) -> String {
    match color {
        Color::Reset => "reset".into(),
        Color::Backdrop => "backdrop".into(),
        Color::Transparent => "transparent".into(),
        Color::Black => "black".into(),
        Color::Red => "red".into(),
        Color::Green => "green".into(),
        Color::Yellow => "yellow".into(),
        Color::Blue => "blue".into(),
        Color::Magenta => "magenta".into(),
        Color::Cyan => "cyan".into(),
        Color::Gray => "gray".into(),
        Color::DarkGray => "dark_gray".into(),
        Color::LightRed => "light_red".into(),
        Color::LightGreen => "light_green".into(),
        Color::LightYellow => "light_yellow".into(),
        Color::LightBlue => "light_blue".into(),
        Color::LightMagenta => "light_magenta".into(),
        Color::LightCyan => "light_cyan".into(),
        Color::White => "white".into(),
        Color::Indexed(index) => format!("indexed({index})"),
        Color::Rgb(r, g, b) => format!("rgb({r},{g},{b})"),
    }
}

#[cfg(feature = "ui-snapshot-json")]
fn checkbox_state_to_json_wire(state: CheckboxState) -> &'static str {
    match state {
        CheckboxState::Unchecked => "unchecked",
        CheckboxState::Checked => "checked",
        CheckboxState::Indeterminate => "indeterminate",
    }
}

fn widget_flags(widget: &UiWidgetDesc) -> String {
    let mut flags = Vec::new();
    if widget.focused {
        flags.push("focused");
    }
    if widget.hovered {
        flags.push("hovered");
    }
    if widget.rect.w == 0 || widget.rect.h == 0 {
        flags.push("zero-area");
    }
    if flags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", flags.join(", "))
    }
}

#[cfg(feature = "ui-snapshot-json")]
#[derive(serde::Serialize)]
struct UiSnapshotJsonView {
    viewport: RectJson,
    focus_key: Option<String>,
    hover_key: Option<String>,
    grid: String,
    widgets: Vec<UiWidgetDescJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cells: Option<Vec<CapturedCellJson>>,
}

#[cfg(feature = "ui-snapshot-json")]
#[derive(serde::Serialize)]
struct RectJson {
    x: i16,
    y: i16,
    w: u16,
    h: u16,
}

#[cfg(feature = "ui-snapshot-json")]
#[derive(serde::Serialize)]
struct UiWidgetDescJson {
    kind: String,
    key: Option<String>,
    rect: RectJson,
    focused: bool,
    hovered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    value_masked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkbox_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scroll_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_count: Option<usize>,
}

#[cfg(feature = "ui-snapshot-json")]
#[derive(serde::Serialize)]
struct CapturedCellJson {
    symbol: String,
    fg: String,
    bg: String,
}

#[cfg(feature = "ui-snapshot-json")]
impl UiSnapshotJsonView {
    fn from_snapshot(snapshot: &UiSnapshot, options: &UiSnapshotFormatOptions) -> Self {
        Self {
            viewport: RectJson {
                x: snapshot.viewport.x,
                y: snapshot.viewport.y,
                w: snapshot.viewport.w,
                h: snapshot.viewport.h,
            },
            focus_key: snapshot.focus_key.as_ref().map(|k| k.as_ref().to_string()),
            hover_key: snapshot.hover_key.as_ref().map(|k| k.as_ref().to_string()),
            grid: snapshot.frame.to_fixed_grid(),
            widgets: snapshot
                .widgets
                .iter()
                .map(UiWidgetDescJson::from)
                .collect(),
            cells: options.include_cells.then(|| {
                snapshot
                    .frame
                    .cells
                    .iter()
                    .map(|cell| CapturedCellJson {
                        symbol: cell.symbol.clone(),
                        fg: color_to_json_wire(&cell.fg),
                        bg: color_to_json_wire(&cell.bg),
                    })
                    .collect()
            }),
        }
    }
}

#[cfg(feature = "ui-snapshot-json")]
impl From<&UiWidgetDesc> for UiWidgetDescJson {
    fn from(widget: &UiWidgetDesc) -> Self {
        Self {
            kind: widget.kind.to_string(),
            key: widget.key.as_ref().map(|k| k.as_ref().to_string()),
            rect: RectJson {
                x: widget.rect.x,
                y: widget.rect.y,
                w: widget.rect.w,
                h: widget.rect.h,
            },
            focused: widget.focused,
            hovered: widget.hovered,
            title: widget.title.clone(),
            label: widget.label.clone(),
            placeholder: widget.placeholder.clone(),
            value: widget.value.clone(),
            value_masked: widget.value_masked,
            checkbox_state: widget
                .checkbox_state
                .map(checkbox_state_to_json_wire)
                .map(str::to_string),
            selected_index: widget.selected_index,
            scroll_offset: widget.scroll_offset,
            item_labels: widget.item_labels.clone(),
            total_items: widget.total_items,
            child_count: widget.child_count,
        }
    }
}
