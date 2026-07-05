//! Agent-oriented UI snapshots combining rendered frames and semantic widget metadata.

mod describe;
mod format;
mod kind;
mod options;
mod request;
mod slot;

use describe::{describe_widgets, key_for_node};

pub use describe::UiWidgetDesc;
pub use format::UiSnapshot;
pub use kind::UiWidgetKind;
pub use options::{UiSnapshotFileFormat, UiSnapshotFormatOptions, UiSnapshotOptions};
pub(crate) use request::UiSnapshotRequest;
pub use slot::UiSnapshotSlot;

use crate::backend::ratatui_backend::capture_render::{
    CaptureInteraction, render_to_captured_frame_with_interaction,
};
use crate::core::node::NodeTree;
use crate::style::Rect;

/// Build a combined visual + semantic UI snapshot from runtime state.
pub(crate) fn build_ui_snapshot(
    tree: &NodeTree,
    viewport: Rect,
    interaction: CaptureInteraction,
    effect_phase: u64,
    screen_background: Option<ratatui::style::Style>,
    options: &UiSnapshotOptions,
) -> UiSnapshot {
    let CaptureInteraction {
        focused, hovered, ..
    } = interaction;
    let frame = render_to_captured_frame_with_interaction(
        tree,
        viewport,
        interaction,
        effect_phase,
        screen_background,
    );
    let widgets = describe_widgets(tree, focused, hovered, options);
    UiSnapshot {
        viewport,
        frame,
        widgets,
        focus_key: key_for_node(tree, focused),
        hover_key: key_for_node(tree, hovered),
    }
}

/// Write a snapshot to disk using the requested format.
pub(crate) fn write_snapshot(
    snapshot: &UiSnapshot,
    path: &std::path::Path,
    format: UiSnapshotFileFormat,
) -> crate::Result<()> {
    let content = match format {
        UiSnapshotFileFormat::Markdown => snapshot.to_markdown().into_bytes(),
        #[cfg(feature = "ui-snapshot-json")]
        UiSnapshotFileFormat::Json => snapshot.to_json_pretty().into_bytes(),
        #[cfg(feature = "ui-snapshot-png")]
        UiSnapshotFileFormat::Png => snapshot.try_to_png_default()?,
    };
    std::fs::write(path, content).map_err(crate::Error::from)
}
