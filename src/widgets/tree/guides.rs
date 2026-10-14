//! Tree indentation guides, independent of the [`Tree`](super::Tree) widget.
//!
//! [`Tree`](super::Tree) draws each row's indentation from where the row sits: how deep it is,
//! whether it is its parent's last child, and which ancestors still have siblings below it. That
//! geometry is useful on its own. An app whose rows are composed elements rather than
//! [`ListItem`](crate::widgets::ListItem)s can describe each row's place with a [`TreeBranch`] and
//! draw the same guides with [`TreeGuides`], so its hierarchy reads exactly like a `Tree`.

use super::IndentStyle;
use crate::style::{Span, Style};
use crate::utils::gradient::ColorGradient;
use unicode_width::UnicodeWidthStr;

/// Where one visible row sits in a tree.
///
/// Build it top-down: [`TreeBranch::root`] for a top-level row, then [`TreeBranch::child`] for each
/// row under it, passing whether that child is the last of its siblings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TreeBranch {
    pub(crate) depth: usize,
    pub(crate) last_child: bool,
    /// One entry per level below the root: whether the ancestor at that level has a later sibling,
    /// so a guide line continues through this row's column for it.
    pub(crate) continued: Vec<bool>,
}

impl TreeBranch {
    /// A top-level row.
    pub fn root() -> Self {
        Self {
            depth: 0,
            last_child: true,
            continued: Vec::new(),
        }
    }

    /// A row directly under this one. `last` says it has no later sibling.
    pub fn child(&self, last: bool) -> Self {
        let mut continued = self.continued.clone();
        continued.push(!last);
        Self {
            depth: self.depth + 1,
            last_child: last,
            continued,
        }
    }

    /// How many levels below a top-level row this one is.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Whether the row has no later sibling.
    pub fn is_last_child(&self) -> bool {
        self.last_child
    }
}

/// How to draw the indentation in front of a tree row.
///
/// ```
/// use tui_lipan::prelude::*;
///
/// let guides = TreeGuides::new(IndentStyle::Short).indent(2);
/// let project = TreeBranch::root();
/// let first = project.child(false);
/// let last = project.child(true);
/// let text = |branch: &TreeBranch| {
///     guides
///         .spans(branch, 1)
///         .iter()
///         .map(|span| span.content.to_string())
///         .collect::<String>()
/// };
/// assert_eq!(text(&project), "");
/// assert_eq!(text(&first), "├ ");
/// assert_eq!(text(&last), "└ ");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TreeGuides {
    style: IndentStyle,
    indent: u16,
    guide_style: Style,
    gradient: Option<ColorGradient>,
    start_depth: usize,
    solid_connector_gap: bool,
}

impl Default for TreeGuides {
    fn default() -> Self {
        Self::new(IndentStyle::None)
    }
}

impl TreeGuides {
    /// Guides in `style`, two cells per level, starting below the top-level rows.
    pub fn new(style: IndentStyle) -> Self {
        Self {
            style,
            indent: 2,
            guide_style: Style::default(),
            gradient: None,
            start_depth: 1,
            solid_connector_gap: false,
        }
    }

    /// Cells per level (default 2).
    pub fn indent(mut self, indent: u16) -> Self {
        self.indent = indent;
        self
    }

    /// Style of the guide characters.
    pub fn guide_style(mut self, style: Style) -> Self {
        self.guide_style = style;
        self
    }

    /// Color the guides by depth along a gradient, deepest at its end.
    pub fn gradient(mut self, gradient: ColorGradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// The shallowest depth that draws guides (default 1). Shallower rows are indented with blank
    /// cells instead.
    pub fn start_depth(mut self, depth: usize) -> Self {
        self.start_depth = depth;
        self
    }

    pub(crate) fn solid_connector_gap(mut self, solid: bool) -> Self {
        self.solid_connector_gap = solid;
        self
    }

    /// The indentation in front of a row at `branch`. `max_depth` is the deepest row on screen, which
    /// a [`gradient`](Self::gradient) spreads its colors across; it is ignored without one.
    pub fn spans(&self, branch: &TreeBranch, max_depth: usize) -> Vec<Span> {
        let mut spans = Vec::new();
        let depth = branch.depth;
        if self.style == IndentStyle::None || depth < self.start_depth.max(1) {
            let indent = self
                .indent
                .saturating_mul(depth.min(u16::MAX as usize) as u16);
            if indent > 0 {
                spans.push(Span::new(" ".repeat(indent as usize)));
            }
            return spans;
        }

        let gap = self.indent.saturating_sub(1);
        for level in 0..depth - 1 {
            if level + 1 >= self.start_depth
                && branch.continued.get(level).copied().unwrap_or(false)
            {
                spans.push(Span::new("│").style(self.style_at(level + 1, max_depth)));
            } else {
                spans.push(Span::new(" "));
            }
            if gap > 0 {
                spans.push(Span::new(" ".repeat(gap as usize)));
            }
        }

        let style = self.style_at(depth, max_depth);
        let connector = match (self.style, branch.last_child) {
            (IndentStyle::None, _) => "",
            (IndentStyle::Line, _) => "│",
            (IndentStyle::Short, false)
            | (IndentStyle::ShortRounded, false)
            | (IndentStyle::Long, false)
            | (IndentStyle::LongRounded, false) => match self.style {
                IndentStyle::Long | IndentStyle::LongRounded => "├─",
                _ => "├",
            },
            (IndentStyle::Short, true) => "└",
            (IndentStyle::ShortRounded, true) => "╰",
            (IndentStyle::Long, true) => "└─",
            (IndentStyle::LongRounded, true) => "╰─",
        };
        spans.push(Span::new(connector).style(style));

        let consumed = UnicodeWidthStr::width(connector) as u16;
        let gap = self.indent.saturating_sub(consumed);
        if gap > 0 {
            let solid = self.solid_connector_gap && self.style != IndentStyle::Line;
            if solid {
                spans.push(Span::new("─".repeat(gap as usize)).style(style));
            } else {
                spans.push(Span::new(" ".repeat(gap as usize)));
            }
        }
        spans
    }

    /// Cells the indentation in front of a row at `branch` occupies.
    pub fn width(&self, branch: &TreeBranch) -> u16 {
        self.spans(branch, branch.depth)
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>()
            .min(u16::MAX as usize) as u16
    }

    /// Guide style at `depth`, colored along the gradient when one is set.
    pub(crate) fn style_at(&self, depth: usize, max_depth: usize) -> Style {
        let mut style = self.guide_style;
        if let Some(gradient) = self.gradient {
            let t = if max_depth <= 1 {
                1.0
            } else {
                let depth_idx = depth.saturating_sub(1).min(max_depth.saturating_sub(1));
                depth_idx as f64 / max_depth.saturating_sub(1) as f64
            };
            style = style.patch(Style::new().fg(gradient.color_at(t)));
        }
        style
    }

    pub(crate) fn style(&self) -> IndentStyle {
        self.style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(guides: &TreeGuides, branch: &TreeBranch) -> String {
        guides
            .spans(branch, 4)
            .iter()
            .map(|span| span.content.to_string())
            .collect()
    }

    /// A guide continues through a row's column only for ancestors that still have siblings below,
    /// which is what makes a deep last child end its branch instead of trailing a stray line.
    #[test]
    fn guides_continue_only_through_open_ancestors() {
        let guides = TreeGuides::new(IndentStyle::Short).indent(2);
        let root = TreeBranch::root();
        let open = root.child(false);
        let closed = root.child(true);
        assert_eq!(text(&guides, &open.child(true)), "│ └ ");
        assert_eq!(text(&guides, &closed.child(false)), "  ├ ");
        assert_eq!(guides.width(&open.child(true)), 4);
    }

    #[test]
    fn long_rounded_connectors_fill_the_indent() {
        let guides = TreeGuides::new(IndentStyle::LongRounded).indent(3);
        let leaf = TreeBranch::root().child(true);
        assert_eq!(text(&guides, &leaf), "╰─ ");
    }

    /// Without a style, rows still indent by depth so a hierarchy stays readable.
    #[test]
    fn no_style_indents_with_blank_cells() {
        let guides = TreeGuides::new(IndentStyle::None).indent(2);
        let leaf = TreeBranch::root().child(false).child(true);
        assert_eq!(text(&guides, &leaf), "    ");
    }

    /// A start depth of zero must not underflow on a top-level row.
    #[test]
    fn a_zero_start_depth_draws_nothing_for_top_level_rows() {
        let guides = TreeGuides::new(IndentStyle::Short).start_depth(0);
        assert_eq!(text(&guides, &TreeBranch::root()), "");
    }
}
