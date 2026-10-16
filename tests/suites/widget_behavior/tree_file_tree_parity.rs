//! `Tree` and `FileTree` share one row layout: guides, the arrow column, the content gap, the icon
//! column, and the icon gap. The same hierarchy with the same options must draw the same grid.
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

#[derive(Clone, Copy, Debug)]
struct Options {
    guides: IndentStyle,
    indent: u16,
    /// `None` hides arrows; otherwise the expanded and collapsed arrows.
    arrows: Option<(&'static str, &'static str)>,
    /// `None` hides icons; otherwise the gap after them.
    icon_gap: Option<u16>,
    content_gap: u16,
}

const ICON_WIDTH: u16 = 3;

struct Files(Options);

impl Component for Files {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let options = self.0;
        let (expanded, collapsed) = options.arrows.unwrap_or(("▼", "▶"));
        FileTree::new("/p")
            .entry_source(FileTreeEntrySource::provided(vec![
                FileTreeDirectoryListing::new(
                    "/p",
                    [
                        FileTreeEntry::directory("a"),
                        FileTreeEntry::directory("b"),
                        FileTreeEntry::file("f.md"),
                    ],
                ),
                FileTreeDirectoryListing::new(
                    "/p/a",
                    [FileTreeEntry::directory("c"), FileTreeEntry::file("g.md")],
                ),
                FileTreeDirectoryListing::new("/p/a/c", [FileTreeEntry::file("h.md")]),
                FileTreeDirectoryListing::new("/p/b", [FileTreeEntry::file("i.md")]),
            ]))
            .initial_expanded_paths(["a", "a/c"])
            .icon_style(FileIconStyle::Text)
            .show_icons(options.icon_gap.is_some())
            .icon_gap(options.icon_gap.unwrap_or(1))
            .show_arrows(options.arrows.is_some())
            .expanded_arrow(expanded)
            .collapsed_arrow(collapsed)
            .indent_style(options.guides)
            .indent_width(options.indent)
            .content_gap(options.content_gap)
            .git_status(false)
            .focusable(false)
            .into()
    }
}

struct Rows(Options);

impl Component for Rows {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let options = self.0;
        let (expanded, collapsed) = options.arrows.unwrap_or(("▼", "▶"));
        let dir = |label: &'static str, open: bool, children: Vec<TreeNode>| {
            TreeNode::new(label)
                .icon(Text::new("[D]"))
                .expansion_mode(TreeExpansionMode::Collapsible)
                .expanded(open)
                .children(children)
        };
        let file = |label: &'static str| TreeNode::new(label).icon(Text::new("[F]"));
        Tree::new(dir(
            "/p",
            true,
            vec![
                dir(
                    "a",
                    true,
                    vec![dir("c", true, vec![file("h.md")]), file("g.md")],
                ),
                dir("b", false, vec![file("i.md")]),
                file("f.md"),
            ],
        ))
        .show_icons(options.icon_gap.is_some())
        .icon_width(ICON_WIDTH)
        .icon_gap(options.icon_gap.unwrap_or(1))
        .show_arrows(options.arrows.is_some())
        .expanded_arrow(expanded)
        .collapsed_arrow(collapsed)
        .indent_style(options.guides)
        .indent(options.indent)
        .content_gap(options.content_gap)
        .focusable(false)
        .into()
    }
}

fn grid<C: Component<Properties = ()>>(component: C) -> Vec<String> {
    let mut backend = TestBackend::new(component);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 8,
    });
    backend.render();
    backend
        .capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

#[test]
fn tree_and_file_tree_draw_the_same_rows_for_every_layout_option() {
    // Rounded variants share their square counterparts' geometry, so one of each pair suffices.
    let guides = [
        IndentStyle::None,
        IndentStyle::Line,
        IndentStyle::LineCappedRounded,
        IndentStyle::Branch,
    ];
    for guides in guides {
        for indent in [0, 1, 3] {
            for arrows in [None, Some(("▼", "▶")), Some(("[-]", "[+]"))] {
                for icon_gap in [None, Some(0), Some(2)] {
                    for content_gap in [0, 3] {
                        let options = Options {
                            guides,
                            indent,
                            arrows,
                            icon_gap,
                            content_gap,
                        };
                        let files = grid(Files(options));
                        let rows = grid(Rows(options));
                        assert_eq!(rows, files, "{options:?}");
                    }
                }
            }
        }
    }
}
