/// Badge labels shown next to key attributes in an [`ErDiagram`](super::ErDiagram).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErDiagramTheme {
    /// Label for primary-key attributes. Default: `"PK"`.
    pub pk: &'static str,
    /// Label for foreign-key attributes. Default: `"FK"`.
    pub fk: &'static str,
    /// Label for unique-key attributes. Default: `"UK"`.
    pub uk: &'static str,
}
impl Default for ErDiagramTheme {
    fn default() -> Self {
        Self {
            pk: "PK",
            fk: "FK",
            uk: "UK",
        }
    }
}
