use std::sync::Arc;

use tui_lipan::prelude::*;

struct TabsHubDemo;

#[derive(Clone)]
struct TabItem {
    title: Arc<str>,
    path: Arc<str>,
    closeable: bool,
    badge: Option<Span>,
}

struct State {
    section_tab: usize,
    classic_tab: usize,
    minimal_tab: usize,
    custom_tab: usize,
    bordered_tabs: Vec<TabItem>,
    bordered_active: usize,
    frame_tabs: Vec<TabItem>,
    frame_active: usize,
    next_tab_id: usize,
    status: Arc<str>,
}

impl Default for State {
    fn default() -> Self {
        let base = vec![
            TabItem {
                title: Arc::from("main.rs"),
                path: Arc::from("src/main.rs"),
                closeable: true,
                badge: Some(Span::new("M").fg(Color::LightYellow).bold()),
            },
            TabItem {
                title: Arc::from("lib.rs"),
                path: Arc::from("src/lib.rs"),
                closeable: true,
                badge: Some(Span::new("A").fg(Color::LightGreen).bold()),
            },
            TabItem {
                title: Arc::from("theme-provider.rs"),
                path: Arc::from("src/widgets/theme_provider.rs"),
                closeable: true,
                badge: Some(Span::new("R").fg(Color::LightBlue).bold()),
            },
            TabItem {
                title: Arc::from("README.md"),
                path: Arc::from("README.md"),
                closeable: false,
                badge: Some(Span::new("?").fg(Color::indexed(208)).bold()),
            },
            TabItem {
                title: Arc::from("Cargo.toml"),
                path: Arc::from("Cargo.toml"),
                closeable: true,
                badge: None,
            },
            TabItem {
                title: Arc::from("workspace.lock"),
                path: Arc::from("Cargo.lock"),
                closeable: true,
                badge: Some(Span::new("M").fg(Color::LightYellow).bold()),
            },
            TabItem {
                title: Arc::from("terminal_filetree_devtools.rs"),
                path: Arc::from("examples/terminal_filetree_devtools.rs"),
                closeable: true,
                badge: Some(Span::new("D").fg(Color::LightRed).bold()),
            },
            TabItem {
                title: Arc::from("opencode_home.rs"),
                path: Arc::from("examples/opencode_home.rs"),
                closeable: false,
                badge: None,
            },
        ];

        Self {
            section_tab: 0,
            classic_tab: 0,
            minimal_tab: 0,
            custom_tab: 0,
            bordered_tabs: base.clone(),
            bordered_active: 0,
            frame_tabs: base,
            frame_active: 1,
            next_tab_id: 1,
            status: Arc::from(
                "Drag to reorder/transfer, wheel to scroll tabs, click x to close, + or n to add tabs (badges show status).",
            ),
        }
    }
}

#[derive(Clone, Debug)]
#[allow(clippy::enum_variant_names)]
enum Msg {
    SectionChanged(TabsEvent),
    ClassicTabChanged(TabsEvent),
    MinimalTabChanged(TabsEvent),
    CustomTabChanged(TabsEvent),
    BorderedChange(TabsEvent),
    BorderedClose(DraggableTabCloseEvent),
    BorderedReorder(DraggableTabReorderEvent),
    FrameChange(TabsEvent),
    FrameClose(DraggableTabCloseEvent),
    FrameReorder(DraggableTabReorderEvent),
    Transfer(DraggableTabTransferEvent),
    AddTab,
}

impl TabsHubDemo {
    fn view_tab_variants(&self, ctx: &Context<Self>) -> Element {
        let tabs = vec!["Files", "Search", "Git"];

        let classic = VStack::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .tab_titles(tabs.clone())
            .active_tab(ctx.state.classic_tab)
            .tab_variant(TabVariant::Classic)
            .active_tab_style(Style::new().fg(Color::LightBlue).bold())
            .inactive_tab_style(Style::new().fg(Color::DarkGray))
            .on_tab_change(ctx.link().callback(Msg::ClassicTabChanged))
            .child(Text::new("TabVariant::Classic").style(Style::new().bold()))
            .child(Text::new("[ Active ]|Inactive|..."))
            .child(Spacer::new())
            .child(
                Text::new(format!("Selected: {}", tabs[ctx.state.classic_tab]))
                    .style(Style::new().fg(Color::LightBlue)),
            );

        let minimal = VStack::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .tab_titles(tabs.clone())
            .active_tab(ctx.state.minimal_tab)
            .tab_variant(TabVariant::Minimal)
            .active_tab_style(Style::new().fg(Color::LightGreen).bold())
            .inactive_tab_style(Style::new().fg(Color::DarkGray))
            .on_tab_change(ctx.link().callback(Msg::MinimalTabChanged))
            .child(Text::new("TabVariant::Minimal").style(Style::new().bold()))
            .child(Text::new("Active - Inactive - ..."))
            .child(Spacer::new())
            .child(
                Text::new(format!("Selected: {}", tabs[ctx.state.minimal_tab]))
                    .style(Style::new().fg(Color::LightGreen)),
            );

        let custom = VStack::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .tab_titles(tabs.clone())
            .active_tab(ctx.state.custom_tab)
            .tab_variant(TabVariant::Custom {
                active_brackets: ('<', '>'),
                separator: " | ",
            })
            .active_tab_style(Style::new().fg(Color::LightMagenta).bold())
            .inactive_tab_style(Style::new().fg(Color::DarkGray))
            .on_tab_change(ctx.link().callback(Msg::CustomTabChanged))
            .child(Text::new("TabVariant::Custom").style(Style::new().bold()))
            .child(Text::new("<Active> | Inactive | ..."))
            .child(Spacer::new())
            .child(
                Text::new(format!("Selected: {}", tabs[ctx.state.custom_tab]))
                    .style(Style::new().fg(Color::LightMagenta)),
            );

        VStack::new()
            .gap(1)
            .child(
                Text::new("TabVariant Styles Demo - Click tabs or press 1/2/3, q to quit")
                    .style(Style::new().fg(Color::DarkGray)),
            )
            .child(
                HStack::new()
                    .gap(1)
                    .child(classic)
                    .child(minimal)
                    .child(custom),
            )
            .into()
    }

    fn view_draggable_tab_bar(&self, ctx: &Context<Self>) -> Element {
        let bordered = DraggableTabBar::new()
            .tabs(ctx.state.bordered_tabs.iter().map(to_widget_tab))
            .tab(new_tab_action())
            .active(ctx.state.bordered_active)
            .bar_id("bordered")
            .drag_group("editor")
            .variant(DraggableTabBarVariant::Bordered)
            .border(true)
            .border_style(BorderStyle::Rounded)
            .tab_hover_style(Style::new().bg(Color::indexed(238)))
            .active_style(Style::new().bg(Color::indexed(24)).fg(Color::White).bold())
            .close_style(Style::new().fg(Color::indexed(246)))
            .close_hover_style(Style::new().fg(Color::LightRed).bold())
            .tab_max_width(Some(14))
            .scroll_wheel(true)
            .show_overflow_controls(true)
            .overflow_style(Style::new().fg(Color::indexed(244)).dim())
            .overflow_hover_style(Style::new().fg(Color::White).bold())
            .show_file_icons(true)
            .file_icon_style(FileIconStyle::NerdFontColored)
            .file_icon_override("md", "󰍔", Some(Color::indexed(222)))
            .reorder_mode(DragReorderMode::Live)
            .on_change(ctx.link().callback(Msg::BorderedChange))
            .on_action(ctx.link().callback(|_| Msg::AddTab))
            .on_close(ctx.link().callback(Msg::BorderedClose))
            .on_transfer(ctx.link().callback(Msg::Transfer))
            .on_reorder(ctx.link().callback(Msg::BorderedReorder));

        let frame_line = DraggableTabBar::new()
            .tabs(ctx.state.frame_tabs.iter().map(to_widget_tab))
            .tab(new_tab_action())
            .active(ctx.state.frame_active)
            .bar_id("frame")
            .drag_group("editor")
            .variant(DraggableTabBarVariant::FrameLine)
            .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
            .tab_hover_style(Style::new().bg(Color::indexed(238)))
            .active_style(Style::new().bg(Color::indexed(25)).fg(Color::White).bold())
            .accent_symbol('▏')
            .accent_style(Style::new().fg(Color::indexed(240)))
            .active_accent_style(Style::new().fg(Color::LightCyan).bold())
            .close_style(Style::new().fg(Color::indexed(246)))
            .close_hover_style(Style::new().fg(Color::LightRed).bold())
            .close_on_hover_only(true)
            .tab_max_width(Some(16))
            .scroll_wheel(true)
            .show_overflow_controls(true)
            .overflow_style(Style::new().fg(Color::indexed(245)).dim())
            .overflow_hover_style(Style::new().fg(Color::indexed(230)).bold())
            .show_file_icons(true)
            .file_icon_style(FileIconStyle::NerdFontColored)
            .file_icon_override("lock", "󰌾", Some(Color::indexed(250)))
            .reorder_mode(DragReorderMode::Live)
            .on_change(ctx.link().callback(Msg::FrameChange))
            .on_action(ctx.link().callback(|_| Msg::AddTab))
            .on_close(ctx.link().callback(Msg::FrameClose))
            .on_transfer(ctx.link().callback(Msg::Transfer))
            .on_reorder(ctx.link().callback(Msg::FrameReorder));

        VStack::new()
            .gap(1)
            .child(
                Text::new("DraggableTabBar Demo")
                    .style(Style::new().bold().fg(Color::Rgb(109, 198, 255))),
            )
            .child(Text::new(
                "Drag tabs to reorder/transfer, click + to create, wheel-scroll horizontally, arrows show hidden counts.",
            ))
            .child(
                Frame::new()
                    .title("Bordered Variant")
                    .border(true)
                    .padding(1)
                    .child(bordered),
            )
            .child(
                Frame::new()
                    .title("FrameLine Variant")
                    .border(true)
                    .padding(1)
                    .child(frame_line),
            )
            .child(
                StatusBar::new()
                    .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
                    .left(Text::new(ctx.state.status.clone()))
                    .right(Text::new("+: add tab | n: add tab | q: quit")),
            )
            .into()
    }
}

impl Component for TabsHubDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SectionChanged(event) => {
                ctx.state.section_tab = event.index.min(1);
                Update::full()
            }
            Msg::ClassicTabChanged(ev) => {
                ctx.state.classic_tab = ev.index;
                Update::full()
            }
            Msg::MinimalTabChanged(ev) => {
                ctx.state.minimal_tab = ev.index;
                Update::full()
            }
            Msg::CustomTabChanged(ev) => {
                ctx.state.custom_tab = ev.index;
                Update::full()
            }
            Msg::BorderedChange(event) => {
                ctx.state.bordered_active = event.index;
                ctx.state.status = Arc::from(format!("Bordered active: {}", event.index));
                Update::full()
            }
            Msg::BorderedClose(event) => {
                close_tab(
                    &mut ctx.state.bordered_tabs,
                    &mut ctx.state.bordered_active,
                    event.index,
                );
                ctx.state.status = Arc::from(format!("Bordered close: {}", event.index));
                Update::full()
            }
            Msg::BorderedReorder(event) => {
                if apply_reorder(
                    &mut ctx.state.bordered_tabs,
                    &mut ctx.state.bordered_active,
                    event.from,
                    event.to,
                ) {
                    ctx.state.status =
                        Arc::from(format!("Bordered reorder: {} -> {}", event.from, event.to));
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::FrameChange(event) => {
                ctx.state.frame_active = event.index;
                ctx.state.status = Arc::from(format!("FrameLine active: {}", event.index));
                Update::full()
            }
            Msg::FrameClose(event) => {
                close_tab(
                    &mut ctx.state.frame_tabs,
                    &mut ctx.state.frame_active,
                    event.index,
                );
                ctx.state.status = Arc::from(format!("FrameLine close: {}", event.index));
                Update::full()
            }
            Msg::FrameReorder(event) => {
                if apply_reorder(
                    &mut ctx.state.frame_tabs,
                    &mut ctx.state.frame_active,
                    event.from,
                    event.to,
                ) {
                    ctx.state.status =
                        Arc::from(format!("FrameLine reorder: {} -> {}", event.from, event.to));
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::Transfer(event) => {
                let changed = apply_transfer(
                    &mut ctx.state.bordered_tabs,
                    &mut ctx.state.bordered_active,
                    &mut ctx.state.frame_tabs,
                    &mut ctx.state.frame_active,
                    &event,
                );
                if changed {
                    ctx.state.status = Arc::from(format!(
                        "Transfer {}:{} -> {}:{}",
                        event.from_bar, event.from, event.to_bar, event.to
                    ));
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::AddTab => {
                let id = ctx.state.next_tab_id;
                let name = Arc::from(format!("scratch-{}.rs", id));
                ctx.state.next_tab_id = ctx.state.next_tab_id.saturating_add(1);
                let item = TabItem {
                    title: name,
                    path: Arc::from(format!("src/scratch-{}.rs", id)),
                    closeable: true,
                    badge: None,
                };
                ctx.state.bordered_tabs.push(item.clone());
                ctx.state.frame_tabs.push(item);
                ctx.state.status = Arc::from("Added tab to both variants".to_string());
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl || key.mods.alt {
            return KeyUpdate::unhandled(Update::none());
        }

        match key.code {
            KeyCode::Char('1') if ctx.state.section_tab == 0 => {
                ctx.state.classic_tab = 0;
                ctx.state.minimal_tab = 0;
                ctx.state.custom_tab = 0;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') if ctx.state.section_tab == 0 => {
                ctx.state.classic_tab = 1;
                ctx.state.minimal_tab = 1;
                ctx.state.custom_tab = 1;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('3') if ctx.state.section_tab == 0 => {
                ctx.state.classic_tab = 2;
                ctx.state.minimal_tab = 2;
                ctx.state.custom_tab = 2;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('n') if ctx.state.section_tab == 1 => {
                ctx.link().send(Msg::AddTab);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let section_tabs = Tabs::new()
            .tab("Tab Variants")
            .tab("Draggable Tab Bar")
            .active(ctx.state.section_tab.min(1))
            .on_change(ctx.link().callback(Msg::SectionChanged));

        let section = if ctx.state.section_tab == 0 {
            self.view_tab_variants(ctx)
        } else {
            self.view_draggable_tab_bar(ctx)
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Text::new("Tabs Hub - Switch sections with tabs (q to quit)")
                    .style(Style::new().fg(Color::DarkGray)),
            )
            .child(section_tabs)
            .child(section)
            .into()
    }
}

fn to_widget_tab(item: &TabItem) -> DraggableTab {
    let mut tab = DraggableTab::new(item.title.clone())
        .path(item.path.clone())
        .closeable(item.closeable);
    if let Some(badge) = &item.badge {
        tab = tab.right_badge(badge.clone());
    }
    tab
}

fn new_tab_action() -> DraggableTab {
    DraggableTab::action("+").style(Style::new().fg(Color::LightGreen).bold())
}

fn close_tab(tabs: &mut Vec<TabItem>, active: &mut usize, index: usize) {
    if index >= tabs.len() || !tabs[index].closeable {
        return;
    }
    tabs.remove(index);
    if tabs.is_empty() {
        *active = 0;
    } else if *active > index {
        *active = active.saturating_sub(1);
    } else if *active >= tabs.len() {
        *active = tabs.len().saturating_sub(1);
    }
}

fn apply_reorder(tabs: &mut Vec<TabItem>, active: &mut usize, from: usize, to: usize) -> bool {
    if from >= tabs.len() || to >= tabs.len() || from == to {
        return false;
    }

    let moved = tabs.remove(from);
    tabs.insert(to, moved);
    *active = remap_index(*active, from, to);
    true
}

fn remap_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        return to;
    }
    if from < to {
        if index > from && index <= to {
            return index.saturating_sub(1);
        }
    } else if index >= to && index < from {
        return index.saturating_add(1);
    }
    index
}

fn apply_transfer(
    bordered_tabs: &mut Vec<TabItem>,
    bordered_active: &mut usize,
    frame_tabs: &mut Vec<TabItem>,
    frame_active: &mut usize,
    event: &DraggableTabTransferEvent,
) -> bool {
    let (src_tabs, src_active, dst_tabs, dst_active) =
        match (event.from_bar.as_ref(), event.to_bar.as_ref()) {
            ("bordered", "frame") => (bordered_tabs, bordered_active, frame_tabs, frame_active),
            ("frame", "bordered") => (frame_tabs, frame_active, bordered_tabs, bordered_active),
            ("bordered", "bordered") => {
                return apply_reorder(bordered_tabs, bordered_active, event.from, event.to);
            }
            ("frame", "frame") => {
                return apply_reorder(frame_tabs, frame_active, event.from, event.to);
            }
            _ => return false,
        };

    if event.from >= src_tabs.len() {
        return false;
    }

    let moved = src_tabs.remove(event.from);
    if src_tabs.is_empty() {
        *src_active = 0;
    } else if *src_active > event.from {
        *src_active = src_active.saturating_sub(1);
    } else if *src_active >= src_tabs.len() {
        *src_active = src_tabs.len().saturating_sub(1);
    }

    let insert_at = event.to.min(dst_tabs.len());
    dst_tabs.insert(insert_at, moved);
    if dst_tabs.len() == 1 {
        *dst_active = 0;
    } else if *dst_active >= insert_at {
        *dst_active = dst_active
            .saturating_add(1)
            .min(dst_tabs.len().saturating_sub(1));
    }

    true
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Tabs Hub")
        .mount(TabsHubDemo)
        .run()
}
