//! FileTree and Tree side by side: one tab per widget, each with live controls for its options.
use std::collections::HashSet;

use tui_lipan::prelude::*;

struct FileTreeShowcase;

#[derive(Clone, Default)]
struct State {
    tab: usize,
    /// Hides the options column so the tree can take the full width, for example to copy rows.
    controls_hidden: bool,
    guide: usize,
    icon: usize,
    arrow: usize,
    theme: usize,
    indent_width: u16,
    content_gap: u16,
    icon_gap: u16,
    show_icons: bool,
    show_arrows: bool,
    git_status: bool,
    diff_stats: bool,
    git_icons: bool,
    git_status_padding_right: u16,
    custom_icons: bool,
    path_style: bool,
    last_path: Option<std::sync::Arc<str>>,
    tree: TreeOptions,
}

/// The Tree tab's options. Expansion is controlled here, by row path, so a folded row can tint
/// its arrow with the state of the rows it hides.
#[derive(Clone)]
struct TreeOptions {
    guide: usize,
    indent: u16,
    icons: bool,
    arrows: bool,
    arrow: usize,
    content_gap: u16,
    icon_gap: u16,
    tint_folded: bool,
    always_open: bool,
    root_headings: bool,
    details: bool,
    collapsed: HashSet<Vec<usize>>,
    activated: Option<String>,
}

impl Default for TreeOptions {
    fn default() -> Self {
        Self {
            guide: 4,
            indent: 2,
            icons: true,
            arrows: true,
            arrow: 0,
            content_gap: 1,
            icon_gap: 1,
            tint_folded: true,
            always_open: true,
            root_headings: false,
            details: false,
            collapsed: HashSet::from([vec![1]]),
            activated: None,
        }
    }
}

#[derive(Clone)]
enum Msg {
    SelectTab(usize),
    ToggleControls,
    NextGuide,
    NextIcon,
    NextArrow,
    NextTheme,
    NextIndentWidth,
    NextContentGap,
    NextIconGap,
    ToggleIcons,
    ToggleArrows,
    ToggleGit,
    ToggleDiffStats,
    ToggleGitIcons,
    NextGitStatusPadding,
    ToggleCustomIcons,
    TogglePathStyle,
    InsertPath(std::sync::Arc<str>),
    Tree(TreeTabMsg),
}

/// Messages from the Tree tab's controls and tree.
#[derive(Clone)]
enum TreeTabMsg {
    NextGuide,
    NextIndent,
    ToggleIcons,
    ToggleArrows,
    NextArrow,
    NextContentGap,
    NextIconGap,
    ToggleTintFolded,
    ToggleAlwaysOpen,
    ToggleRootHeadings,
    ToggleDetails,
    Toggled(Vec<usize>, bool),
    Activated(Vec<usize>),
}

const GUIDES: [IndentStyle; 6] = [
    IndentStyle::None,
    IndentStyle::Line,
    IndentStyle::LineCapped,
    IndentStyle::LineCappedRounded,
    IndentStyle::Branch,
    IndentStyle::BranchRounded,
];

const GUIDE_NAMES: [&str; 6] = [
    "None",
    "Line",
    "LineCapped",
    "LineCappedRounded",
    "Branch",
    "BranchRounded",
];

const ICONS: [FileIconStyle; 3] = [
    FileIconStyle::Text,
    FileIconStyle::NerdFont,
    FileIconStyle::NerdFontColored,
];

const ARROWS: [(&str, &str, &str); 4] = [
    ("Unicode", "▼", "▶"),
    ("Nerd Font", "", ""),
    ("ASCII", "v", ">"),
    ("Brackets", "[-]", "[+]"),
];

const ICON_NAMES: [&str; 3] = ["Text", "NerdFont", "NerdFontColored"];

impl Component for FileTreeShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            guide: 4,
            icon: 2,
            indent_width: 2,
            content_gap: 1,
            icon_gap: 1,
            show_icons: true,
            show_arrows: true,
            git_status: true,
            diff_stats: true,
            git_icons: false,
            custom_icons: false,
            path_style: false,
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::NextGuide => ctx.state.guide = (ctx.state.guide + 1) % GUIDES.len(),
            Msg::NextIcon => ctx.state.icon = (ctx.state.icon + 1) % ICONS.len(),
            Msg::NextArrow => ctx.state.arrow = (ctx.state.arrow + 1) % ARROWS.len(),
            Msg::NextTheme => ctx.state.theme = (ctx.state.theme + 1) % 3,
            Msg::NextIndentWidth => ctx.state.indent_width = (ctx.state.indent_width + 1) % 5,
            Msg::NextContentGap => ctx.state.content_gap = (ctx.state.content_gap + 1) % 4,
            Msg::NextIconGap => ctx.state.icon_gap = (ctx.state.icon_gap + 1) % 4,
            Msg::ToggleIcons => ctx.state.show_icons = !ctx.state.show_icons,
            Msg::ToggleArrows => ctx.state.show_arrows = !ctx.state.show_arrows,
            Msg::ToggleGit => ctx.state.git_status = !ctx.state.git_status,
            Msg::ToggleDiffStats => ctx.state.diff_stats = !ctx.state.diff_stats,
            Msg::ToggleGitIcons => ctx.state.git_icons = !ctx.state.git_icons,
            Msg::NextGitStatusPadding => {
                ctx.state.git_status_padding_right = (ctx.state.git_status_padding_right + 1) % 4;
            }
            Msg::ToggleCustomIcons => ctx.state.custom_icons = !ctx.state.custom_icons,
            Msg::TogglePathStyle => ctx.state.path_style = !ctx.state.path_style,
            Msg::InsertPath(path) => ctx.state.last_path = Some(path),
            Msg::SelectTab(tab) => ctx.state.tab = tab,
            Msg::ToggleControls => ctx.state.controls_hidden = !ctx.state.controls_hidden,
            Msg::Tree(msg) => update_tree(&mut ctx.state.tree, msg),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tabs = Tabs::new()
            .tab("FileTree")
            .tab("Tree")
            .active(ctx.state.tab)
            .on_change(
                ctx.link()
                    .callback(|event: TabsEvent| Msg::SelectTab(event.index)),
            );
        let toggle = control_button(if ctx.state.controls_hidden {
            "▸ Show options".into()
        } else {
            "◂ Hide options".into()
        })
        .on_click(ctx.link().callback(|_| Msg::ToggleControls))
        .automation_id("toggle-options");
        let bar = HStack::new()
            .height(Length::Px(1))
            .child(tabs.width(Length::Auto))
            .child(Spacer::new())
            .child(toggle);
        let body = match ctx.state.tab {
            0 => file_tree_tab(ctx),
            _ => tree_tab(ctx),
        };
        VStack::new().gap(1).child(bar).child(body).into()
    }
}

fn accent(state: &State) -> Color {
    match state.theme {
        0 => Color::LightCyan,
        1 => Color::LightGreen,
        _ => Color::LightMagenta,
    }
}

fn file_tree_tab(ctx: &Context<FileTreeShowcase>) -> Element {
    let state = &ctx.state;
    let accent = accent(state);
    let tree = FileTree::new("/demo/project")
        .entry_source(FileTreeEntrySource::provided(demo_listings()))
        .initial_expanded_paths(["src", "src/widgets", "examples", "docs"])
        .change_source(FileTreeChangeSource::Provided(demo_changes()))
        .show_hidden(true)
        .show_icons(state.show_icons)
        .show_arrows(state.show_arrows)
        .expanded_arrow(ARROWS[state.arrow].1)
        .collapsed_arrow(ARROWS[state.arrow].2)
        .arrow_style(Style::new().fg(accent))
        .content_gap(state.content_gap)
        .icon_gap(state.icon_gap)
        .icon_style(ICONS[state.icon])
        .indent_style(GUIDES[state.guide])
        .indent_guide_style(Style::new().fg(accent).dim())
        .directory_label_style(Style::new().fg(accent).bold())
        .file_label_style(Style::new().fg(Color::indexed(252)))
        .selection_style(Style::new().bg(Color::indexed(236)).fg(Color::White).bold())
        .change_suffix_style(Style::new().fg(accent).bold())
        .git_status(state.git_status)
        .git_icon_style(if state.git_icons {
            GitIconStyle::NerdFont
        } else {
            GitIconStyle::Text
        })
        .show_diff_stats(state.diff_stats)
        .git_status_padding_right(state.git_status_padding_right)
        .tree_focus_key("showcase-tree")
        .on_ctrl_activate(
            ctx.link()
                .callback(|event: FileTreeEvent| Msg::InsertPath(event.path)),
        )
        .indent_width(state.indent_width);

    let tree = if state.path_style {
        tree.path_style(
            "src/theme.rs",
            FileTreeItemStyle::new()
                .row(Style::new().bg(Color::indexed(235)))
                .icon(Style::new().fg(Color::LightYellow))
                .label(Style::new().fg(Color::LightYellow).bold()),
        )
    } else {
        tree
    };

    let tree = if state.custom_icons {
        tree.icon_override("rs", "󰌠", Some(Color::LightBlue))
            .icon_override("toml", "󰅐", Some(Color::LightYellow))
            .icon_override("widgets", "★", Some(Color::LightYellow))
    } else {
        tree
    };

    let controls = VStack::new()
        .gap(1)
        .width(Length::Px(34))
        .child(Text::new("FILE TREE CONTROLS").style(Style::new().fg(accent).bold()))
        .child(
            control_button(format!("Guides: {}", GUIDE_NAMES[state.guide]))
                .on_click(ctx.link().callback(|_| Msg::NextGuide))
                .automation_id("guide-style"),
        )
        .child(
            control_button(format!("Icons: {}", ICON_NAMES[state.icon]))
                .disabled(!state.show_icons)
                .on_click(ctx.link().callback(|_| Msg::NextIcon)),
        )
        .child(
            control_button(spacing_label("Indent", state.indent_width))
                .on_click(ctx.link().callback(|_| Msg::NextIndentWidth))
                .automation_id("indent-width"),
        )
        .child(
            control_button(toggle_label("File icons", state.show_icons))
                .on_click(ctx.link().callback(|_| Msg::ToggleIcons))
                .automation_id("file-icons"),
        )
        .child(
            control_button(toggle_label("Expand arrows", state.show_arrows))
                .on_click(ctx.link().callback(|_| Msg::ToggleArrows))
                .automation_id("expand-arrows"),
        )
        .child(
            control_button(format!("Arrow glyphs: {}", ARROWS[state.arrow].0))
                .disabled(!state.show_arrows)
                .on_click(ctx.link().callback(|_| Msg::NextArrow))
                .automation_id("arrow-glyphs"),
        )
        .child(
            control_button(spacing_label("Content gap", state.content_gap))
                .on_click(ctx.link().callback(|_| Msg::NextContentGap))
                .automation_id("content-gap"),
        )
        .child(
            control_button(spacing_label("Icon gap", state.icon_gap))
                .disabled(!state.show_icons)
                .on_click(ctx.link().callback(|_| Msg::NextIconGap))
                .automation_id("icon-gap"),
        )
        .child(
            control_button(toggle_label("Git status markers", state.git_status))
                .on_click(ctx.link().callback(|_| Msg::ToggleGit))
                .automation_id("git-status-markers"),
        )
        .child(
            control_button(toggle_label("Diff counts", state.diff_stats))
                .on_click(ctx.link().callback(|_| Msg::ToggleDiffStats))
                .automation_id("diff-counts"),
        )
        .child(
            control_button(toggle_label("Nerd Font Git marks", state.git_icons))
                .disabled(!state.git_status)
                .on_click(ctx.link().callback(|_| Msg::ToggleGitIcons))
                .automation_id("nerd-git-marks"),
        )
        .child(
            control_button(format!(
                "Git status right pad: {}",
                state.git_status_padding_right
            ))
            .disabled(!state.git_status)
            .on_click(ctx.link().callback(|_| Msg::NextGitStatusPadding))
            .automation_id("git-status-padding-right"),
        )
        .child(
            control_button(toggle_label(
                "Custom file/directory icons",
                state.custom_icons,
            ))
            .disabled(!state.show_icons)
            .on_click(ctx.link().callback(|_| Msg::ToggleCustomIcons)),
        )
        .child(
            control_button(toggle_label("Highlight src/theme.rs", state.path_style))
                .on_click(ctx.link().callback(|_| Msg::TogglePathStyle))
                .automation_id("highlight-theme-file"),
        )
        .child(
            control_button(format!("Theme accent: {}", state.theme + 1))
                .on_click(ctx.link().callback(|_| Msg::NextTheme)),
        );
    let header = HStack::new()
        .child(Text::new("/demo/project").style(Style::new().bold()))
        .child(Spacer::new())
        .child(Text::new(format!(
            "{} · {}",
            GUIDE_NAMES[state.guide], ICON_NAMES[state.icon]
        )))
        .height(Length::Px(1));

    with_controls(&ctx.state, controls)
        .child(
            VStack::new()
                .gap(1)
                .height(Length::Flex(1))
                .child(header)
                .child(tree.height(Length::Flex(1)))
                .child(
                    Text::new(
                        state
                            .last_path
                            .as_ref()
                            .map(|path| format!("Path: {path}"))
                            .unwrap_or_else(|| {
                                "Ctrl+click/Ctrl+Enter: use path · Enter/→ open · ← collapse".into()
                            }),
                    )
                    .style(Style::new().fg(Color::DarkGray)),
                ),
        )
        .into()
}

fn update_tree(tree: &mut TreeOptions, msg: TreeTabMsg) {
    match msg {
        TreeTabMsg::NextGuide => tree.guide = (tree.guide + 1) % GUIDES.len(),
        TreeTabMsg::NextIndent => tree.indent = (tree.indent + 1) % 5,
        TreeTabMsg::ToggleIcons => tree.icons = !tree.icons,
        TreeTabMsg::ToggleArrows => tree.arrows = !tree.arrows,
        TreeTabMsg::NextArrow => tree.arrow = (tree.arrow + 1) % ARROWS.len(),
        TreeTabMsg::NextContentGap => tree.content_gap = (tree.content_gap + 1) % 4,
        TreeTabMsg::NextIconGap => tree.icon_gap = (tree.icon_gap + 1) % 4,
        TreeTabMsg::ToggleTintFolded => tree.tint_folded = !tree.tint_folded,
        TreeTabMsg::ToggleAlwaysOpen => tree.always_open = !tree.always_open,
        TreeTabMsg::ToggleRootHeadings => tree.root_headings = !tree.root_headings,
        TreeTabMsg::ToggleDetails => tree.details = !tree.details,
        TreeTabMsg::Toggled(path, expanded) => {
            if expanded {
                tree.collapsed.remove(&path);
            } else {
                tree.collapsed.insert(path);
            }
        }
        TreeTabMsg::Activated(path) => {
            tree.activated = label_at(&demo_rows(), &path).map(str::to_string);
        }
    }
}

fn tree_tab(ctx: &Context<FileTreeShowcase>) -> Element {
    let options = &ctx.state.tree;
    let accent = accent(&ctx.state);
    let link = ctx.link();
    let button = |label: String, msg: TreeTabMsg| {
        control_button(label).on_click(link.callback(move |_| Msg::Tree(msg.clone())))
    };
    // The same options, in the same order, as the FileTree tab; then what only Tree has.
    let controls = VStack::new()
        .gap(1)
        .width(Length::Px(34))
        .child(Text::new("TREE CONTROLS").style(Style::new().fg(accent).bold()))
        .child(
            button(
                format!("Guides: {}", GUIDE_NAMES[options.guide]),
                TreeTabMsg::NextGuide,
            )
            .automation_id("tree-guide-style"),
        )
        .child(
            button(
                spacing_label("Indent", options.indent),
                TreeTabMsg::NextIndent,
            )
            .automation_id("tree-indent"),
        )
        .child(
            button(
                toggle_label("Row icons", options.icons),
                TreeTabMsg::ToggleIcons,
            )
            .automation_id("tree-row-icons"),
        )
        .child(
            button(
                toggle_label("Expand arrows", options.arrows),
                TreeTabMsg::ToggleArrows,
            )
            .automation_id("tree-expand-arrows"),
        )
        .child(
            button(
                format!("Arrow glyphs: {}", ARROWS[options.arrow].0),
                TreeTabMsg::NextArrow,
            )
            .disabled(!options.arrows)
            .automation_id("tree-arrow-glyphs"),
        )
        .child(
            button(
                spacing_label("Content gap", options.content_gap),
                TreeTabMsg::NextContentGap,
            )
            .automation_id("tree-content-gap"),
        )
        .child(
            button(
                spacing_label("Icon gap", options.icon_gap),
                TreeTabMsg::NextIconGap,
            )
            .disabled(!options.icons)
            .automation_id("tree-icon-gap"),
        )
        .child(Text::new("TREE ONLY").style(Style::new().fg(accent).bold()))
        .child(
            button(
                toggle_label("Tint folded arrows", options.tint_folded),
                TreeTabMsg::ToggleTintFolded,
            )
            .disabled(!options.arrows),
        )
        .child(button(
            toggle_label("Checkouts always open", options.always_open),
            TreeTabMsg::ToggleAlwaysOpen,
        ))
        .child(button(
            toggle_label("Roots as section headings", options.root_headings),
            TreeTabMsg::ToggleRootHeadings,
        ))
        .child(button(
            toggle_label("Detail lines", options.details),
            TreeTabMsg::ToggleDetails,
        ))
        .child(
            control_button(format!("Theme accent: {}", ctx.state.theme + 1))
                .on_click(ctx.link().callback(|_| Msg::NextTheme)),
        );

    let rows = demo_rows();
    let nodes = rows
        .iter()
        .enumerate()
        .map(|(index, row)| tree_node(row, vec![index], options, accent))
        .collect::<Vec<_>>();
    let tree = Tree::forest(nodes)
        .controlled_expansion(true)
        .toggle_on_activate(false)
        .indent_style(GUIDES[options.guide])
        .indent_guide_style(Style::new().fg(accent).dim())
        .indent(options.indent)
        .show_icons(options.icons)
        .icon_gap(options.icon_gap)
        .show_arrows(options.arrows)
        .expanded_arrow(ARROWS[options.arrow].1)
        .collapsed_arrow(ARROWS[options.arrow].2)
        .arrow_style(Style::new().fg(accent))
        .content_gap(options.content_gap)
        .selection_style(Style::new().bg(Color::indexed(236)).fg(Color::White).bold())
        .focus_key("showcase-rows")
        .on_toggle(ctx.link().callback(|event: TreeToggleEvent| {
            Msg::Tree(TreeTabMsg::Toggled(
                event.path.segments().to_vec(),
                event.expanded,
            ))
        }))
        .on_activate(ctx.link().callback(|event: TreeEvent| {
            Msg::Tree(TreeTabMsg::Activated(event.path.segments().to_vec()))
        }))
        .height(Length::Flex(1));
    let header = HStack::new()
        .child(Text::new("Agents and checkouts").style(Style::new().bold()))
        .child(Spacer::new())
        .child(Text::new(format!(
            "{} · indent {}",
            GUIDE_NAMES[options.guide], options.indent
        )))
        .height(Length::Px(1));
    let footer = options
        .activated
        .as_ref()
        .map(|label| format!("Activated: {label}"))
        .unwrap_or_else(|| "Enter/click: activate · arrow or Space/←/→: fold".into());

    with_controls(&ctx.state, controls)
        .child(
            VStack::new()
                .gap(1)
                .height(Length::Flex(1))
                .child(header)
                .child(tree)
                .child(Text::new(footer).style(Style::new().fg(Color::DarkGray))),
        )
        .into()
}

/// An agent's or session's state, most urgent first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Blocked,
    Working,
    Done,
    Current,
    Idle,
}

impl Status {
    fn color(self) -> Color {
        match self {
            Status::Blocked => Color::Yellow,
            Status::Working => Color::LightBlue,
            Status::Done | Status::Current => Color::LightGreen,
            Status::Idle => Color::DarkGray,
        }
    }

    /// The row icon: drawn in the column the tree's arrows use.
    fn icon(self) -> Element {
        let style = Style::new().fg(self.color());
        match self {
            Status::Working => Spinner::new().style(style).into(),
            Status::Blocked => Text::new("▲").style(style).into(),
            Status::Done => Text::new("✓").style(style).into(),
            Status::Current => Text::new("●").style(style).into(),
            Status::Idle => Text::new("○").style(style).into(),
        }
    }
}

struct Row {
    label: &'static str,
    status: Option<Status>,
    detail: Option<&'static str>,
    /// An icon for a row without a status, such as a checkout.
    marker: Option<&'static str>,
    /// A checkout: a group the "always open" control keeps open.
    group: bool,
    children: Vec<Row>,
}

/// A row's icon: its status, or its marker.
fn row_icon(row: &Row) -> Option<Element> {
    match (row.status, row.marker) {
        (Some(status), _) => Some(status.icon()),
        (None, Some(marker)) => Some(Text::new(marker).style(Style::new().fg(Color::Gray)).into()),
        (None, None) => None,
    }
}

fn row(label: &'static str) -> Row {
    Row {
        label,
        status: None,
        detail: None,
        marker: None,
        group: false,
        children: Vec::new(),
    }
}

impl Row {
    fn status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }

    fn detail(mut self, detail: &'static str) -> Self {
        self.detail = Some(detail);
        self
    }

    /// A checkout, which the "always open" control keeps open.
    fn group(mut self) -> Self {
        self.group = true;
        self.marker = Some("◇");
        self
    }

    fn marker(mut self, marker: &'static str) -> Self {
        self.marker = Some(marker);
        self
    }

    fn children(mut self, children: impl IntoIterator<Item = Row>) -> Self {
        self.children.extend(children);
        self
    }

    /// The most urgent state among the rows beneath this one.
    fn hidden_status(&self) -> Option<Status> {
        self.children
            .iter()
            .flat_map(|child| child.status.into_iter().chain(child.hidden_status()))
            .min()
    }
}

fn demo_rows() -> Vec<Row> {
    vec![
        row("rozi").marker("◈").children([
            row("master")
                .group()
                .detail("primary checkout")
                .children([row("dev")
                    .status(Status::Current)
                    .detail("Current · 3 panes")]),
            row("feat/login")
                .group()
                .detail("#114 · passed")
                .children([row("review")
                    .status(Status::Idle)
                    .detail("Background · 2 panes")]),
            row("Claude Code")
                .marker("◆")
                .detail("4 sessions")
                .children([
                    row("fix login redirect")
                        .status(Status::Blocked)
                        .detail("Approve edit to src/auth.rs?"),
                    row("github issue review")
                        .status(Status::Working)
                        .detail("Working · 14m"),
                    row("sidebar audit")
                        .status(Status::Done)
                        .detail("Done · 1h"),
                    row("notes").status(Status::Idle).detail("Idle"),
                ]),
            row("Codex")
                .status(Status::Working)
                .detail("Running cargo test"),
        ]),
        row("tui-lipan").marker("◈").children([
            row("main")
                .group()
                .children([row("docs").status(Status::Done).detail("Done")]),
            row("opencode")
                .status(Status::Blocked)
                .detail("Waiting for approval"),
        ]),
    ]
}

fn tree_node(row: &Row, path: Vec<usize>, options: &TreeOptions, accent: Color) -> TreeNode {
    // A section heading has no arrow column: it starts where its siblings' arrows would, and its
    // children hang from the end of its branch.
    let heading = options.root_headings && path.len() == 1;
    let label_style = if heading {
        Style::new().fg(accent).bold()
    } else {
        Style::default()
    };
    let mut node = TreeNode::new(ListItem::from_spans([
        Span::new(row.label).style(label_style)
    ]))
    .key(row.label);
    if options.details
        && let Some(detail) = row.detail
    {
        node = node
            .content(
                VStack::new()
                    .gap(0)
                    .child(Text::new(row.label).style(label_style))
                    .child(Text::new(detail).style(Style::new().fg(Color::DarkGray))),
            )
            .content_height(2);
    }
    if let Some(icon) = row_icon(row) {
        node = node.icon(icon);
    }
    if heading {
        node = node
            .expansion_mode(TreeExpansionMode::AlwaysOpen)
            .show_arrow(false);
    } else if row.group && options.always_open {
        node = node.expansion_mode(TreeExpansionMode::AlwaysOpen);
    } else if !row.children.is_empty() {
        let expanded = !options.collapsed.contains(&path);
        node = node.expanded(expanded);
        if options.tint_folded
            && !expanded
            && let Some(status) = row.hidden_status()
        {
            node = node.arrow_style(Style::new().fg(status.color()));
        }
    }
    node.children(row.children.iter().enumerate().map(|(index, child)| {
        let mut path = path.clone();
        path.push(index);
        tree_node(child, path, options, accent)
    }))
}

fn label_at(rows: &[Row], path: &[usize]) -> Option<&'static str> {
    let (first, rest) = path.split_first()?;
    let row = rows.get(*first)?;
    if rest.is_empty() {
        Some(row.label)
    } else {
        label_at(&row.children, rest)
    }
}

/// A tab's row: its options column and a divider, unless the options are hidden.
fn with_controls(state: &State, controls: VStack) -> HStack {
    let row = HStack::new().gap(0);
    if state.controls_hidden {
        return row;
    }
    row.child(controls)
        .child(Divider::vertical().style(Style::new().fg(Color::indexed(238))))
}

fn spacing_label(label: &str, cells: u16) -> String {
    let unit = if cells == 1 { "cell" } else { "cells" };
    format!("{label}: {cells} {unit}")
}

fn toggle_label(label: &str, enabled: bool) -> String {
    format!("[{}] {label}", if enabled { 'x' } else { ' ' })
}

fn control_button(label: String) -> Button {
    Button::filled(label)
        .style(Style::new().fg(Color::indexed(252)))
        .disabled_style(Style::new().fg(Color::indexed(241)).dim())
}

fn demo_listings() -> Vec<FileTreeDirectoryListing> {
    vec![
        FileTreeDirectoryListing::new(
            "/demo/project",
            [
                FileTreeEntry::directory("src"),
                FileTreeEntry::directory("examples"),
                FileTreeEntry::directory("docs"),
                FileTreeEntry::directory(".github"),
                FileTreeEntry::file("Cargo.toml"),
                FileTreeEntry::file("README.md"),
                FileTreeEntry::file("rust-toolchain.toml"),
                FileTreeEntry::file("current.log").symlink_target("logs/latest.log"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/src",
            [
                FileTreeEntry::directory("widgets"),
                FileTreeEntry::directory("app"),
                FileTreeEntry::file("lib.rs"),
                FileTreeEntry::file("main.rs"),
                FileTreeEntry::file("theme.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/src/widgets",
            [
                FileTreeEntry::directory("file_tree"),
                FileTreeEntry::file("tree.rs"),
                FileTreeEntry::file("file_tree.rs"),
                FileTreeEntry::file("button.rs"),
                FileTreeEntry::file("text_input.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/examples",
            [
                FileTreeEntry::file("file_tree_showcase.rs"),
                FileTreeEntry::file("lazygit.rs"),
                FileTreeEntry::file("showcase.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/docs",
            [
                FileTreeEntry::directory("widgets"),
                FileTreeEntry::file("components.md"),
                FileTreeEntry::file("styling.md"),
                FileTreeEntry::file("data.md"),
            ],
        ),
    ]
}

fn demo_changes() -> Vec<FileTreeChange> {
    vec![
        FileTreeChange::new("src/widgets/file_tree.rs", FileTreeChangeStatus::Modified)
            .kind(FileKind::File)
            .diff_stat(42, 11),
        FileTreeChange::new("src/lib.rs", FileTreeChangeStatus::Modified).kind(FileKind::File),
        FileTreeChange::new(
            "examples/file_tree_showcase.rs",
            FileTreeChangeStatus::Added,
        )
        .kind(FileKind::File)
        .staged(true),
        FileTreeChange::new("docs/data.md", FileTreeChangeStatus::Renamed).kind(FileKind::File),
        FileTreeChange::new("README.md", FileTreeChangeStatus::Untracked).kind(FileKind::File),
    ]
}

fn main() -> Result<()> {
    App::new()
        .title("FileTree and Tree Showcase")
        .mount(FileTreeShowcase)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    #[test]
    fn ctrl_activation_reports_paths_without_closing_demo_directories() {
        use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 110,
            h: 34,
        });
        backend.render();
        assert!(backend.focus_key(&"showcase-tree".into()));
        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: KeyMods::CTRL,
            })
            .unwrap();
        backend.render();
        assert_eq!(backend.state().last_path.as_deref(), Some("/demo/project"));
        let lines = backend.capture_frame().to_fixed_grid_lines();
        let (y, row) = lines
            .iter()
            .enumerate()
            .find(|(_, row)| row.trim_end().ends_with("docs"))
            .unwrap();
        let x = row[..row.find("docs").unwrap()].chars().count() as u16;
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x,
                    y: y as u16,
                    kind,
                    mods: KeyMods::CTRL,
                })
                .unwrap();
            backend.render();
        }
        assert_eq!(
            backend.state().last_path.as_deref(),
            Some("/demo/project/docs")
        );
        assert!(
            backend
                .capture_frame()
                .plain_text()
                .contains("components.md")
        );
    }

    #[test]
    fn git_status_padding_aligns_markers_after_diff_numbers() {
        for width in [80, 100] {
            let mut backend = TestBackend::new(FileTreeShowcase);
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 34,
            });
            backend.dispatch(Msg::ToggleGitIcons).unwrap();
            backend.dispatch(Msg::NextGitStatusPadding).unwrap();
            backend.render();
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let icon_only = lines.iter().find(|row| row.contains("lib.rs")).unwrap();
            let with_stats = lines
                .iter()
                .find(|row| row.contains("file_tree.rs"))
                .unwrap();
            assert!(with_stats.trim_end().ends_with("+42 -11 "), "{with_stats}");
            let icon_x = icon_only.chars().position(|ch| ch == '').unwrap();
            let stats_end = with_stats
                .chars()
                .enumerate()
                .filter(|(_, ch)| !ch.is_whitespace())
                .map(|(x, _)| x)
                .last()
                .unwrap();
            assert_eq!(
                icon_x, stats_end,
                "status icons align with and without diff counts"
            );
            assert_eq!(icon_only.chars().nth(icon_x + 1), Some(' '));
            let unpadded_x = icon_x + 1;
            for padding in [2, 3, 0] {
                backend.dispatch(Msg::NextGitStatusPadding).unwrap();
                backend.render();
                let lines = backend.capture_frame().to_fixed_grid_lines();
                for label in ["lib.rs", "file_tree.rs"] {
                    let row = lines.iter().find(|row| row.contains(label)).unwrap();
                    let marker_x = row.chars().position(|ch| ch == '').unwrap();
                    assert_eq!(marker_x + padding, unpadded_x);
                    assert!(
                        row.chars()
                            .skip(marker_x + 1)
                            .take(padding)
                            .all(|ch| ch == ' ')
                    );
                }
            }
            backend.dispatch(Msg::NextGitStatusPadding).unwrap();
            backend.dispatch(Msg::ToggleGit).unwrap();
            backend.render();
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let stats_only = lines
                .iter()
                .find(|row| row.contains("file_tree.rs"))
                .unwrap();
            assert!(
                stats_only.ends_with("+42 -11"),
                "stats-only rows have no trailing padding: {stats_only}"
            );
        }
    }

    #[test]
    fn live_controls_keep_arrows_independent_and_preserve_complete_rows() {
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 32,
        });
        backend.render();
        for message in [
            Msg::ToggleArrows,
            Msg::ToggleIcons,
            Msg::ToggleArrows,
            Msg::NextArrow,
            Msg::NextArrow,
            Msg::NextArrow,
            Msg::ToggleIcons,
            Msg::ToggleCustomIcons,
            Msg::ToggleGit,
            Msg::ToggleDiffStats,
            Msg::ToggleGit,
            Msg::ToggleDiffStats,
            Msg::TogglePathStyle,
            Msg::NextContentGap,
            Msg::NextContentGap,
            Msg::NextContentGap,
            Msg::NextIconGap,
            Msg::NextIconGap,
            Msg::NextIconGap,
        ] {
            backend.dispatch(message).unwrap();
            backend.render();
            let frame = backend.capture_ui_snapshot().frame;
            for cell in &frame.cells {
                assert!(
                    cell.symbol.chars().count() <= 1,
                    "unexpected cell {:?}",
                    cell.symbol
                );
            }
            let lines = frame.to_fixed_grid_lines();
            assert!(lines.iter().any(|line| line.contains("Expand arrows")));
            let changed_row = lines
                .iter()
                .find(|line| line.contains("file_tree.rs"))
                .unwrap();
            let suffix = changed_row.split_once("file_tree.rs").unwrap().1.trim();
            let expected = match (backend.state().git_status, backend.state().diff_stats) {
                (true, true) => "+42 -11 M",
                (true, false) => "M",
                (false, true) => "+42 -11",
                (false, false) => "",
            };
            assert_eq!(suffix, expected, "markers and diff counts are independent");
            let (y, line) = lines
                .iter()
                .enumerate()
                .find(|(_, line)| line.contains("components.md"))
                .unwrap();
            let x = line[..line.find("components.md").unwrap()].chars().count();
            let cells = &frame.row(y as u16)[x..x + "components.md".len()];
            assert!(
                cells.iter().all(|cell| cell.fg == cells[0].fg),
                "file labels must keep their color after a toggle"
            );
            assert_eq!(
                frame.plain_text().contains('▶'),
                backend.state().show_arrows && backend.state().arrow == 0
            );
        }
    }
    #[test]
    fn content_gap_control_remains_clickable_when_arrows_are_hidden() {
        use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 32,
        });
        backend.render();
        backend.dispatch(Msg::ToggleArrows).unwrap();
        backend.render();
        let snapshot = backend.capture_ui_snapshot();
        let button = snapshot
            .widgets
            .iter()
            .find(|widget| {
                widget
                    .label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Content gap:"))
            })
            .unwrap()
            .rect;
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: button.x as u16 + 1,
                    y: button.y as u16,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        backend.render();
        assert_eq!(backend.state().content_gap, 2);
        assert!(!backend.state().show_arrows);
    }

    fn tree_backend() -> TestBackend<FileTreeShowcase> {
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 34,
        });
        backend.dispatch(Msg::SelectTab(1)).unwrap();
        backend.render();
        backend
    }

    fn label_column(lines: &[String], label: &str) -> usize {
        let line = lines
            .iter()
            .find(|line| line.contains(label))
            .unwrap_or_else(|| panic!("{label} is on screen: {lines:#?}"));
        line[..line.find(label).unwrap()].chars().count()
    }

    #[test]
    fn tree_tab_keeps_sibling_labels_aligned_through_every_control() {
        let mut backend = tree_backend();
        for message in [
            None,
            Some(TreeTabMsg::NextIndent),
            Some(TreeTabMsg::NextContentGap),
            Some(TreeTabMsg::ToggleArrows),
            Some(TreeTabMsg::ToggleIcons),
            Some(TreeTabMsg::ToggleArrows),
            Some(TreeTabMsg::ToggleIcons),
            Some(TreeTabMsg::NextArrow),
            Some(TreeTabMsg::ToggleAlwaysOpen),
            Some(TreeTabMsg::ToggleAlwaysOpen),
            Some(TreeTabMsg::ToggleDetails),
            Some(TreeTabMsg::NextIconGap),
            Some(TreeTabMsg::ToggleRootHeadings),
            Some(TreeTabMsg::NextGuide),
            Some(TreeTabMsg::ToggleRootHeadings),
            Some(TreeTabMsg::NextGuide),
        ] {
            if let Some(message) = message {
                backend.dispatch(Msg::Tree(message)).unwrap();
                backend.render();
            }
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let checkout = label_column(&lines, "feat/login");
            for label in ["master", "Claude Code", "Codex"] {
                assert_eq!(label_column(&lines, label), checkout, "{label}: {lines:#?}");
            }
            assert_eq!(
                label_column(&lines, "tui-lipan"),
                label_column(&lines, "rozi")
            );
        }
    }

    /// Every level is one indent deeper than its parent, and its connector sits where the
    /// parent's branch ends, whatever the guides, arrows, and icons.
    #[test]
    fn tree_tab_children_sit_one_indent_deeper_than_their_parent() {
        for guide in 0..GUIDES.len() {
            for indent in 1..=4u16 {
                for (arrows, icons) in [(true, true), (true, false), (false, true), (false, false)]
                {
                    let mut backend = tree_backend();
                    let state = backend.state_mut();
                    state.tree.guide = guide;
                    state.tree.indent = indent;
                    state.tree.arrows = arrows;
                    state.tree.icons = icons;
                    backend.render();
                    let lines = backend.capture_frame().to_fixed_grid_lines();
                    let context = format!(
                        "{} indent={indent} arrows={arrows} icons={icons}: {lines:#?}",
                        GUIDE_NAMES[guide]
                    );
                    for (parent, child) in [
                        ("rozi", "master"),
                        ("master", "dev"),
                        ("Claude Code", "github issue review"),
                    ] {
                        // As in FileTree, a root with no arrow has nothing in front of it and so
                        // no content gap; its children, with guides in front, do.
                        let root_gap = if !arrows && parent == "rozi" { 1 } else { 0 };
                        assert_eq!(
                            label_column(&lines, child),
                            label_column(&lines, parent) + indent as usize + root_gap,
                            "{parent} → {child}, {context}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tree_tab_tints_a_folded_arrow_and_folds_by_clicking_it() {
        let mut backend = tree_backend();
        let lines = backend.capture_frame().to_fixed_grid_lines();
        let (y, line) = lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("tui-lipan"))
            .unwrap();
        let arrow = line
            .find('▶')
            .map(|byte| line[..byte].chars().count())
            .unwrap();
        let frame = backend.capture_frame();
        assert_eq!(frame.cell(arrow as u16, y as u16).fg, Color::Yellow);

        let lines = frame.to_fixed_grid_lines();
        let (y, line) = lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("Claude Code"))
            .unwrap();
        let x = line
            .find('▼')
            .map(|byte| line[..byte].chars().count())
            .unwrap();
        use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: x as u16,
                    y: y as u16,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        backend.render();
        let text = backend.capture_frame().plain_text();
        assert!(!text.contains("fix login redirect"), "{text}");
        assert!(backend.state().tree.collapsed.contains(&vec![0, 2]));
        assert_eq!(
            backend.state().tree.activated,
            None,
            "the arrow folds, not activates"
        );
    }

    #[test]
    fn tree_tab_labels_activate_rows_when_clicked() {
        use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
        for details in [false, true] {
            let mut backend = tree_backend();
            if details {
                backend
                    .dispatch(Msg::Tree(TreeTabMsg::ToggleDetails))
                    .unwrap();
                backend.render();
            }
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let (y, line) = lines
                .iter()
                .enumerate()
                .find(|(_, line)| line.contains("Codex"))
                .unwrap();
            let x = line[..line.find("Codex").unwrap()].chars().count() + 2;
            for kind in [
                MouseKind::Down(MouseButton::Left),
                MouseKind::Up(MouseButton::Left),
            ] {
                backend
                    .send_mouse(MouseEvent {
                        x: x as u16,
                        y: y as u16,
                        kind,
                        mods: KeyMods::NONE,
                    })
                    .unwrap();
            }
            backend.render();
            assert_eq!(
                backend.state().tree.activated.as_deref(),
                Some("Codex"),
                "details={details}"
            );
        }
    }

    #[test]
    fn options_column_hides_and_returns_on_both_tabs() {
        for (tab, title, content) in [
            (0, "FILE TREE CONTROLS", "components.md"),
            (1, "TREE CONTROLS", "Claude Code"),
        ] {
            let mut backend = TestBackend::new(FileTreeShowcase);
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 34,
            });
            backend.dispatch(Msg::SelectTab(tab)).unwrap();
            backend.render();
            assert!(backend.capture_frame().plain_text().contains(title));
            backend.dispatch(Msg::ToggleControls).unwrap();
            backend.render();
            let text = backend.capture_frame().plain_text();
            assert!(!text.contains(title), "{text}");
            assert!(
                text.contains(content) && text.contains("Show options"),
                "{text}"
            );
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let tree_top = lines
                .iter()
                .find(|line| line.contains(if tab == 0 { "/demo/project" } else { "rozi" }))
                .unwrap();
            assert!(
                tree_top.chars().take(4).any(|ch| !ch.is_whitespace()),
                "the tree takes the full width: {tree_top:?}"
            );
            backend.dispatch(Msg::ToggleControls).unwrap();
            backend.render();
            assert!(backend.capture_frame().plain_text().contains(title));
        }
    }
}
