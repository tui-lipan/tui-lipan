/// Visibility-prefix glyphs used to render [`ClassMember`](super::ClassMember)s in
/// a [`ClassDiagram`](super::ClassDiagram).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassDiagramTheme {
    /// Glyph for public members. Default: `'+'`.
    pub public: char,
    /// Glyph for private members. Default: `'-'`.
    pub private: char,
    /// Glyph for protected members. Default: `'#'`.
    pub protected: char,
    /// Glyph for package members. Default: `'~'`.
    pub package: char,
}

impl Default for ClassDiagramTheme {
    fn default() -> Self {
        Self {
            public: '+',
            private: '-',
            protected: '#',
            package: '~',
        }
    }
}
