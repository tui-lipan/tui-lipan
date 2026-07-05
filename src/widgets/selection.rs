/// Behavior used for triple-click text selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TripleClickSelectionMode {
    /// Select the current logical line or rendered visual line.
    #[default]
    Line,
    /// Select the current paragraph bounded by blank lines.
    Paragraph,
}
