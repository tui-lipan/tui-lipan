/// Glyphs used to render pseudo-states in a [`StateDiagram`](super::StateDiagram).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDiagramTheme {
    /// Glyph for the initial pseudo-state. Default: `'●'`.
    pub start: char,
    /// Glyph for the final pseudo-state. Default: `'◉'`.
    pub end: char,
    /// Glyph for a choice pseudo-state. Default: `'◇'`.
    pub choice: char,
    /// Glyph for fork/join bars. Default: `'━'`.
    pub fork_join: char,
}
impl Default for StateDiagramTheme {
    fn default() -> Self {
        Self {
            start: '●',
            end: '◉',
            choice: '◇',
            fork_join: '━',
        }
    }
}
