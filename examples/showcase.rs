use std::sync::Arc;

use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;

struct Showcase;

#[derive(Default)]
struct State {
    tab: usize,
    acc_open: [bool; 3],
    show_cmd: bool,
    show_fuzzy: bool,
    show_ctx: bool,
    ctx_anchor: Option<(u16, u16)>,
    nav_path: Vec<usize>,
    show_tooltip: bool,
    input_value: Arc<str>,
    input_cursor: usize,
    input_anchor: Option<usize>,
    last_input_edit: Option<TextEditEvent>,
    text_value: Arc<str>,
    text_cursor: usize,
    text_anchor: Option<usize>,
    last_text_edit: Option<TextEditEvent>,
    table_selected: usize,
    tree_selection: Option<String>,
    #[cfg(feature = "syntax-syntect")]
    highlight_on: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    SetTab(usize),
    ToggleAccordion(usize),
    ToggleCmd(bool),
    ToggleFuzzy(bool),
    ToggleCtx(bool),
    OpenCtx(u16, u16),
    ContextAction(usize),
    NavSelect(TreePath),
    BreadcrumbSelect(usize),
    InputChanged(InputEvent),
    InputEdited(TextEditEvent),
    TextAreaChanged(TextAreaEvent),
    TextAreaEdited(TextEditEvent),
    TableSelected(TableEvent),
    TreeSelected(TreeEvent),
    ShowToast,
    ShowDefaultToast,
    ToggleTooltip(bool),
    #[cfg(feature = "syntax-syntect")]
    ToggleHighlight(bool),
    NoOp,
}

impl Component for Showcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let base = State {
            input_value: Arc::from("Search term"),
            text_value: Arc::from("Line one\nLine two\nLine three"),
            ..State::default()
        };

        #[cfg(feature = "syntax-syntect")]
        {
            State {
                highlight_on: true,
                ..base
            }
        }

        #[cfg(not(feature = "syntax-syntect"))]
        {
            base
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SetTab(i) => {
                ctx.state.tab = i;
                Update::full()
            }
            Msg::ToggleAccordion(i) => {
                if i < 3 {
                    ctx.state.acc_open[i] = !ctx.state.acc_open[i];
                }
                Update::full()
            }
            Msg::ToggleCmd(show) => {
                ctx.state.show_cmd = show;
                Update::full()
            }
            Msg::ToggleFuzzy(show) => {
                ctx.state.show_fuzzy = show;
                Update::full()
            }
            Msg::ToggleCtx(show) => {
                ctx.state.show_ctx = show;
                if !show {
                    ctx.state.ctx_anchor = None;
                }
                Update::full()
            }
            Msg::OpenCtx(x, y) => {
                ctx.state.show_ctx = true;
                ctx.state.ctx_anchor = Some((x, y));
                Update::full()
            }
            Msg::ContextAction(index) => {
                ctx.state.show_ctx = false;
                ctx.state.ctx_anchor = None;
                let label = match index {
                    0 => "Cut",
                    1 => "Copy",
                    2 => "Paste",
                    3 => "Delete",
                    _ => "Action",
                };
                ctx.toast().push(Toast::new(format!("{} (demo)", label)));
                Update::full()
            }
            Msg::NavSelect(path) => {
                ctx.state.nav_path = path.segments().to_vec();
                Update::full()
            }
            Msg::BreadcrumbSelect(index) => {
                let take = index.min(ctx.state.nav_path.len());
                ctx.state.nav_path = ctx.state.nav_path.iter().cloned().take(take).collect();
                Update::full()
            }
            Msg::InputChanged(event) => {
                ctx.state.input_value = event.value;
                ctx.state.input_cursor = event.cursor;
                ctx.state.input_anchor = event.anchor;
                Update::full()
            }
            Msg::InputEdited(event) => {
                ctx.state.last_input_edit = Some(event);
                Update::full()
            }
            Msg::TextAreaChanged(event) => {
                ctx.state.text_value = event.value;
                ctx.state.text_cursor = event.cursor;
                ctx.state.text_anchor = event.anchor;
                Update::full()
            }
            Msg::TextAreaEdited(event) => {
                ctx.state.last_text_edit = Some(event);
                Update::full()
            }
            Msg::TableSelected(event) => {
                ctx.state.table_selected = event.index;
                Update::full()
            }
            Msg::TreeSelected(event) => {
                let path = event
                    .path
                    .segments()
                    .iter()
                    .map(|segment| segment.to_string())
                    .collect::<Vec<_>>()
                    .join(".");
                ctx.state.tree_selection = Some(path);
                Update::full()
            }
            Msg::ShowToast => {
                ctx.toast().push(
                    Toast::new("Action completed successfully!")
                        .title(Some("Notification"))
                        .title_alignment(Align::Center)
                        .frame_style(Style::new().bg(Color::Black))
                        .message_style(Style::new().fg(Color::White))
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoHeavy)
                                .style(Style::new().fg(Color::LightBlue)),
                        )
                        .decoration(
                            EdgeDecoration::new(Edge::Right)
                                .glyph(DecorationGlyph::AutoHeavy)
                                .style(Style::new().fg(Color::LightBlue)),
                        )
                        .border(false)
                        .padding((1, 2))
                        .max_width(Length::Px(56)),
                );
                Update::full()
            }
            Msg::ShowDefaultToast => {
                ctx.toast()
                    .push(Toast::new("Default toast using the standard chrome.").copyable(true));
                Update::full()
            }
            Msg::ToggleTooltip(show) => {
                ctx.state.show_tooltip = show;
                Update::full()
            }
            #[cfg(feature = "syntax-syntect")]
            Msg::ToggleHighlight(show) => {
                ctx.state.highlight_on = show;
                Update::full()
            }
            Msg::NoOp => Update::none(),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let content = rsx! {
            VStack {
                gap: 1,
                padding: 1,
                Tabs {
                    border: true,
                    active: ctx.state.tab,
                    on_change: ctx.link().callback(|e: TabsEvent| Msg::SetTab(e.index)),
                    tab: "General",
                    tab: "Navigation",
                    tab: "Data & Text",
                    tab: "Overlays",
                    tab: "Code",
                    height: Length::Px(3),
                },
                Frame::new()
                    .border(false)
                    .height(Length::Flex(1))
                    .child(
                        match ctx.state.tab {
                            0 => self.view_general(ctx),
                            1 => self.view_navigation(ctx),
                            2 => self.view_data_text(ctx),
                            3 => self.view_overlays(ctx),
                            4 => self.view_code(ctx),
                            _ => Text::new("Unknown Tab").into(),
                        },
                    ),
                {
                    StatusBar::new()
                        .left(Text::new("Showcase Mode").style(Style::new().bold()))
                        .center(Text::new(format!("Tab: {}", ctx.state.tab)))
                        .right(Badge::new("v0.1").style(Style::new().bg(Color::Red)))
                        .style(Style::new().bg(Color::DarkGray).fg(Color::White))
                        .height(Length::Px(1))
                },
            }
        };

        let shell = rsx! {
            ZStack {
                content,
                if ctx.state.show_cmd {
                    {
                        Modal::new()
                            .title("Quick Actions")
                            .child(
                                SearchPalette::<Arc<str>>::new()
                                    .items(
                                        vec![
                                            SearchItem::new("Reload", Arc::from("Reload")).description("Reload config"),
                                            SearchItem::new("Exit", Arc::from("Exit")).description("Quit application"),
                                        ],
                                    ),
                            )
                            .backdrop_style(Style::new().dim_by(0.5))
                            .padding(0)
                            .on_close(ctx.link().callback(|_| Msg::ToggleCmd(false)))
                            .key("search-palette-cmd")
                    },
                },
                if ctx.state.show_fuzzy {
                    {
                        Modal::new()
                            .title("Search Files")
                            .child(
                                SearchPalette::<Arc<str>>::new()
                                    .items(
                                        vec![
                                            SearchItem::new("src/main.rs", Arc::from("src/main.rs")),
                                            SearchItem::new("src/lib.rs", Arc::from("src/lib.rs")),
                                            SearchItem::new("examples/showcase.rs", Arc::from("examples/showcase.rs")),
                                        ],
                                    ),
                            )
                            .padding(0)
                            .on_close(ctx.link().callback(|_| Msg::ToggleFuzzy(false)))
                            .key("search-palette-fuzzy")
                    },
                },
            }
        };

        ThemeProvider::new(Theme {
            primary: Style::new().fg(Color::LightBlue),
            ..Theme::default()
        })
        .child(shell)
        .into()
    }
}

impl Showcase {
    fn view_general(&self, ctx: &Context<Self>) -> Element {
        let show_tooltip = ctx.state.show_tooltip;
        rsx! {
            HStack {
                gap: 2,
                VStack {
                    width: Length::Flex(1),
                    gap: 1,
                    Frame {
                        title: "Accordion",
                        border: true,
                        {
                            Accordion::new()
                                .exclusive(false)
                                .on_toggle(ctx.link().callback(Msg::ToggleAccordion))
                                .item(
                                    AccordionItem::new(
                                            "Section A",
                                            Text::new("This is the content of section A."),
                                        )
                                        .expanded(ctx.state.acc_open[0]),
                                )
                                .item(
                                    AccordionItem::new("Section B", Button::filled("A Button Inside"))
                                        .expanded(ctx.state.acc_open[1]),
                                )
                        },
                    },
                    Frame {
                        title: "Sparkline",
                        border: true,
                        VStack {
                            gap: 1,
                            Text::new("Server Load (bars):"),
                            {
                                Sparkline::new(vec![10, 40, 20, 50, 80, 30, 60, 90, 45, 10])
                                    .max(100)
                                    .bars_preset(SparklineBarsPreset::Blocks)
                            },
                            Text::new("Latency Trend (line):"),
                            {
                                Sparkline::new(vec![35, 30, 32, 29, 27, 31, 26, 24, 28, 25, 22, 23])
                                    .line()
                                    .max_points(10)
                                    .rising_style(Style::new().fg(Color::Green))
                                    .falling_style(Style::new().fg(Color::Red))
                                    .turn_style(Style::new().fg(Color::Yellow))
                            },
                        },
                    },
                },
                VStack {
                    width: Length::Flex(1),
                    gap: 1,
                    Frame {
                        title: "Badges",
                        border: true,
                        HStack {
                            gap: 2,
                            {
                                Badge::new("99+")
                                    .position(BadgePosition::TopStart)
                                    .style(Style::new().bg(Color::Red).fg(Color::Black).bold())
                                    .child(Button::outlined("Notifications"))
                            },
                            {
                                Badge::new("New")
                                    .position(BadgePosition::TopStart)
                                    .style(Style::new().bg(Color::Blue).fg(Color::Black).bold())
                                    .child(Frame::new().border(true).child(Text::new("Feature Box")))
                            },
                        },
                    },
                    Frame {
                        title: "Tooltip",
                        border: true,
                        Center {
                            {
                                Tooltip::new("This is a helpful tip!")
                                    .open(show_tooltip)
                                    .placement(PopoverPlacement::RightCenter)
                                    .child(
                                        Button::filled("Hover Me (Click to Toggle)")
                                            .on_click(
                                                ctx.link().callback(move |_| Msg::ToggleTooltip(!show_tooltip)),
                                            ),
                                    )
                            },
                        },
                    },
                },
            }
        }
    }

    fn view_navigation(&self, ctx: &Context<Self>) -> Element {
        let nav_root = nav_tree_data();
        let breadcrumb_segments = nav_breadcrumb_segments(&nav_root, &ctx.state.nav_path);
        let breadcrumb_active = breadcrumb_segments.len().saturating_sub(1);
        let breadcrumb = Breadcrumb::new()
            .segments(breadcrumb_segments)
            .active(Some(breadcrumb_active))
            .on_select(ctx.link().callback(Msg::BreadcrumbSelect))
            .active_style(Style::new().bold())
            .inactive_style(Style::new().fg(Color::DarkGray))
            .hover_style(Style::new().fg(Color::LightBlue).underline())
            .separator_style(Style::new().fg(Color::DarkGray));
        let tree = Tree::new(nav_tree_widget(&nav_root))
            .on_select(ctx.link().callback(|ev: TreeEvent| Msg::NavSelect(ev.path)))
            .style(Style::new().fg(Color::indexed(25)))
            .selection_style(Style::new().reverse())
            .item_hover_style(Style::new().fg(Color::LightBlue))
            .scrollbar(true)
            .height(Length::Flex(1));
        rsx! {
            HStack {
                gap: 2,
                align: Align::Stretch,
                Frame {
                    title: "Project Navigator",
                    border: true,
                    padding: 1,
                    width: Length::Flex(2),
                    height: Length::Flex(1),
                    VStack {
                        gap: 1,
                        height: Length::Flex(1),
                        breadcrumb,
                        Divider::horizontal(),
                        tree,
                    },
                },
                Frame {
                    title: "Quick Actions",
                    border: true,
                    padding: 1,
                    width: Length::Flex(1),
                    height: Length::Flex(1),
                    VStack {
                        gap: 1,
                        align: Align::Stretch,
                        justify: Justify::Center,
                        height: Length::Flex(1),
                        Button {
                            label: "Open Command Palette",
                            full_width: true,
                            on_click: ctx.link().callback(|_| Msg::ToggleCmd(true)),
                        },
                        Button {
                            label: "Open Fuzzy Finder",
                            full_width: true,
                            on_click: ctx.link().callback(|_| Msg::ToggleFuzzy(true)),
                        },
                    },
                },
            }
        }
    }

    fn view_overlays(&self, ctx: &Context<Self>) -> Element {
        let text_view = TextArea::new(
            "Select some text and right-click to open the context menu.\n\nThe actions here are demo-only, but the selection is real.",
        )
        .read_only(true)
        .wrap(true)
        .border(false)
        .padding(1)
        .scrollbar(false)
        .height(Length::Px(6))
        .on_click(ctx.link().callback(|ev: MouseEvent| {
            if matches!(ev.kind, MouseKind::Down(MouseButton::Right)) {
                let anchor_x = ev.x.saturating_sub(1);
                let anchor_y = ev.y.saturating_sub(1);
                Msg::OpenCtx(anchor_x, anchor_y)
            } else {
                Msg::NoOp
            }
        }));

        let context_menu = ContextMenu::new(
            Frame::new()
                .title("Text View")
                .border(true)
                .child(text_view),
        )
        .open(ctx.state.show_ctx)
        .anchor(ctx.state.ctx_anchor)
        .offset((0, 1))
        .on_close(ctx.link().callback(|_| Msg::ToggleCtx(false)))
        .on_select(ctx.link().callback(Msg::ContextAction))
        .selection_style(Style::new().reverse())
        .selection_symbol(None::<&str>)
        .items(vec![
            ListItem::new("Cut"),
            ListItem::new("Copy"),
            ListItem::new("Paste"),
            ListItem::new("Delete").style(Style::new().fg(Color::Red)),
        ])
        .width(Length::Auto);

        rsx! {
            VStack {
                gap: 2,
                Button {
                    label: "Show Toast Notification",
                    on_click: ctx.link().callback(|_| Msg::ShowToast),
                },
                Button {
                    label: "Show Default Toast",
                    on_click: ctx.link().callback(|_| Msg::ShowDefaultToast),
                },
                Divider::horizontal(),
                context_menu,
            }
        }
    }

    fn view_data_text(&self, ctx: &Context<Self>) -> Element {
        let table = Table::new()
            .header(TableRow::new(["ID", "Details", "Status"]))
            .rows([
                TableRow::new(["01", "Single line", "Ok"]),
                TableRow::new(["02", "Line one\nLine two", "Auto height"]).auto_height(),
                TableRow::new(["03", "Line one\nLine two\nLine three", "Auto height"])
                    .auto_height(),
            ])
            .widths([
                ColumnWidth::Fixed(4),
                ColumnWidth::Fill(2),
                ColumnWidth::Min(10),
            ])
            .selected(ctx.state.table_selected)
            .selection_symbol(Some(">"))
            .scrollbar(true)
            .border(true)
            .on_select(ctx.link().callback(Msg::TableSelected));

        let root = TreeNode::new("workspace")
            .expanded(true)
            .child(
                TreeNode::new("src")
                    .expanded(true)
                    .child(TreeNode::new("app.rs"))
                    .child(TreeNode::new("widgets"))
                    .child(TreeNode::new("style")),
            )
            .child(
                TreeNode::new("examples")
                    .expanded(true)
                    .child(TreeNode::new("showcase.rs")),
            )
            .child(TreeNode::new("docs"));

        let tree_label = ctx.state.tree_selection.as_deref().unwrap_or("(none)");
        let input_edit = ctx
            .state
            .last_input_edit
            .as_ref()
            .map(format_edit)
            .unwrap_or_else(|| "(none)".to_string());
        let text_edit = ctx
            .state
            .last_text_edit
            .as_ref()
            .map(format_edit)
            .unwrap_or_else(|| "(none)".to_string());

        rsx! {
            VStack {
                gap: 2,
                HStack {
                    gap: 2,
                    Frame {
                        title: "Table (auto height)",
                        border: true,
                        padding: 1,
                        width: Length::Flex(1),
                        height: Length::Flex(1),
                        table,
                    },
                    Frame {
                        title: "Tree (custom icons)",
                        border: true,
                        padding: 1,
                        width: Length::Flex(1),
                        height: Length::Flex(1),
                        VStack {
                            gap: 1,
                            {
                                Tree::new(root)
                                    .show_icons(true)
                                    .expanded_icon("▼")
                                    .collapsed_icon("▶")
                                    .scrollbar(true)
                                    .show_scroll_indicators(true)
                                    .focus_policy(FocusAccordion::default())
                                    .on_select(ctx.link().callback(Msg::TreeSelected))
                            },
                            Text::new(format!("Selected path: {}", tree_label)).style(Style::new().dim()),
                        },
                    },
                },
                HStack {
                    gap: 2,
                    Frame {
                        title: "Input (anchor + on_edit)",
                        border: true,
                        padding: 1,
                        width: Length::Flex(1),
                        VStack {
                            gap: 1,
                            {
                                Input::new(ctx.state.input_value.clone())
                                    .cursor(ctx.state.input_cursor)
                                    .anchor(ctx.state.input_anchor)
                                    .placeholder("Type to edit")
                                    .on_change(ctx.link().callback(Msg::InputChanged))
                                    .on_edit(ctx.link().callback(Msg::InputEdited))
                            },
                            Text::new(
                                    format!(
                                        "on_change cursor={} anchor={:?}", ctx.state.input_cursor, ctx.state
                                        .input_anchor
                                    ),
                                )
                                .style(Style::new().dim()),
                            Text::new(format!("on_edit: {}", input_edit)).style(Style::new().dim()),
                        },
                    },
                    Frame {
                        title: "TextArea (anchor + on_edit)",
                        border: true,
                        padding: 1,
                        width: Length::Flex(1),
                        height: Length::Flex(1),
                        VStack {
                            gap: 1,
                            {
                                TextArea::new(ctx.state.text_value.clone())
                                    .cursor(ctx.state.text_cursor)
                                    .anchor(ctx.state.text_anchor)
                                    .line_numbers(true)
                                    .on_change(ctx.link().callback(Msg::TextAreaChanged))
                                    .on_edit(ctx.link().callback(Msg::TextAreaEdited))
                            },
                            Text::new(
                                    format!(
                                        "on_change cursor={} anchor={:?}", ctx.state.text_cursor, ctx.state
                                        .text_anchor
                                    ),
                                )
                                .style(Style::new().dim()),
                            Text::new(format!("on_edit: {}", text_edit)).style(Style::new().dim()),
                        },
                    },
                },
            }
        }
    }

    fn view_code(&self, ctx: &Context<Self>) -> Element {
        #[cfg(not(feature = "syntax-syntect"))]
        let _ = ctx;

        #[cfg(feature = "syntax-syntect")]
        let show_highlight = ctx.state.highlight_on;

        #[cfg(feature = "syntax-syntect")]
        let mut code_area = TextArea::new(
            r#"fn main() {
    println!("Hello, tui-lipan!");

    // This is a code block widget
    let x = 42;
    match x {
        42 => println!("Answer!"),
        _ => println!("Unknown"),
    }
}"#,
        )
        .line_numbers(true)
        .read_only(true)
        .border(false)
        .padding(0);

        #[cfg(not(feature = "syntax-syntect"))]
        let code_area = TextArea::new(
            r#"fn main() {
    println!("Hello, tui-lipan!");

    // This is a code block widget
    let x = 42;
    match x {
        42 => println!("Answer!"),
        _ => println!("Unknown"),
    }
}"#,
        )
        .line_numbers(true)
        .read_only(true)
        .border(false)
        .padding(0);

        #[cfg(feature = "syntax-syntect")]
        if show_highlight {
            code_area = code_area.with_syntax("rust", "Monokai Extended");
        }

        let mut stack = VStack::new().height(Length::Auto).gap(1);

        #[cfg(feature = "syntax-syntect")]
        {
            let label = if show_highlight {
                "Highlight: On "
            } else {
                "Highlight: Off"
            };
            stack = stack.child(
                HStack::new()
                    .gap(1)
                    .child(
                        Button::outlined(label).on_click(
                            ctx.link()
                                .callback(move |_| Msg::ToggleHighlight(!show_highlight)),
                        ),
                    )
                    .child(Text::new("Toggle syntax highlighting")),
            );
        }

        #[cfg(not(feature = "syntax-syntect"))]
        {
            stack = stack.child(
                Text::new("Enable `syntax-syntect` to see highlighting.").style(Style::new().dim()),
            );
        }

        stack
            .child(Frame::new().title("Example Code").child(code_area))
            .into()
    }
}

#[derive(Clone)]
struct NavNode {
    label: &'static str,
    expanded: bool,
    children: Vec<NavNode>,
}

impl NavNode {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            expanded: false,
            children: Vec::new(),
        }
    }

    fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    fn child(mut self, child: NavNode) -> Self {
        self.children.push(child);
        self
    }
}

fn nav_tree_data() -> NavNode {
    NavNode::new("Workspace")
        .expanded(true)
        .child(
            NavNode::new("src")
                .expanded(true)
                .child(
                    NavNode::new("app")
                        .expanded(true)
                        .child(NavNode::new("runner.rs"))
                        .child(
                            NavNode::new("input")
                                .expanded(true)
                                .child(NavNode::new("keyboard.rs"))
                                .child(NavNode::new("mouse.rs"))
                                .child(NavNode::new("convert.rs")),
                        ),
                )
                .child(
                    NavNode::new("widgets")
                        .expanded(true)
                        .child(NavNode::new("button.rs"))
                        .child(NavNode::new("context_menu.rs"))
                        .child(NavNode::new("tree.rs"))
                        .child(NavNode::new("tooltip.rs"))
                        .child(
                            NavNode::new("popover")
                                .expanded(true)
                                .child(NavNode::new("mod.rs"))
                                .child(NavNode::new("layout.rs")),
                        ),
                )
                .child(
                    NavNode::new("layout")
                        .expanded(true)
                        .child(NavNode::new("hash.rs"))
                        .child(NavNode::new("measure.rs"))
                        .child(
                            NavNode::new("reconcile")
                                .expanded(true)
                                .child(NavNode::new("element.rs"))
                                .child(NavNode::new("overlay.rs")),
                        ),
                )
                .child(NavNode::new("lib.rs")),
        )
        .child(
            NavNode::new("examples")
                .expanded(true)
                .child(NavNode::new("showcase.rs"))
                .child(NavNode::new("forms.rs"))
                .child(NavNode::new("todo.rs"))
                .child(NavNode::new("lazygit.rs"))
                .child(NavNode::new("todo_ui.rs"))
                .child(NavNode::new("gradient_widgets.rs")),
        )
        .child(
            NavNode::new("docs")
                .expanded(true)
                .child(NavNode::new("ROADMAP.md"))
                .child(NavNode::new("AGENTS.md"))
                .child(NavNode::new("THEMES.md")),
        )
        .child(
            NavNode::new(".github")
                .expanded(true)
                .child(
                    NavNode::new("workflows")
                        .expanded(true)
                        .child(NavNode::new("ci.yml"))
                        .child(NavNode::new("release.yml")),
                )
                .child(NavNode::new("ISSUE_TEMPLATE.md")),
        )
        .child(NavNode::new("Cargo.toml"))
        .child(NavNode::new("README.md"))
        .child(NavNode::new("LICENSE"))
        .child(NavNode::new("tui-lipan-macro"))
        .child(NavNode::new("target"))
        .child(NavNode::new(".gitignore"))
}

fn format_edit(edit: &TextEditEvent) -> String {
    let deleted = short_text(edit.deleted.as_ref());
    let inserted = short_text(edit.inserted.as_ref());
    format!(
        "kind={:?} start={} deleted=\"{}\" inserted=\"{}\"",
        edit.kind, edit.start, deleted, inserted
    )
}

fn short_text(text: &str) -> String {
    let limit = 12;
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push_str("...");
    }
    out
}

fn nav_tree_widget(node: &NavNode) -> TreeNode {
    let mut tree = TreeNode::new(node.label).expanded(node.expanded);
    for child in &node.children {
        tree = tree.child(nav_tree_widget(child));
    }
    tree
}

fn nav_breadcrumb_segments(root: &NavNode, path: &[usize]) -> Vec<&'static str> {
    let mut segments = vec![root.label];
    let mut cursor = root;
    for &idx in path {
        let Some(next) = cursor.children.get(idx) else {
            break;
        };
        segments.push(next.label);
        cursor = next;
    }
    segments
}

fn main() -> Result<()> {
    App::new().title("tui-lipan Showcase").mount(Showcase).run()
}
