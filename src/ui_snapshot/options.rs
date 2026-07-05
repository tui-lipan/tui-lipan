/// Options controlling semantic widget extraction from the node tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSnapshotOptions {
    /// Include zero-area layout nodes.
    pub include_zero_area: bool,
    /// Include spacers and empty dividers.
    pub include_chrome: bool,
    /// Maximum list/table item labels emitted per widget.
    pub max_list_items: usize,
}

#[allow(clippy::derivable_impls)]
impl Default for UiSnapshotOptions {
    fn default() -> Self {
        Self {
            include_zero_area: false,
            include_chrome: false,
            max_list_items: 20,
        }
    }
}

impl UiSnapshotOptions {
    /// Returns diagnostic options for clipped, zero-area, or flex layout debugging.
    ///
    /// This includes zero-area nodes plus chrome widgets such as spacers and dividers,
    /// while keeping the default list/table preview limit.
    pub fn diagnostic() -> Self {
        Self {
            include_zero_area: true,
            include_chrome: true,
            ..Self::default()
        }
    }
}

/// Options for JSON/markdown export formatting.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UiSnapshotFormatOptions {
    /// Include the full captured cell buffer in JSON export.
    pub include_cells: bool,
}

/// File format for queued live-app snapshot export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSnapshotFileFormat {
    /// Markdown report (always available).
    Markdown,
    /// JSON export (`ui-snapshot-json` feature).
    #[cfg(feature = "ui-snapshot-json")]
    Json,
    /// PNG image export (`ui-snapshot-png` feature).
    #[cfg(feature = "ui-snapshot-png")]
    Png,
}

#[allow(clippy::derivable_impls)]
impl Default for UiSnapshotFileFormat {
    fn default() -> Self {
        #[cfg(feature = "ui-snapshot-json")]
        {
            Self::Json
        }
        #[cfg(not(feature = "ui-snapshot-json"))]
        {
            Self::Markdown
        }
    }
}
