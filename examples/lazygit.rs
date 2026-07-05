//! Lazygit-style TUI mock demonstrating tui-lipan's layout + focus.
//!
//! Goals:
//! - Multi-panel layout (lazygit-like)
//! - Only the focused pane is active_index
//! - Bottom hint bar (1 line) changes by active pane
//! - Number keys jump focus to panes
//! - Compact mode: non-focused panes collapse when space is tight

use tui_lipan::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Mock Data
// ─────────────────────────────────────────────────────────────────────────────

const MOCK_FILES: &[(&str, &str)] = &[
    ("M ", "src/app.rs"),
    ("M ", "src/layout.rs"),
    ("A ", "examples/lazygit.rs"),
    ("D ", "src/old_module.rs"),
    ("??", "TODO.md"),
    ("M ", "Cargo.toml"),
];

const MOCK_BRANCHES: &[(&str, bool)] = &[
    ("master", true),
    ("feature/lazygit-clone", false),
    ("fix/scrollbar-jitter", false),
    ("refactor/layout-engine", false),
];

const MOCK_REMOTES: &[&str] = &["origin/master", "origin/develop", "upstream/master"];

const MOCK_TAGS: &[&str] = &["v0.1.0", "v0.2.0", "v0.3.0"];

const MOCK_WORKTREES: &[&str] = &[
    "/home/user/work/tui-lipan (main)",
    "/tmp/tui-lipan-fix (fix)",
];

const MOCK_SUBMODULES: &[&str] = &["vendor/ratatui (v0.24.0)", "vendor/crossterm (v0.27.0)"];

const MOCK_COMMITS: &[(&str, &str)] = &[
    ("a1b2c3d", "feat: Add focus request API"),
    ("e4f5g6h", "fix: Layout remainder gaps"),
    ("i7j8k9l", "fix: Scrollbar drag in lists"),
    ("m0n1o2p", "ui: Lazygit mock example"),
    ("q3r4s5t", "feat: Responsive breakpoints"),
    ("u6v7w8x", "fix: Focus traversal edge case"),
    ("y9z0a1b", "chore: Bump dependencies"),
    ("c2d3e4f", "feat: Add Frame widget"),
    ("d4e5f6g", "refactor: Abstract layout logic"),
    ("h7i8j9k", "perf: Optimize rendering path"),
    ("l0m1n2o", "docs: Improve API documentation"),
    ("p3q4r5s", "test: Add panel focus tests"),
    ("t6u7v8w", "chore: Update CI workflow"),
    ("x9y0z1a", "feat: Add mouse support"),
    ("b2c3d4e", "fix: List selection overflow"),
    ("f5g6h7i", "feat: Inline error reporting"),
    ("j8k9l0m", "style: Cleanup formatting"),
    ("n1o2p3q", "feat: Add multi-select support"),
];

const MOCK_REFLOG: &[&str] = &[
    "HEAD@{0}: checkout: moving from feature/foo to master",
    "HEAD@{1}: commit: feat: Add focus request API",
    "HEAD@{2}: commit: fix: Layout remainder gaps",
];

const MOCK_STASH: &[&str] = &["stash@{0}: WIP on master: a1b2c3d", "stash@{1}: WIP on fix"];

const MOCK_COMMAND_LOG: &[&str] = &["git status", "git log --oneline -n10", "git diff --stat"];

const MOCK_LOG: &[&str] = &[
    "* a1b2c3d (HEAD -> master, origin/master) feat: Add focus request API",
    "* e4f5g6h fix: Layout remainder gaps",
    "* i7j8k9l fix: Scrollbar drag in lists",
    "| * m0n1o2p (feature/lazygit-clone) ui: Lazygit mock example",
    "| * q3r4s5t feat: Responsive breakpoints",
    "|/  ",
    "* u6v7w8x fix: Focus traversal edge case",
    "* y9z0a1b chore: Bump dependencies",
    "* c2d3e4f feat: Add Frame widget",
    "* d4e5f6g refactor: Abstract layout logic",
    "* h7i8j9k perf: Optimize rendering path",
    "* l0m1n2o docs: Improve API documentation",
    "* p3q4r5s test: Add panel focus tests",
    "* t6u7v8w chore: Update CI workflow",
    "* x9y0z1a feat: Add mouse support",
    "* b2c3d4e fix: List selection overflow",
    "* f5g6h7i feat: Inline error reporting",
    "* j8k9l0m style: Cleanup formatting",
    "* n1o2p3q feat: Add multi-select support",
    "| * r4s5t6u (feature/merge-ui) feat: Add merge UI flow",
    "| * t7u8v9w fix: Merge conflict resolution bug",
    "| * z1x2c3v (feature/checkout-improvements) feat: Fast-forward checkout speed",
    "|\\  ",
    "* e8f9g0h (hotfix/urgent-bug, origin/hotfix/urgent-bug) fix: Hot crash on launch",
    "* i9j0k1l chore: Dependency security audit",
    "| * q1w2e3r (release/v1.0, origin/release/v1.0) chore: Prepare v1.0 release",
    "|/  ",
    "* m9n8b7v feat: Stash UI improvements",
    "* k2j3h4g (bugfix/typo) fix: Typo in branch output",
    "* w4e5r6t docs: Add usage examples",
    "| * x8c7v6b (feature/auth, origin/feature/auth) feat: Integrate OAuth sign-in",
    "|\\  ",
    "* n8m7l6k feat: New CLI flags",
    "* g5f4d3s (HEAD -> develop, origin/develop) fix: Resolve stuttering redraws",
    "* j2k3l4m (feature/perf, origin/feature/perf) perf: Boost redraw efficiency",
    "| * p6o7i8u (feature/ui-theme) feat: Add dark mode",
    "|/  ",
    "* b3n4m5l docs: Update README with badges",
    "* r7t8y9u feat: Drag-to-reorder panels",
    "* e7r6t5y refactor: Generalize panel layout component",
    "* z9x8c7v test: Add integration tests for focus",
    "* l9m8n7b chore: Update .gitignore",
];

// ─────────────────────────────────────────────────────────────────────────────
// Panes
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pane {
    Log,
    Status,
    Files,
    Branches,
    Commits,
    Stash,
    CommandLog,
}

impl Pane {
    const ALL: &'static [Pane] = &[
        Pane::Log,
        Pane::Status,
        Pane::Files,
        Pane::Branches,
        Pane::Commits,
        Pane::Stash,
        Pane::CommandLog,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Status => "Status",
            Pane::Files => "Files",
            Pane::Branches => "Local Branches",
            Pane::Commits => "Commits",
            Pane::Stash => "Stash",
            Pane::Log => "Log",
            Pane::CommandLog => "Command Log",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Pane::Status => "pane-status",
            Pane::Files => "pane-files",
            Pane::Branches => "pane-branches",
            Pane::Commits => "pane-commits",
            Pane::Stash => "pane-stash",
            Pane::Log => "pane-log",
            Pane::CommandLog => "pane-cmd",
        }
    }

    fn number(self) -> char {
        match self {
            Pane::Log => '0',
            Pane::Status => '1',
            Pane::Files => '2',
            Pane::Branches => '3',
            Pane::Commits => '4',
            Pane::Stash => '5',
            Pane::CommandLog => '6',
        }
    }

    fn from_number(c: char) -> Option<Self> {
        match c {
            '0' => Some(Pane::Log),
            '1' => Some(Pane::Status),
            '2' => Some(Pane::Files),
            '3' => Some(Pane::Branches),
            '4' => Some(Pane::Commits),
            '5' => Some(Pane::Stash),
            '6' => Some(Pane::CommandLog),
            _ => None,
        }
    }
}

const HINT_BAR_HEIGHT: u16 = 1;
const FOCUSED_PANEL_MIN_HEIGHT: u16 = 7;

fn build_log_elements() -> Vec<Element> {
    MOCK_LOG
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let element = if let Some((prefix, rest)) = line.split_once('(') {
                if let Some((middle, suffix)) = rest.split_once(')') {
                    Element::from(Text::from_spans(vec![
                        Span::new(prefix).fg(Color::Yellow),
                        Span::new(format!("({})", middle)).fg(Color::LightCyan),
                        Span::new(suffix).fg(Color::DarkGray),
                    ]))
                } else {
                    let style = if line.starts_with('*') {
                        Style::new().fg(Color::Yellow)
                    } else {
                        Style::new().fg(Color::DarkGray)
                    };
                    Element::from(Text::new(*line).style(style))
                }
            } else {
                let style = if line.starts_with('*') {
                    Style::new().fg(Color::Yellow)
                } else {
                    Style::new().fg(Color::DarkGray)
                };
                Element::from(Text::new(*line).style(style))
            };
            element.key(format!("log-{}", i))
        })
        .collect()
}

fn panel_chrome_style() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn panel_focus_style() -> Style {
    Style::new().fg(Color::LightCyan)
}

fn panel_focus_title_style() -> Style {
    panel_focus_style().bold()
}

fn panel_list_selection_style() -> Style {
    Style::new().bg(Color::rgb(0x2A, 0x2F, 0x39))
}

fn panel_frame() -> Frame {
    Frame::new()
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(panel_chrome_style())
        .focus_style(panel_focus_style())
        .title_style(panel_chrome_style())
        .header_padding(1)
        .footer_padding(1)
        .focus_title_style(panel_focus_title_style())
        .status_style(panel_chrome_style())
        .focus_status_style(panel_focus_style())
        .padding(0)
}

fn numbered_panel_frame(pane: Pane) -> Frame {
    panel_frame().title_prefix(format!("[{}]", pane.number()))
}

fn panel_list(
    items: impl IntoIterator<Item = ListItem>,
    selected: usize,
    on_select: Callback<ListEvent>,
) -> Element {
    List::new()
        .items(items)
        .selected(selected)
        .selection_style(panel_list_selection_style())
        .selection_symbol(None::<&str>)
        .selection_full_width(true)
        .on_select(on_select)
        .padding(0)
        .scrollbar(true)
        .scrollbar_config(
            ScrollbarConfig::new()
                .variant(ScrollbarVariant::Integrated)
                .thumb('▐')
                .thumb_style(panel_chrome_style())
                .thumb_focus_style(panel_focus_style()),
        )
        .into()
}

// ─────────────────────────────────────────────────────────────────────────────
// Component
// ─────────────────────────────────────────────────────────────────────────────

struct LazygitDemo;

struct State {
    files_selected: usize,
    worktrees_selected: usize,
    submodules_selected: usize,
    files_tab: usize,

    branches_selected: usize,
    remotes_selected: usize,
    tags_selected: usize,
    branches_tab: usize,

    commits_selected: usize,
    reflog_selected: usize,
    commits_tab: usize,

    stash_selected: usize,
    cmd_selected: usize,
    log_scroll_offset: usize,

    /// Pre-computed log elements; built once at startup, cloned cheaply per frame.
    log_elements: Vec<Element>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            files_selected: 0,
            worktrees_selected: 0,
            submodules_selected: 0,
            files_tab: 0,
            branches_selected: 0,
            remotes_selected: 0,
            tags_selected: 0,
            branches_tab: 0,
            commits_selected: 0,
            reflog_selected: 0,
            commits_tab: 0,
            stash_selected: 0,
            cmd_selected: 0,
            log_scroll_offset: 0,
            log_elements: build_log_elements(),
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    FilesSelected(ListEvent),
    WorktreesSelected(ListEvent),
    SubmodulesSelected(ListEvent),
    FilesTabChanged(TabsEvent),

    BranchesSelected(ListEvent),
    RemotesSelected(ListEvent),
    TagsSelected(ListEvent),
    BranchesTabChanged(TabsEvent),

    CommitsSelected(ListEvent),
    ReflogSelected(ListEvent),
    CommitsTabChanged(TabsEvent),

    StashSelected(ListEvent),
    CmdSelected(ListEvent),
    LogScroll(ScrollEvent),
}

impl Component for LazygitDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let viewport = ctx.viewport();
        if viewport.w < 30 || viewport.h < 8 {
            return rsx! {
                Center {
                    VStack {
                        align: Align::Center,
                        border: true,
                        Text {
                            content: "Terminal Too Small",
                            style: Style::new().fg(Color::Red).bold(),
                        },
                        Spacer { height: Length::Px(1) },
                        Text {
                            content: format!("Required: 30x8"),
                            style: Style::new().fg(Color::Gray),
                        },
                        Text {
                            content: format!("Current:  {}x{}", viewport.w, viewport.h),
                            style: Style::new().fg(Color::DarkGray),
                        },
                    },
                }
            };
        }

        let bp = ctx.breakpoint(80, 140);
        let active_pane = Pane::ALL
            .iter()
            .copied()
            .find(|p| ctx.has_focus_within_key(p.key()))
            .unwrap_or(Pane::Files);

        let count_status = |selected: usize, len: usize| {
            if len == 0 {
                "0".to_string()
            } else {
                format!("{} of {}", selected.saturating_add(1).min(len), len)
            }
        };

        let files_items: Vec<ListItem> = MOCK_FILES
            .iter()
            .map(|(status, name)| {
                let color = match *status {
                    "M " => Color::Yellow,
                    "A " => Color::LightGreen,
                    "D " => Color::LightRed,
                    "??" => Color::LightMagenta,
                    _ => Color::White,
                };
                ListItem::from_spans(vec![Span::new(*status).fg(color), Span::new(*name)])
            })
            .collect();

        let worktrees_items: Vec<ListItem> = MOCK_WORKTREES
            .iter()
            .map(|name| ListItem::new(*name))
            .collect();

        let submodules_items: Vec<ListItem> = MOCK_SUBMODULES
            .iter()
            .map(|name| ListItem::new(*name))
            .collect();

        let branches_items: Vec<ListItem> = MOCK_BRANCHES
            .iter()
            .map(|(name, is_current)| {
                let prefix = if *is_current { "* " } else { "  " };
                let color = if *is_current {
                    Color::LightGreen
                } else {
                    Color::White
                };
                ListItem::new(format!("{}{}", prefix, name)).style(Style::new().fg(color))
            })
            .collect();

        let remotes_items: Vec<ListItem> = MOCK_REMOTES
            .iter()
            .map(|name| ListItem::new(format!("  {}", name)))
            .collect();

        let tags_items: Vec<ListItem> = MOCK_TAGS.iter().map(|t| ListItem::new(*t)).collect();

        let commits_items: Vec<ListItem> = MOCK_COMMITS
            .iter()
            .map(|(hash, msg)| {
                ListItem::from_spans(vec![
                    Span::new(*hash).fg(Color::Yellow),
                    Span::new(format!(" {}", msg)),
                ])
            })
            .collect();

        let reflog_items: Vec<ListItem> = MOCK_REFLOG.iter().map(|l| ListItem::new(*l)).collect();

        let stash_items: Vec<ListItem> = if MOCK_STASH.is_empty() {
            vec![ListItem::new("(empty)").style(Style::new().fg(Color::DarkGray))]
        } else {
            MOCK_STASH.iter().map(|s| ListItem::new(*s)).collect()
        };

        let cmd_items: Vec<ListItem> = MOCK_COMMAND_LOG.iter().map(|l| ListItem::new(*l)).collect();

        let files_len = match ctx.state.files_tab {
            0 => files_items.len(),
            1 => worktrees_items.len(),
            _ => submodules_items.len(),
        };
        let branches_len = match ctx.state.branches_tab {
            0 => branches_items.len(),
            1 => remotes_items.len(),
            _ => tags_items.len(),
        };
        let commits_len = match ctx.state.commits_tab {
            0 => commits_items.len(),
            _ => reflog_items.len(),
        };
        let stash_len = stash_items.len();

        let status_panel: Element = numbered_panel_frame(Pane::Status)
            .height(Length::Auto)
            .focus_min_height(3)
            .title(Pane::Status.title())
            .focusable(true)
            .child(rsx! {
                VStack {
                    gap: 0,
                    Text {
                        content: "My Project -> master",
                        style: Style::new().fg(Color::LightGreen).bold(),
                    },
                }
            })
            .into();
        let status_panel = status_panel.key(Pane::Status.key());

        let files_panel_body = match ctx.state.files_tab {
            0 => panel_list(
                files_items.clone(),
                ctx.state.files_selected,
                ctx.link().callback(Msg::FilesSelected),
            ),
            1 => panel_list(
                worktrees_items.clone(),
                ctx.state.worktrees_selected,
                ctx.link().callback(Msg::WorktreesSelected),
            ),
            _ => panel_list(
                submodules_items.clone(),
                ctx.state.submodules_selected,
                ctx.link().callback(Msg::SubmodulesSelected),
            ),
        };
        let files_panel: Element = numbered_panel_frame(Pane::Files)
            .tab_titles(["Files", "Worktrees", "Submodules"])
            .tab_variant(TabVariant::Minimal)
            .active_tab(ctx.state.files_tab)
            .active_tab_style(panel_focus_style())
            .focus_active_tab_style(panel_focus_title_style())
            .on_tab_change(ctx.link().callback(Msg::FilesTabChanged))
            .status_right(count_status(
                match ctx.state.files_tab {
                    0 => ctx.state.files_selected,
                    1 => ctx.state.worktrees_selected,
                    _ => ctx.state.submodules_selected,
                },
                files_len,
            ))
            .height(Length::Flex(1))
            .focus_min_height(FOCUSED_PANEL_MIN_HEIGHT)
            .width(Length::Flex(1))
            .focusable(false)
            .child(files_panel_body)
            .into();
        let files_panel = files_panel.key(Pane::Files.key());

        let branches_panel_body = match ctx.state.branches_tab {
            0 => panel_list(
                branches_items.clone(),
                ctx.state.branches_selected,
                ctx.link().callback(Msg::BranchesSelected),
            ),
            1 => panel_list(
                remotes_items.clone(),
                ctx.state.remotes_selected,
                ctx.link().callback(Msg::RemotesSelected),
            ),
            _ => panel_list(
                tags_items.clone(),
                ctx.state.tags_selected,
                ctx.link().callback(Msg::TagsSelected),
            ),
        };
        let branches_panel: Element = numbered_panel_frame(Pane::Branches)
            .tab_titles(["Branches", "Remotes", "Tags"])
            .tab_variant(TabVariant::Minimal)
            .active_tab(ctx.state.branches_tab)
            .active_tab_style(panel_focus_style())
            .focus_active_tab_style(panel_focus_title_style())
            .on_tab_change(ctx.link().callback(Msg::BranchesTabChanged))
            .status_right(count_status(
                match ctx.state.branches_tab {
                    0 => ctx.state.branches_selected,
                    1 => ctx.state.remotes_selected,
                    _ => ctx.state.tags_selected,
                },
                branches_len,
            ))
            .height(Length::Flex(1))
            .focus_min_height(FOCUSED_PANEL_MIN_HEIGHT)
            .width(Length::Flex(1))
            .focusable(false)
            .child(branches_panel_body)
            .into();
        let branches_panel = branches_panel.key(Pane::Branches.key());

        let commits_panel_body = match ctx.state.commits_tab {
            0 => panel_list(
                commits_items.clone(),
                ctx.state.commits_selected,
                ctx.link().callback(Msg::CommitsSelected),
            ),
            _ => panel_list(
                reflog_items.clone(),
                ctx.state.reflog_selected,
                ctx.link().callback(Msg::ReflogSelected),
            ),
        };
        let commits_panel: Element = numbered_panel_frame(Pane::Commits)
            .tab_titles(["Commits", "Reflog"])
            .tab_variant(TabVariant::Minimal)
            .active_tab(ctx.state.commits_tab)
            .active_tab_style(panel_focus_style())
            .focus_active_tab_style(panel_focus_title_style())
            .on_tab_change(ctx.link().callback(Msg::CommitsTabChanged))
            .status_right(count_status(
                match ctx.state.commits_tab {
                    0 => ctx.state.commits_selected,
                    _ => ctx.state.reflog_selected,
                },
                commits_len,
            ))
            .height(Length::Flex(1))
            .focus_min_height(FOCUSED_PANEL_MIN_HEIGHT)
            .width(Length::Flex(1))
            .focusable(false)
            .child(commits_panel_body)
            .into();
        let commits_panel = commits_panel.key(Pane::Commits.key());

        let stash_panel: Element = numbered_panel_frame(Pane::Stash)
            .title(Pane::Stash.title())
            .status_right(count_status(ctx.state.stash_selected, stash_len))
            .height(Length::Flex(1))
            .unfocused_height(Length::Px(3))
            .focus_min_height(4)
            .width(Length::Flex(1))
            .focusable(false)
            .child(panel_list(
                stash_items.clone(),
                ctx.state.stash_selected,
                ctx.link().callback(Msg::StashSelected),
            ))
            .into();
        let stash_panel = stash_panel.key(Pane::Stash.key());

        let log_panel: Element = numbered_panel_frame(Pane::Log)
            .title(Pane::Log.title())
            .height(Length::Flex(2))
            .focus_min_height(FOCUSED_PANEL_MIN_HEIGHT)
            .width(Length::Flex(1))
            .focusable(true)
            .child(
                ScrollView::new()
                    .scrollbar(true)
                    .scrollbar_config(
                        ScrollbarConfig::new()
                            .variant(ScrollbarVariant::Integrated)
                            .thumb('▐')
                            .thumb_style(panel_chrome_style())
                            .thumb_focus_style(panel_focus_style()),
                    )
                    .scroll_keys(ScrollKeymap::DEFAULT)
                    .focusable(true)
                    .offset(ctx.state.log_scroll_offset)
                    .on_scroll(ctx.link().callback(Msg::LogScroll))
                    .gap(0)
                    .children(ctx.state.log_elements.iter().cloned()),
            )
            .into();
        let log_panel = log_panel.key(Pane::Log.key());

        let cmd_panel: Element = panel_frame()
            .title(Pane::CommandLog.title())
            .height(Length::Flex(1))
            .unfocused_height(Length::Px(3))
            .focus_min_height(FOCUSED_PANEL_MIN_HEIGHT)
            .width(Length::Flex(1))
            .focusable(true)
            .child(panel_list(
                cmd_items.clone(),
                ctx.state.cmd_selected,
                ctx.link().callback(Msg::CmdSelected),
            ))
            .into();
        let cmd_panel = cmd_panel.key(Pane::CommandLog.key());

        let right_column = if active_pane == Pane::CommandLog {
            rsx! {
                VStack {
                    width: Length::Flex(2),
                    gap: 0,
                    cmd_panel.clone(),
                }
            }
        } else {
            rsx! {
                VStack {
                    width: Length::Flex(2),
                    gap: 0,
                    focus_policy: FocusPolicy::Accordion(FocusAccordion {
                        focused_min: FOCUSED_PANEL_MIN_HEIGHT,
                        ..FocusAccordion::default()
                    }),
                    log_panel.clone(),
                    cmd_panel.clone(),
                }
            }
        };

        let main_content: Element = match bp {
            Breakpoint::Large => {
                rsx! {
                    HStack {
                        gap: 0,
                        VStack {
                            width: Length::Flex(1),
                            gap: 0,
                            focus_policy: FocusPolicy::Accordion(FocusAccordion {
                                focused_min: FOCUSED_PANEL_MIN_HEIGHT,
                                expanded_weight: 1,
                                ..FocusAccordion::default()
                            }),
                            status_panel.clone(),
                            files_panel.clone(),
                            branches_panel.clone(),
                            commits_panel.clone(),
                            stash_panel.clone(),
                        },
                        right_column.clone(),
                    }
                }
            }
            Breakpoint::Medium => {
                rsx! {
                    HStack {
                        gap: 0,
                        VStack {
                            width: Length::Flex(1),
                            gap: 0,
                            focus_policy: FocusPolicy::Accordion(FocusAccordion {
                                focused_min: FOCUSED_PANEL_MIN_HEIGHT,
                                expanded_weight: 1,
                                ..FocusAccordion::default()
                            }),
                            status_panel.clone(),
                            files_panel.clone(),
                            branches_panel.clone(),
                            commits_panel.clone(),
                            stash_panel.clone(),
                        },
                        right_column.clone(),
                    }
                }
            }
            Breakpoint::Small => {
                rsx! {
                    VStack {
                        gap: 0,
                        focus_policy: FocusPolicy::Accordion(FocusAccordion {
                            focused_min: FOCUSED_PANEL_MIN_HEIGHT,
                            ..FocusAccordion::default()
                        }),
                        status_panel.clone(),
                        files_panel.clone(),
                        branches_panel.clone(),
                        commits_panel.clone(),
                        stash_panel.clone(),
                    }
                }
            }
        };

        let hint = match active_pane {
            Pane::Status => {
                "Edit config file: e | Check for update: u | Switch to a recent repo: <enter> | Keybindings: ?"
            }
            Pane::Files => match ctx.state.files_tab {
                0 => {
                    "Stage: <space> | Commit: c | Edit: e | Stash: s | Discard: d | Reset: D | Keybindings: ?"
                }
                1 => "Checkout: <enter> | New: n | Delete: d | Keybindings: ?",
                _ => "Update: <enter> | Keybindings: ?",
            },
            Pane::Branches => match ctx.state.branches_tab {
                0 => {
                    "Checkout: <enter> | New branch: n | Delete: d | Rebase: r | Reset: g | Upstream: u | Keybindings: ?"
                }
                1 => "Fetch: f | Keybindings: ?",
                _ => "Checkout: <enter> | Keybindings: ?",
            },
            Pane::Commits => match ctx.state.commits_tab {
                0 => "Checkout: <enter> | Rebase: r | Reset: g | Fixup: f | Keybindings: ?",
                _ => "Checkout: <enter> | Keybindings: ?",
            },
            Pane::Stash => "Apply: <enter> | Drop: d | Pop: g | Keybindings: ?",
            Pane::Log => "Move: j/k | Keybindings: ?",
            Pane::CommandLog => "Move: j/k | Keybindings: ?",
        };

        let hint_bar = rsx! {
            HStack {
                height: Length::Px(HINT_BAR_HEIGHT),
                style: Style::new(),
                padding: (0, 1),
                Text {
                    content: hint,
                    overflow: Overflow::Ellipsis,
                    style: Style::new().fg(Color::Blue),
                },
                Spacer {},
                Text {
                    content: "Amazing tui-lipan!",
                    overflow: Overflow::Ellipsis,
                    style: Style::new().fg(Color::Yellow),
                },
            }
        };

        rsx! {
            VStack {
                gap: 0,
                VStack {
                    height: Length::Flex(1),
                    main_content,
                },
                hint_bar,
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl || key.mods.alt {
            return KeyUpdate::unhandled(Update::none());
        }

        let active_pane = Pane::ALL
            .iter()
            .copied()
            .find(|p| ctx.has_focus_within_key(p.key()))
            .unwrap_or(Pane::Files);

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char(c @ '0'..='6') => {
                let viewport = ctx.viewport();
                let is_small = viewport.w < 80;

                if let Some(pane) = Pane::from_number(c) {
                    if is_small && matches!(pane, Pane::Log | Pane::CommandLog) {
                        return KeyUpdate::unhandled(Update::none());
                    }
                    ctx.request_focus(pane.key());
                    KeyUpdate::handled(Update::full())
                } else {
                    KeyUpdate::unhandled(Update::none())
                }
            }
            KeyCode::Char('[') => match active_pane {
                Pane::Files => {
                    ctx.state.files_tab = ctx.state.files_tab.saturating_sub(1);
                    KeyUpdate::handled(Update::full())
                }
                Pane::Branches => {
                    ctx.state.branches_tab = ctx.state.branches_tab.saturating_sub(1);
                    KeyUpdate::handled(Update::full())
                }
                Pane::Commits => {
                    ctx.state.commits_tab = ctx.state.commits_tab.saturating_sub(1);
                    KeyUpdate::handled(Update::full())
                }
                _ => KeyUpdate::unhandled(Update::none()),
            },
            KeyCode::Char(']') => match active_pane {
                Pane::Files => {
                    ctx.state.files_tab = (ctx.state.files_tab + 1).min(2);
                    KeyUpdate::handled(Update::full())
                }
                Pane::Branches => {
                    ctx.state.branches_tab = (ctx.state.branches_tab + 1).min(2);
                    KeyUpdate::handled(Update::full())
                }
                Pane::Commits => {
                    ctx.state.commits_tab = (ctx.state.commits_tab + 1).min(1);
                    KeyUpdate::handled(Update::full())
                }
                _ => KeyUpdate::unhandled(Update::none()),
            },
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FilesSelected(ev) => {
                ctx.state.files_selected = ev.index;
                Update::full()
            }
            Msg::WorktreesSelected(ev) => {
                ctx.state.worktrees_selected = ev.index;
                Update::full()
            }
            Msg::SubmodulesSelected(ev) => {
                ctx.state.submodules_selected = ev.index;
                Update::full()
            }
            Msg::FilesTabChanged(ev) => {
                ctx.state.files_tab = ev.index;
                Update::full()
            }
            Msg::BranchesSelected(ev) => {
                ctx.state.branches_selected = ev.index;
                Update::full()
            }
            Msg::RemotesSelected(ev) => {
                ctx.state.remotes_selected = ev.index;
                Update::full()
            }
            Msg::TagsSelected(ev) => {
                ctx.state.tags_selected = ev.index;
                Update::full()
            }
            Msg::BranchesTabChanged(ev) => {
                ctx.state.branches_tab = ev.index;
                Update::full()
            }
            Msg::CommitsSelected(ev) => {
                ctx.state.commits_selected = ev.index;
                Update::full()
            }
            Msg::ReflogSelected(ev) => {
                ctx.state.reflog_selected = ev.index;
                Update::full()
            }
            Msg::CommitsTabChanged(ev) => {
                ctx.state.commits_tab = ev.index;
                Update::full()
            }
            Msg::StashSelected(ev) => {
                ctx.state.stash_selected = ev.index;
                Update::full()
            }
            Msg::CmdSelected(ev) => {
                ctx.state.cmd_selected = ev.index;
                Update::full()
            }
            Msg::LogScroll(ev) => {
                ctx.state.log_scroll_offset = ev.offset;
                Update::full()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new().mount(LazygitDemo).run()
}
