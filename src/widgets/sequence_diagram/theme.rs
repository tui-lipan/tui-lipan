use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::style::{BorderGlyphs, BorderStyle, Style};

use super::{FragmentKind, MessageStyle};

/// Number of message styles; size of the per-style theme tables.
pub const MESSAGE_STYLE_COUNT: usize = MessageStyle::INDEX_COUNT;
/// Number of fragment kinds; size of the per-kind theme tables.
pub const FRAGMENT_KIND_COUNT: usize = FragmentKind::INDEX_COUNT;

const ASCII_BORDER: BorderStyle = BorderStyle::Custom {
    glyphs: BorderGlyphs {
        top_left: "+",
        top: "-",
        top_right: "+",
        left: "|",
        right: "|",
        bottom_left: "+",
        bottom: "-",
        bottom_right: "+",
    },
};

/// Full visual theme for a [`SequenceDiagram`](super::SequenceDiagram): per-kind
/// styles, glyph sets, and chrome styling. Use the [`classic`](Self::classic),
/// [`minimal`](Self::minimal), or [`ascii`](Self::ascii) presets as a starting point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceDiagramTheme {
    /// Per-[`MessageStyle`](super::MessageStyle) line/label styles, keyed by index.
    pub message_styles: [Style; MESSAGE_STYLE_COUNT],
    /// Glyphs used to draw message lines and arrowheads.
    pub message_glyphs: MessageGlyphs,
    /// Per-[`FragmentKind`](super::FragmentKind) block styles, keyed by index.
    pub fragment_styles: [Style; FRAGMENT_KIND_COUNT],
    /// Glyphs used to draw fragment borders and branch separators.
    pub fragment_glyphs: FragmentGlyphs,
    /// Lifeline glyph and style.
    pub lifeline: LifelineTheme,
    /// Activation-bar glyph and style.
    pub activation: ActivationTheme,
    /// Autonumber badge format and style.
    pub autonumber: AutonumberTheme,
    /// Style of note boxes.
    pub note_style: Style,
    /// Border style of note boxes.
    pub note_border: BorderStyle,
    /// Style of participant header boxes.
    pub participant_style: Style,
    /// Border style of participant header boxes.
    pub participant_border: BorderStyle,
    /// Default style of message labels.
    pub message_label_style: Style,
    /// Style applied to a hovered item.
    pub hover_style: Style,
}

impl SequenceDiagramTheme {
    /// The default Mermaid-like theme: Unicode box-drawing glyphs, boxed participants.
    pub fn classic() -> Self {
        Self {
            message_styles: [Style::default(); MESSAGE_STYLE_COUNT],
            message_glyphs: MessageGlyphs::classic(),
            fragment_styles: [Style::default(); FRAGMENT_KIND_COUNT],
            fragment_glyphs: FragmentGlyphs::classic(),
            lifeline: LifelineTheme {
                glyph: '┊',
                style: Style::default(),
            },
            activation: ActivationTheme::classic(),
            autonumber: AutonumberTheme::classic(),
            note_style: Style::default(),
            note_border: BorderStyle::Plain,
            participant_style: Style::default(),
            participant_border: BorderStyle::Plain,
            message_label_style: Style::default(),
            hover_style: Style::default(),
        }
    }

    /// A compact theme pairing with the [`Minimal`](super::SequenceDiagramVariant::Minimal) variant.
    pub fn minimal() -> Self {
        Self {
            lifeline: LifelineTheme {
                glyph: '│',
                style: Style::default(),
            },
            autonumber: AutonumberTheme {
                format: Arc::from("{n}"),
                style: Style::default(),
            },
            ..Self::classic()
        }
    }

    /// An all-ASCII theme for terminals without Unicode box-drawing support.
    pub fn ascii() -> Self {
        Self {
            message_glyphs: MessageGlyphs::ascii(),
            fragment_glyphs: FragmentGlyphs::ascii(),
            lifeline: LifelineTheme {
                glyph: '|',
                style: Style::default(),
            },
            activation: ActivationTheme {
                fill_glyph: '|',
                style: Style::default(),
                fill_background: true,
            },
            note_border: ASCII_BORDER,
            participant_border: ASCII_BORDER,
            autonumber: AutonumberTheme::classic(),
            ..Self::classic()
        }
    }

    pub(crate) fn message_style(&self, kind: MessageStyle) -> Style {
        self.message_styles[kind.index()]
    }

    pub(crate) fn message_style_mut(&mut self, kind: MessageStyle) -> &mut Style {
        &mut self.message_styles[kind.index()]
    }

    pub(crate) fn fragment_style(&self, kind: FragmentKind) -> Style {
        self.fragment_styles[kind.index()]
    }

    pub(crate) fn fragment_style_mut(&mut self, kind: FragmentKind) -> &mut Style {
        &mut self.fragment_styles[kind.index()]
    }
}

impl Default for SequenceDiagramTheme {
    fn default() -> Self {
        Self::classic()
    }
}

impl Hash for SequenceDiagramTheme {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.message_styles.hash(state);
        self.message_glyphs.hash(state);
        self.fragment_styles.hash(state);
        self.fragment_glyphs.hash(state);
        self.lifeline.hash(state);
        self.activation.hash(state);
        self.autonumber.hash(state);
        self.note_style.hash(state);
        self.note_border.hash(state);
        self.participant_style.hash(state);
        self.participant_border.hash(state);
        self.message_label_style.hash(state);
        self.hover_style.hash(state);
    }
}

/// Glyphs used to draw message lines, arrowheads, and self-loop corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageGlyphs {
    /// Solid message line.
    pub line: char,
    /// Dashed (reply) message line.
    pub dashed_line: char,
    /// Filled arrowhead pointing right.
    pub arrow_filled_right: char,
    /// Filled arrowhead pointing left.
    pub arrow_filled_left: char,
    /// Open arrowhead pointing right.
    pub arrow_open_right: char,
    /// Open arrowhead pointing left.
    pub arrow_open_left: char,
    /// Terminator glyph for lost/open messages.
    pub lost_terminator: char,
    /// Top-right corner of a self-message loop.
    pub self_loop_top_right: char,
    /// Bottom-right corner of a self-message loop.
    pub self_loop_bottom_right: char,
}

impl MessageGlyphs {
    /// Unicode box-drawing glyph set.
    pub fn classic() -> Self {
        Self {
            line: '─',
            dashed_line: '╌',
            arrow_filled_right: '▶',
            arrow_filled_left: '◀',
            arrow_open_right: '〉',
            arrow_open_left: '〈',
            lost_terminator: '╳',
            self_loop_top_right: '╮',
            self_loop_bottom_right: '╯',
        }
    }

    /// All-ASCII glyph set.
    pub fn ascii() -> Self {
        Self {
            line: '-',
            dashed_line: '-',
            arrow_filled_right: '>',
            arrow_filled_left: '<',
            arrow_open_right: '>',
            arrow_open_left: '<',
            lost_terminator: 'x',
            self_loop_top_right: '+',
            self_loop_bottom_right: '+',
        }
    }
}

impl Default for MessageGlyphs {
    fn default() -> Self {
        Self::classic()
    }
}

/// Glyphs used to draw fragment-block borders and branch separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FragmentGlyphs {
    /// Border style of the fragment box.
    pub border: BorderStyle,
    /// Separator drawn between fragment branches.
    pub branch_separator: char,
}

impl FragmentGlyphs {
    /// Unicode box-drawing glyph set.
    pub fn classic() -> Self {
        Self {
            border: BorderStyle::Plain,
            branch_separator: '╌',
        }
    }

    /// All-ASCII glyph set.
    pub fn ascii() -> Self {
        Self {
            border: ASCII_BORDER,
            branch_separator: '-',
        }
    }
}

impl Default for FragmentGlyphs {
    fn default() -> Self {
        Self::classic()
    }
}

/// Lifeline glyph and style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LifelineTheme {
    /// Glyph repeated vertically to draw the lifeline.
    pub glyph: char,
    /// Style applied to the lifeline.
    pub style: Style,
}

/// Activation-bar glyph and style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActivationTheme {
    /// Glyph used to fill the activation bar.
    pub fill_glyph: char,
    /// Style applied to the activation bar.
    pub style: Style,
    /// Whether the bar fills its background (vs. drawing only the glyph).
    pub fill_background: bool,
}

impl ActivationTheme {
    /// Unicode-glyph activation theme with a filled background.
    pub fn classic() -> Self {
        Self {
            fill_glyph: '┃',
            style: Style::default(),
            fill_background: true,
        }
    }
}

/// Autonumber badge format and style.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AutonumberTheme {
    /// Format string; `{n}` is replaced with the message number.
    pub format: Arc<str>,
    /// Style applied to the badge.
    pub style: Style,
}

impl AutonumberTheme {
    /// Default `[{n}]` badge style.
    pub fn classic() -> Self {
        Self {
            format: Arc::from("[{n}]"),
            style: Style::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SequenceDiagram, SequenceDiagramNode};
    use super::*;
    use crate::style::Color;
    use crate::widgets::sequence_diagram::reconcile_sequence_diagram;

    #[test]
    fn default_theme_matches_classic_preset() {
        assert_eq!(
            SequenceDiagramTheme::default(),
            SequenceDiagramTheme::classic()
        );
    }

    #[test]
    fn flat_setter_lifeline_style_mutates_theme_slot() {
        let style = Style::new().fg(Color::Red);
        let diagram = SequenceDiagram::new().lifeline_style(style);
        assert_eq!(diagram.theme.lifeline.style, style);
    }

    #[test]
    fn message_kind_style_only_affects_target_kind() {
        let style = Style::new().fg(Color::Red);
        let diagram = SequenceDiagram::new().message_kind_style(MessageStyle::Sync, style);
        assert_eq!(diagram.theme.message_style(MessageStyle::Sync), style);
        assert_eq!(
            diagram.theme.message_style(MessageStyle::Async),
            Style::default()
        );
    }

    #[test]
    fn theme_changes_invalidate_widget_cache() {
        let mut node = SequenceDiagramNode::default();
        let diagram = SequenceDiagram::new();
        assert!(reconcile_sequence_diagram(&diagram, &mut node));
        let themed = diagram.theme(SequenceDiagramTheme::ascii());
        assert!(reconcile_sequence_diagram(&themed, &mut node));
    }

    #[test]
    fn ascii_preset_uses_no_unicode_box_drawing() {
        let theme = SequenceDiagramTheme::ascii();
        let glyphs = [
            theme.message_glyphs.line,
            theme.message_glyphs.dashed_line,
            theme.message_glyphs.arrow_filled_right,
            theme.message_glyphs.arrow_filled_left,
            theme.message_glyphs.arrow_open_right,
            theme.message_glyphs.arrow_open_left,
            theme.message_glyphs.lost_terminator,
            theme.message_glyphs.self_loop_top_right,
            theme.message_glyphs.self_loop_bottom_right,
            theme.fragment_glyphs.branch_separator,
            theme.lifeline.glyph,
            theme.activation.fill_glyph,
        ];
        assert!(glyphs.iter().all(char::is_ascii));
        assert_ascii_border(theme.fragment_glyphs.border);
        assert_ascii_border(theme.note_border);
    }

    fn assert_ascii_border(border: BorderStyle) {
        let BorderStyle::Custom { glyphs } = border else {
            panic!("expected custom ASCII border");
        };
        let all = [
            glyphs.top_left,
            glyphs.top,
            glyphs.top_right,
            glyphs.left,
            glyphs.right,
            glyphs.bottom_left,
            glyphs.bottom,
            glyphs.bottom_right,
        ];
        assert!(all.iter().all(|glyph| glyph.is_ascii()));
    }
}
