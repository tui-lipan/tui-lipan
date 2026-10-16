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
    /// Blank cells before each level's guide: where a parent had no arrow column, its children
    /// hang beneath its icon or label, one content gap after its branch.
    pub(crate) level_gaps: Vec<u16>,
}

impl TreeBranch {
    /// A top-level row.
    pub fn root() -> Self {
        Self {
            depth: 0,
            last_child: true,
            continued: Vec::new(),
            level_gaps: Vec::new(),
        }
    }

    /// A row directly under this one. `last` says it has no later sibling.
    pub fn child(&self, last: bool) -> Self {
        self.child_with_gap(last, 0)
    }

    pub(crate) fn child_with_gap(&self, last: bool, gap: u16) -> Self {
        let mut continued = self.continued.clone();
        continued.push(!last);
        let mut level_gaps = self.level_gaps.clone();
        level_gaps.push(gap);
        Self {
            depth: self.depth + 1,
            last_child: last,
            continued,
            level_gaps,
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
/// let guides = TreeGuides::new(IndentStyle::Branch).indent(2);
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
/// assert_eq!(text(&first), "├─");
/// assert_eq!(text(&last), "└─");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TreeGuides {
    style: IndentStyle,
    indent: u16,
    guide_style: Style,
    gradient: Option<ColorGradient>,
    start_depth: usize,
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

    /// The indentation in front of a row at `branch`. `max_depth` is the deepest row on screen, which
    /// a [`gradient`](Self::gradient) spreads its colors across; it is ignored without one.
    pub fn spans(&self, branch: &TreeBranch, max_depth: usize) -> Vec<Span> {
        self.render_levels(branch, max_depth, true)
    }

    /// The indentation for a row's second and later lines.
    ///
    /// Guides keep running through every ancestor that still has siblings below, and through the
    /// row's own column when it has a later sibling, so a multi-line row does not break the lines
    /// connecting what is above it to what is below. Same width as [`spans`](Self::spans).
    pub fn continuation_spans(&self, branch: &TreeBranch, max_depth: usize) -> Vec<Span> {
        self.render_levels(branch, max_depth, false)
    }

    fn render_levels(&self, branch: &TreeBranch, max_depth: usize, first_line: bool) -> Vec<Span> {
        if self.indent == 0 {
            return Vec::new();
        }
        let mut spans = Vec::with_capacity(branch.depth.saturating_mul(2));
        for level in 0..branch.depth {
            self.append_level(&mut spans, branch, level, max_depth, first_line);
        }
        spans
    }

    fn append_level(
        &self,
        spans: &mut Vec<Span>,
        branch: &TreeBranch,
        level: usize,
        max_depth: usize,
        first_line: bool,
    ) {
        let gap = branch.level_gaps.get(level).copied().unwrap_or(0);
        if gap > 0 {
            spans.push(Span::new(" ".repeat(gap as usize)));
        }
        let glyph = self.level_glyph(branch, level, first_line);
        let style = self.style_at(level + 1, max_depth);
        spans.push(Span::new(glyph).style(if glyph == " " {
            Style::default()
        } else {
            style
        }));
        let padding = self.indent - 1;
        if padding > 0 {
            let horizontal = first_line
                && level + 1 == branch.depth
                && level + 1 >= self.start_depth.max(1)
                && matches!(self.style, IndentStyle::Branch | IndentStyle::BranchRounded);
            let fill = if horizontal { "─" } else { " " };
            spans.push(
                Span::new(fill.repeat(padding as usize)).style(if horizontal {
                    style
                } else {
                    Style::default()
                }),
            );
        }
    }

    fn level_glyph(&self, branch: &TreeBranch, level: usize, first_line: bool) -> &'static str {
        if self.style == IndentStyle::None || level + 1 < self.start_depth.max(1) {
            return " ";
        }
        if first_line && level + 1 == branch.depth {
            return self.connector(branch.last_child);
        }
        if branch.continued.get(level).copied().unwrap_or(false) {
            "│"
        } else {
            " "
        }
    }

    fn connector(&self, last: bool) -> &'static str {
        match (self.style, last) {
            (IndentStyle::None, _) => " ",
            (IndentStyle::Line, _) => "│",
            (IndentStyle::LineCapped | IndentStyle::LineCappedRounded, false) => "│",
            (IndentStyle::LineCapped | IndentStyle::Branch, true) => "└",
            (IndentStyle::LineCappedRounded | IndentStyle::BranchRounded, true) => "╰",
            (IndentStyle::Branch | IndentStyle::BranchRounded, false) => "├",
        }
    }

    /// Fill an unused arrow column without changing indentation or its separator.
    pub(crate) fn arrow_placeholder_spans(
        &self,
        branch: &TreeBranch,
        max_depth: usize,
        cells: u16,
    ) -> Vec<Span> {
        if cells == 0 {
            return Vec::new();
        }
        let extend = self.indent > 0
            && branch.depth >= self.start_depth.max(1)
            && matches!(self.style, IndentStyle::Branch | IndentStyle::BranchRounded);
        if extend {
            vec![
                Span::new("─".repeat(cells as usize)).style(self.style_at(branch.depth, max_depth)),
            ]
        } else {
            vec![Span::new(" ".repeat(cells as usize))]
        }
    }

    /// Cells the indentation in front of a row at `branch` occupies.
    pub fn width(&self, branch: &TreeBranch) -> u16 {
        if self.indent == 0 {
            return 0;
        }
        branch.level_gaps.iter().fold(
            self.indent
                .saturating_mul(branch.depth.min(u16::MAX as usize) as u16),
            |width, gap| width.saturating_add(*gap),
        )
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
        let guides = TreeGuides::new(IndentStyle::Branch).indent(2);
        let root = TreeBranch::root();
        let open = root.child(false);
        let closed = root.child(true);
        assert_eq!(text(&guides, &open.child(true)), "│ └─");
        assert_eq!(text(&guides, &closed.child(false)), "  ├─");
        assert_eq!(guides.width(&open.child(true)), 4);
    }

    #[test]
    fn rounded_branches_fill_the_indent() {
        let guides = TreeGuides::new(IndentStyle::BranchRounded).indent(3);
        let leaf = TreeBranch::root().child(true);
        assert_eq!(text(&guides, &leaf), "╰──");
    }

    /// Without a style, rows still indent by depth so a hierarchy stays readable.
    #[test]
    fn no_style_indents_with_blank_cells() {
        let guides = TreeGuides::new(IndentStyle::None).indent(2);
        let leaf = TreeBranch::root().child(false).child(true);
        assert_eq!(text(&guides, &leaf), "    ");
    }

    /// A two-line row keeps the guides below it connected: its own column continues only while a
    /// later sibling follows, and the width matches the first line's.
    #[test]
    fn continuation_lines_keep_open_guides_running() {
        let guides = TreeGuides::new(IndentStyle::Branch).indent(2);
        let root = TreeBranch::root();
        let middle = root.child(false);
        let last = root.child(true);
        let cont = |branch: &TreeBranch| {
            guides
                .continuation_spans(branch, 4)
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };
        assert_eq!(cont(&middle), "│ ");
        assert_eq!(cont(&last), "  ");
        assert_eq!(cont(&middle.child(true)), "│   ");
        assert_eq!(cont(&root), "");
    }

    /// A start depth of zero must not underflow on a top-level row.
    #[test]
    fn a_zero_start_depth_draws_nothing_for_top_level_rows() {
        let guides = TreeGuides::new(IndentStyle::Branch).start_depth(0);
        assert_eq!(text(&guides, &TreeBranch::root()), "");
    }
    #[test]
    fn capped_lines_only_change_the_last_sibling() {
        let root = TreeBranch::root();
        let cases = [
            (IndentStyle::Line, "│  ", "│  "),
            (IndentStyle::LineCapped, "│  ", "└  "),
            (IndentStyle::LineCappedRounded, "│  ", "╰  "),
            (IndentStyle::Branch, "├──", "└──"),
            (IndentStyle::BranchRounded, "├──", "╰──"),
        ];
        for (style, middle, last) in cases {
            let guides = TreeGuides::new(style).indent(3);
            assert_eq!(text(&guides, &root.child(false)), middle);
            assert_eq!(text(&guides, &root.child(true)), last);
            assert_eq!(
                text(&guides, &root.child(false).child(true)),
                format!("│  {last}")
            );
            assert_eq!(
                text(&guides, &root.child(true).child(true)),
                format!("   {last}")
            );
        }
    }

    #[test]
    fn every_style_respects_the_width_on_first_and_continuation_lines() {
        use unicode_width::UnicodeWidthStr;
        for style in [
            IndentStyle::None,
            IndentStyle::Line,
            IndentStyle::LineCapped,
            IndentStyle::LineCappedRounded,
            IndentStyle::Branch,
            IndentStyle::BranchRounded,
        ] {
            for indent in 0..=4 {
                for start in 0..=4 {
                    let guides = TreeGuides::new(style).indent(indent).start_depth(start);
                    let mut branch = TreeBranch::root();
                    for depth in 0..=4 {
                        let first = text(&guides, &branch);
                        let rest: String = guides
                            .continuation_spans(&branch, 4)
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect();
                        let expected = indent as usize * depth;
                        assert_eq!(
                            first.width(),
                            expected,
                            "{style:?}, indent={indent}, start={start}, depth={depth}"
                        );
                        assert_eq!(rest.width(), expected);
                        assert_eq!(guides.width(&branch) as usize, expected);
                        branch = branch.child(depth % 2 == 0);
                    }
                }
            }
        }
    }

    #[test]
    fn start_depth_hides_all_parts_of_a_connector() {
        let guides = TreeGuides::new(IndentStyle::BranchRounded)
            .indent(3)
            .start_depth(2);
        let branch = TreeBranch::root().child(false);
        assert_eq!(text(&guides, &branch), "   ");
        assert_eq!(text(&guides, &branch.child(true)), "   ╰──");
    }
    #[test]
    fn only_visible_branches_extend_through_unused_disclosure_cells() {
        let branch = TreeBranch::root().child(true);
        let style = Style::new().bold();
        for (kind, expected) in [
            (IndentStyle::None, "   "),
            (IndentStyle::Line, "   "),
            (IndentStyle::LineCapped, "   "),
            (IndentStyle::LineCappedRounded, "   "),
            (IndentStyle::Branch, "───"),
            (IndentStyle::BranchRounded, "───"),
        ] {
            let guides = TreeGuides::new(kind).indent(1).guide_style(style);
            let spans = guides.arrow_placeholder_spans(&branch, 1, 3);
            let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
            assert_eq!(text, expected);
            if expected.starts_with('─') {
                assert_eq!(spans[0].style, style);
            }
            for guides in [guides.clone().indent(0), guides.clone().start_depth(2)] {
                let text: String = guides
                    .arrow_placeholder_spans(&branch, 1, 3)
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                assert_eq!(text, "   ");
            }
            assert!(guides.arrow_placeholder_spans(&branch, 1, 0).is_empty());
        }
    }
}
