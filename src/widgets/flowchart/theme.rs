use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::style::{BorderStyle, Style};

use super::{EdgeArrow, EdgeStyle};

/// Style defaults for flowchart nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeStyleSet {
    /// Default node style.
    pub default: Style,
    /// Per-class style map.
    pub classes: Arc<HashMap<Arc<str>, Style>>,
    /// Border style for rectangular node shapes.
    pub border_style: BorderStyle,
}

impl Hash for NodeStyleSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.default.hash(state);
        self.border_style.hash(state);
        let mut entries: Vec<_> = self.classes.iter().collect();
        entries.sort_by_key(|(key, _)| *key);
        entries.len().hash(state);
        for (key, value) in entries {
            key.hash(state);
            value.hash(state);
        }
    }
}

impl Default for NodeStyleSet {
    fn default() -> Self {
        Self {
            default: Style::default(),
            classes: Arc::new(HashMap::new()),
            border_style: BorderStyle::Rounded,
        }
    }
}

/// Glyph set used for one edge style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EdgeGlyphs {
    /// Horizontal segment glyph.
    pub horizontal: char,
    /// Vertical segment glyph.
    pub vertical: char,
    /// Border style used for bends and junctions.
    pub border_style: BorderStyle,
}

impl EdgeGlyphs {
    const fn solid() -> Self {
        Self {
            horizontal: '─',
            vertical: '│',
            border_style: BorderStyle::Plain,
        }
    }

    const fn dashed() -> Self {
        Self {
            horizontal: '╌',
            vertical: '╎',
            border_style: BorderStyle::Plain,
        }
    }

    const fn thick() -> Self {
        Self {
            horizontal: '━',
            vertical: '┃',
            border_style: BorderStyle::Plain,
        }
    }

    const fn invisible() -> Self {
        Self {
            horizontal: ' ',
            vertical: ' ',
            border_style: BorderStyle::Plain,
        }
    }
}

/// Glyphs for one arrowhead style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArrowGlyphs {
    /// Up-facing arrowhead.
    pub up: char,
    /// Down-facing arrowhead.
    pub down: char,
    /// Left-facing arrowhead.
    pub left: char,
    /// Right-facing arrowhead.
    pub right: char,
}

impl ArrowGlyphs {
    const fn new(up: char, down: char, left: char, right: char) -> Self {
        Self {
            up,
            down,
            left,
            right,
        }
    }
}

/// Styling for subgraph boxes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SubgraphTheme {
    /// Base subgraph style.
    pub style: Style,
    /// Header label style.
    pub header_style: Style,
    /// Subgraph border style.
    pub border_style: BorderStyle,
}

impl Default for SubgraphTheme {
    fn default() -> Self {
        Self {
            style: Style::default(),
            header_style: Style::default(),
            border_style: BorderStyle::Rounded,
        }
    }
}

/// Diagram-local flowchart theme.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowchartTheme {
    /// Node styles and border defaults.
    pub node_styles: NodeStyleSet,
    /// Edge glyphs indexed by [`EdgeStyle`].
    pub edge_glyphs: [EdgeGlyphs; 4],
    /// Arrow glyphs indexed by [`EdgeArrow`].
    pub arrow_heads: [ArrowGlyphs; 5],
    /// Subgraph style defaults.
    pub subgraph: SubgraphTheme,
    /// Default label style.
    pub label_style: Style,
    /// Hover overlay style.
    pub item_hover_style: Style,
}

impl FlowchartTheme {
    /// Default Mermaid-like theme using box drawing and filled arrowheads.
    pub fn classic() -> Self {
        Self {
            node_styles: NodeStyleSet::default(),
            edge_glyphs: [
                EdgeGlyphs::solid(),
                EdgeGlyphs::dashed(),
                EdgeGlyphs::thick(),
                EdgeGlyphs::invisible(),
            ],
            arrow_heads: [
                ArrowGlyphs::new(' ', ' ', ' ', ' '),
                ArrowGlyphs::new('△', '▽', '◁', '▷'),
                ArrowGlyphs::new('▲', '▼', '◀', '▶'),
                ArrowGlyphs::new('×', '×', '×', '×'),
                ArrowGlyphs::new('○', '○', '○', '○'),
            ],
            subgraph: SubgraphTheme::default(),
            label_style: Style::default(),
            item_hover_style: Style::default(),
        }
    }

    /// Minimal thin-stroke theme with open arrowheads.
    pub fn minimal() -> Self {
        let mut theme = Self::classic();
        theme.node_styles.border_style = BorderStyle::Plain;
        theme.arrow_heads[EdgeArrow::Filled as usize] = ArrowGlyphs::new('△', '▽', '◁', '▷');
        theme
    }

    /// ASCII-only fallback theme.
    pub fn ascii() -> Self {
        Self {
            node_styles: NodeStyleSet {
                border_style: BorderStyle::Plain,
                ..NodeStyleSet::default()
            },
            edge_glyphs: [
                EdgeGlyphs {
                    horizontal: '-',
                    vertical: '|',
                    border_style: BorderStyle::Plain,
                },
                EdgeGlyphs {
                    horizontal: '-',
                    vertical: ':',
                    border_style: BorderStyle::Plain,
                },
                EdgeGlyphs {
                    horizontal: '=',
                    vertical: '#',
                    border_style: BorderStyle::Plain,
                },
                EdgeGlyphs::invisible(),
            ],
            arrow_heads: [
                ArrowGlyphs::new(' ', ' ', ' ', ' '),
                ArrowGlyphs::new('^', 'v', '<', '>'),
                ArrowGlyphs::new('^', 'v', '<', '>'),
                ArrowGlyphs::new('x', 'x', 'x', 'x'),
                ArrowGlyphs::new('o', 'o', 'o', 'o'),
            ],
            subgraph: SubgraphTheme {
                border_style: BorderStyle::Plain,
                ..SubgraphTheme::default()
            },
            label_style: Style::default(),
            item_hover_style: Style::default(),
        }
    }
}

impl Default for FlowchartTheme {
    fn default() -> Self {
        Self::classic()
    }
}

impl EdgeStyle {
    pub(crate) const fn theme_index(self) -> usize {
        match self {
            EdgeStyle::Solid => 0,
            EdgeStyle::Dashed => 1,
            EdgeStyle::Thick => 2,
            EdgeStyle::Invisible => 3,
        }
    }
}

impl EdgeArrow {
    pub(crate) const fn theme_index(self) -> usize {
        match self {
            EdgeArrow::None => 0,
            EdgeArrow::Open => 1,
            EdgeArrow::Filled => 2,
            EdgeArrow::Cross => 3,
            EdgeArrow::Circle => 4,
        }
    }
}
