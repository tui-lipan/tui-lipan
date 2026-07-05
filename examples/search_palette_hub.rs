use std::sync::Arc;

use tui_lipan::prelude::*;

const CONTROLLED_ITEMS: &[(&str, &str)] = &[
    ("src/lib.rs", "Crate root & re-exports"),
    ("src/app.rs", "Application entrypoint"),
    ("src/style/theme.rs", "Style and theming primitives"),
    ("src/widgets/list/mod.rs", "List widget"),
    ("src/widgets/frame/mod.rs", "Frame widget"),
    ("src/widgets/search_palette/mod.rs", "Search palette widget"),
    (
        "src/widgets/search_palette/component.rs",
        "Palette component",
    ),
    (
        "src/widgets/search_palette/matching.rs",
        "Nucleo fuzzy matching",
    ),
    ("src/widgets/text_area/mod.rs", "Multi-line text editor"),
    ("src/widgets/input/mod.rs", "Single-line text input"),
    ("src/core/component.rs", "Component trait & runtime"),
    ("src/core/element.rs", "Element tree node"),
    ("src/utils/nucleo.rs", "Nucleo matcher wrapper"),
    ("examples/search_palette_hub.rs", "This demo"),
    ("examples/frame_hub.rs", "Frame demos hub"),
    ("examples/forms.rs", "Form inputs demo"),
];

const POPOVER_PLACEMENTS: [PopoverPlacement; 4] = [
    PopoverPlacement::BelowStart,
    PopoverPlacement::AboveStart,
    PopoverPlacement::RightStart,
    PopoverPlacement::LeftStart,
];

const PLACEMENTS: [(&str, DescriptionPlacement, MultiSelectDescriptionPlacement); 4] = [
    (
        "Inline",
        DescriptionPlacement::Inline,
        MultiSelectDescriptionPlacement::Inline,
    ),
    (
        "Right",
        DescriptionPlacement::Right,
        MultiSelectDescriptionPlacement::Right,
    ),
    (
        "Above",
        DescriptionPlacement::Above,
        MultiSelectDescriptionPlacement::Above,
    ),
    (
        "Below",
        DescriptionPlacement::Below,
        MultiSelectDescriptionPlacement::Below,
    ),
];

struct SearchPaletteHub;

struct State {
    active_tab: usize,

    // Uncontrolled palette
    palette_show: bool,
    palette_last_selected: Option<Arc<str>>,
    palette_transparent_frame: bool,

    // Controlled palette
    controlled_query: TextInput,
    controlled_last_activated: Option<Arc<str>>,

    // Delete flow palette
    delete_show: bool,
    delete_items: Vec<(Arc<str>, Arc<str>)>,
    delete_pending: Option<usize>,
    delete_selected_index: Option<usize>,

    // Description placement showcase
    placement_index: usize,
    description_selection: bool,
    description_wrap: bool,
    multi_active_index: usize,
    multi_selected: Vec<usize>,
    placement_status: String,

    // Overlays tab
    popover_open: bool,
    popover_placement_index: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active_tab: 0,
            palette_show: false,
            palette_last_selected: None,
            palette_transparent_frame: false,
            controlled_query: TextInput::default(),
            controlled_last_activated: None,
            delete_show: false,
            delete_items: default_delete_items(),
            delete_pending: None,
            delete_selected_index: None,
            placement_index: 0,
            description_selection: false,
            description_wrap: false,
            multi_active_index: 0,
            multi_selected: vec![1],
            placement_status: "Use Left/Right (or h/l) to change placement".to_string(),

            popover_open: false,
            popover_placement_index: 0,
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),

    PaletteToggle(bool),
    PaletteSelected(SearchEvent<Arc<str>>),
    PaletteActivated(SearchEvent<Arc<str>>),

    ControlledQueryChanged(InputEvent),
    ControlledActivated(SearchEvent<Arc<str>>),

    DeleteToggle(bool),
    DeleteSelected(SearchEvent<usize>),
    DeleteActivated(SearchEvent<usize>),
    DeleteRequest,

    NextPlacement,
    PrevPlacement,
    ToggleDescriptionHighlight,
    ToggleDescriptionOverflow,
    MultiHighlightChanged(usize),
    MultiChanged(MultiSelectChangeEvent),
    MultiCommit(MultiSelectCommitEvent),
    PlacementActivated(SearchEvent<Arc<str>>),

    PopoverToggle,
    PopoverClose,
    NextPopoverPlacement,
    PrevPopoverPlacement,
}

impl SearchPaletteHub {
    fn view_uncontrolled_palette(&self, ctx: &Context<Self>) -> Element {
        let hint = if ctx.state.palette_show {
            "Esc close  |  t transparent frame (Ctrl+t from filter)  |  ↑↓  |  Enter"
        } else {
            "Press '/' to open this palette"
        };

        let color_blocks = HStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x1E, 0x40, 0xAF))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Blue Section\nFiles:  1,024\nLines: 48,391")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x16, 0x5A, 0x32))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Green Section\nTests:   312\nPassed: 312")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x78, 0x35, 0x00))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Orange Section\nWarnings: 7\nErrors:   0")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x6B, 0x21, 0xA8))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Purple Section\nCoverage: 87%\nDelta:  +2%")),
            );

        let text_rows = VStack::new()
            .gap(0)
            .child(
                Text::new("src/lib.rs              - Crate root & re-exports")
                    .style(Style::new().fg(Color::Cyan)),
            )
            .child(
                Text::new("src/app.rs              - Application entrypoint")
                    .style(Style::new().fg(Color::LightGreen)),
            )
            .child(
                Text::new("src/style/theme.rs      - Style and theming primitives")
                    .style(Style::new().fg(Color::Yellow)),
            )
            .child(
                Text::new("src/widgets/list/mod.rs - List widget")
                    .style(Style::new().fg(Color::LightMagenta)),
            )
            .child(
                Text::new("src/widgets/modal.rs    - Modal dialog widget")
                    .style(Style::new().fg(Color::LightCyan)),
            )
            .child(
                Text::new("src/backend/common.rs   - Renderer utilities")
                    .style(Style::new().fg(Color::Transparent)),
            );

        let mut body = VStack::new()
            .gap(1)
            .child(Text::new(hint).style(Style::new().fg(Color::DarkGray)))
            .child(color_blocks)
            .child(text_rows);

        if let Some(path) = &ctx.state.palette_last_selected {
            body = body
                .child(Text::new(format!("Opened: {path}")).style(Style::new().fg(Color::Green)));
        }

        if ctx.state.palette_show {
            let entries = vec![
                SearchEntry::header("Sources"),
                SearchEntry::item("src/lib.rs", Arc::from("src/lib.rs"))
                    .description("Crate root & re-exports"),
                SearchEntry::item("src/app.rs", Arc::from("src/app.rs"))
                    .description("Application entrypoint"),
                SearchEntry::item("src/style/theme.rs", Arc::from("src/style/theme.rs"))
                    .description("Style and theming primitives"),
                SearchEntry::spacer(),
                SearchEntry::header("Widgets"),
                SearchEntry::item(
                    "src/widgets/list/mod.rs",
                    Arc::from("src/widgets/list/mod.rs"),
                )
                .description("List widget"),
                SearchEntry::item(
                    "src/widgets/search_palette/mod.rs",
                    Arc::from("src/widgets/search_palette/mod.rs"),
                )
                .description("Search palette widget"),
                SearchEntry::item(
                    "src/widgets/text_area/mod.rs",
                    Arc::from("src/widgets/text_area/mod.rs"),
                )
                .description("Multi-line text editor"),
                SearchEntry::spacer(),
                SearchEntry::header("Examples"),
                SearchEntry::item(
                    "examples/search_palette_hub.rs",
                    Arc::from("examples/search_palette_hub.rs"),
                )
                .description("This demo"),
                SearchEntry::item(
                    "examples/markdown_hub.rs",
                    Arc::from("examples/markdown_hub.rs"),
                )
                .description("Markdown rendering demo"),
                SearchEntry::item(
                    "examples/markdown_editor_sync.rs",
                    Arc::from("examples/markdown_editor_sync.rs"),
                )
                .description("Synchronized editor + preview"),
            ];

            let palette = SearchPalette::<Arc<str>>::new()
                .entries(entries)
                .height(Length::Auto)
                .input_border(false)
                .list_border(false)
                .list_scrollbar(true)
                .list_selection_full_width(true)
                .list_item_hover_style(Style::new().bg(Color::DarkGray))
                .on_select(ctx.link().callback(Msg::PaletteSelected))
                .on_activate(ctx.link().callback(Msg::PaletteActivated));

            let mut modal = Modal::new()
                .title("Open File")
                .child(palette)
                .width(Length::Px(60))
                .height(Length::Auto)
                .border_style(BorderStyle::Rounded)
                .padding(0)
                .backdrop_style(Style::new().tint_by(Color::rgb(10, 20, 60), 0.55))
                .on_close(ctx.link().callback(|_| Msg::PaletteToggle(false)));

            modal = if ctx.state.palette_transparent_frame {
                modal.frame_style(Style::new().bg(Color::Transparent))
            } else {
                modal
            };

            body = body.child(modal.key("hub-uncontrolled-palette"));
        }

        Frame::new()
            .title("Uncontrolled SearchPalette")
            .border(true)
            .padding(1)
            .child(body)
            .into()
    }

    fn view_controlled_palette(&self, ctx: &Context<Self>) -> Element {
        let items: Vec<SearchItem<Arc<str>>> = CONTROLLED_ITEMS
            .iter()
            .map(|(path, desc)| SearchItem::new(*path, Arc::from(*path)).description(*desc))
            .collect();

        let palette = SearchPalette::new()
            .items(items)
            .query(Arc::from(ctx.state.controlled_query.text()))
            .list_selection_full_width(true)
            .list_scrollbar(true)
            .list_item_hover_style(Style::new().bg(Color::Indexed(237)))
            .on_activate(ctx.link().callback(Msg::ControlledActivated));

        let query_input = Element::from(
            Input::new(ctx.state.controlled_query.text().to_owned())
                .cursor(ctx.state.controlled_query.cursor())
                .anchor(ctx.state.controlled_query.anchor())
                .placeholder("filter files...")
                .border(false)
                .padding(0)
                .focus_style(Style::new().fg(Color::White))
                .placeholder_style(Style::new().fg(Color::DarkGray).dim())
                .on_change(ctx.link().callback(Msg::ControlledQueryChanged)),
        )
        .min_width(Length::Px(18))
        .max_width(Length::Px(36));

        let mut body = VStack::new().gap(1);

        if let Some(path) = &ctx.state.controlled_last_activated {
            body = body.child(
                Text::new(format!("Opened: {path}")).style(Style::new().fg(Color::LightGreen)),
            );
        }

        body.child(
            Frame::new()
                .title(" Files ")
                .border_style(BorderStyle::Rounded)
                .height(Length::Flex(1))
                .header(query_input)
                .header_padding(5)
                .child(VStack::new().gap(0).child(palette)),
        )
        .child(Text::new("↑↓ navigate  Enter open  q quit").style(Style::new().fg(Color::DarkGray)))
        .into()
    }

    fn view_delete_palette(&self, ctx: &Context<Self>) -> Element {
        let hint = if ctx.state.delete_show {
            "Esc close  |  ↑↓ navigate  |  Enter open  |  Ctrl+D delete (press twice)"
        } else {
            "Press '/' to open this palette"
        };

        let mut body = VStack::new()
            .gap(1)
            .child(Text::new(hint).style(Style::new().fg(Color::DarkGray)));

        if ctx.state.delete_show {
            let entries: Vec<SearchEntry<usize>> = ctx
                .state
                .delete_items
                .iter()
                .enumerate()
                .map(|(i, (label, desc))| {
                    SearchEntry::item(label.clone(), i).description(desc.clone())
                })
                .collect();

            let pending_for_render = ctx.state.delete_pending;
            let items_for_render = ctx.state.delete_items.clone();

            let palette = SearchPalette::<usize>::new()
                .entries(entries)
                .sync_selection(true)
                .height(Length::Auto)
                .input_border(false)
                .list_border(false)
                .list_scrollbar(true)
                .list_selection_full_width(true)
                .list_item_hover_style(Style::new().bg(Color::DarkGray))
                .input_key_interceptor(ctx.link().key_handler(|key| {
                    if key.code == KeyCode::Char('d') && key.mods.ctrl {
                        Some(Msg::DeleteRequest)
                    } else {
                        None
                    }
                }))
                .on_select(ctx.link().callback(Msg::DeleteSelected))
                .on_activate(ctx.link().callback(Msg::DeleteActivated))
                .render_item(Arc::new(move |item: &SearchItem<usize>, _highlight| {
                    let idx = item.value;
                    let (label, _desc) = &items_for_render[idx];
                    let is_pending = pending_for_render == Some(idx);

                    if is_pending {
                        Some(ListItem::from_spans(vec![
                            Span::new("⚠ ").style(Style::new().fg(Color::Red)),
                            Span::new(label.as_ref())
                                .style(Style::new().fg(Color::Red).strikethrough()),
                            Span::new("  press Ctrl+D again to confirm")
                                .style(Style::new().fg(Color::DarkGray).italic()),
                        ]))
                    } else {
                        None
                    }
                }));

            body = body.child(
                Modal::new()
                    .title("Delete Demo")
                    .child(palette)
                    .width(Length::Px(65))
                    .height(Length::Auto)
                    .border_style(BorderStyle::Rounded)
                    .padding(0)
                    .backdrop_style(Style::new().tint_by(Color::rgb(10, 20, 60), 0.55))
                    .on_close(ctx.link().callback(|_| Msg::DeleteToggle(false)))
                    .key("hub-delete-palette"),
            );
        }

        Frame::new()
            .title("Delete Workflow")
            .border(true)
            .padding(1)
            .child(body)
            .into()
    }

    fn view_overlays_tab(&self, ctx: &Context<Self>) -> Element {
        let placement = POPOVER_PLACEMENTS[ctx.state.popover_placement_index];

        let popover_content = Frame::new()
            .title("Popover Content")
            .padding(1)
            .child(Text::new("This is a popover!").style(Style::new().fg(Color::Green)))
            .border(true)
            .border_style(BorderStyle::Double);

        Frame::new()
            .title("Overlays")
            .border(true)
            .padding(1)
            .child(
                Center::new().child(
                    VStack::new()
                        .gap(1)
                        .child(Text::new("Click the button or press 'p' to toggle popover"))
                        .child(
                            Text::new(format!(
                                "Placement: {:?}  |  Left/Right or h/l to change",
                                placement
                            ))
                            .style(Style::new().fg(Color::Cyan)),
                        )
                        .child(
                            Popover::new()
                                .trigger(
                                    Button::filled("Toggle Popover")
                                        .on_click(ctx.link().callback(|_| Msg::PopoverToggle)),
                                )
                                .content(popover_content)
                                .open(ctx.state.popover_open)
                                .placement(placement)
                                .offset((0, 1))
                                .on_close(ctx.link().callback(|_| Msg::PopoverClose)),
                        ),
                ),
            )
            .into()
    }

    fn view_description_placement(&self, ctx: &Context<Self>) -> Element {
        let (placement_name, search_placement, multi_placement) =
            PLACEMENTS[ctx.state.placement_index];

        let entries = vec![
            SearchEntry::header("Project"),
            SearchEntry::item("src/lib.rs", Arc::from("src/lib.rs"))
                .description("Crate root and exports"),
            SearchEntry::item(
                "src/widgets/list/mod.rs",
                Arc::from("src/widgets/list/mod.rs"),
            )
            .description("List widget API and behavior"),
            SearchEntry::item(
                "src/widgets/search_palette/mod.rs",
                Arc::from("src/widgets/search_palette/mod.rs"),
            )
            .description("Search palette API"),
        ];

        let multi_items = vec![
            MultiSelectItem::new("Cargo.toml").description("Workspace manifest"),
            MultiSelectItem::new("src/lib.rs").description("Public API entrypoint"),
            MultiSelectItem::new("docs/widgets/overlays.md")
                .description("SearchPalette and Modal docs"),
            MultiSelectItem::new("examples/search_palette_hub.rs").description("Hub demo"),
        ];

        let controls = format!(
            "Placement: {} | description_selection: {} | description_overflow: {} (Above/Below only) | Left/Right or h/l, d/w toggle, q quit",
            placement_name,
            if ctx.state.description_selection {
                "on"
            } else {
                "off"
            },
            if ctx.state.description_wrap {
                "wrap"
            } else {
                "truncate"
            }
        );

        let description_overflow = if ctx.state.description_wrap {
            DescriptionOverflow::Wrap
        } else {
            DescriptionOverflow::Truncate
        };

        let multi_description_overflow = if ctx.state.description_wrap {
            MultiSelectDescriptionOverflow::Wrap
        } else {
            MultiSelectDescriptionOverflow::Truncate
        };

        Frame::new()
            .title("Description Placement")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(controls).style(Style::new().fg(Color::Cyan)))
                    .child(
                        HStack::new()
                            .gap(1)
                            .height(Length::Px(16))
                            .child(
                                Frame::new()
                                    .title("SearchPalette")
                                    .border(true)
                                    .height(Length::Flex(1))
                                    .child(
                                        SearchPalette::<Arc<str>>::new()
                                            .entries(entries)
                                            .description_placement(search_placement)
                                            .description_selection(ctx.state.description_selection)
                                            .description_overflow(description_overflow)
                                            .list_selection_full_width(true)
                                            .list_scrollbar(true)
                                            .on_activate(
                                                ctx.link().callback(Msg::PlacementActivated),
                                            ),
                                    ),
                            )
                            .child(
                                Frame::new()
                                    .title("MultiSelect")
                                    .border(true)
                                    .height(Length::Flex(1))
                                    .child(
                                        MultiSelect::new()
                                            .items(multi_items)
                                            .description_placement(multi_placement)
                                            .description_overflow(multi_description_overflow)
                                            .description_selection(ctx.state.description_selection)
                                            .selection_full_width(true)
                                            .active_index(ctx.state.multi_active_index)
                                            .selected_indices(ctx.state.multi_selected.clone())
                                            .on_active_index_change(
                                                ctx.link().callback(Msg::MultiHighlightChanged),
                                            )
                                            .on_change(ctx.link().callback(Msg::MultiChanged))
                                            .on_commit(ctx.link().callback(Msg::MultiCommit)),
                                    ),
                            ),
                    )
                    .child(Text::new(format!("Status: {}", ctx.state.placement_status))),
            )
            .into()
    }
}

impl Component for SearchPaletteHub {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tabs = Tabs::new()
            .tabs(vec![
                Tab::new("Palette"),
                Tab::new("Controlled"),
                Tab::new("Delete"),
                Tab::new("Description"),
                Tab::new("Overlays"),
            ])
            .active(ctx.state.active_tab.min(4))
            .on_change(ctx.link().callback(Msg::TabChanged));

        let mode_hint = match ctx.state.active_tab {
            0 => "Palette mode: / open, t/Ctrl+t toggle frame tint, q quit when closed",
            1 => "Controlled mode: type in header input, Enter activate, q quit",
            2 => "Delete mode: / open, Ctrl+d twice to delete selected, q quit when closed",
            3 => "Description mode: Left/Right or h/l, d/w toggles, q quit",
            4 => "Overlays mode: p toggle, Esc close, Left/Right or h/l placement, q quit",
            _ => "",
        };

        let content = match ctx.state.active_tab {
            0 => self.view_uncontrolled_palette(ctx),
            1 => self.view_controlled_palette(ctx),
            2 => self.view_delete_palette(ctx),
            3 => self.view_description_placement(ctx),
            4 => self.view_overlays_tab(ctx),
            _ => self.view_uncontrolled_palette(ctx),
        };

        Frame::new()
            .title("Search Palette Hub")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new("Tabs: click or Ctrl+1..5 to switch variants")
                            .style(Style::new().fg(Color::DarkGray)),
                    )
                    .child(Text::new(mode_hint).style(Style::new().fg(Color::DarkGray)))
                    .child(tabs)
                    .child(content),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(ev) => {
                ctx.state.active_tab = ev.index.min(4);
                Update::full()
            }

            Msg::PaletteToggle(show) => {
                ctx.state.palette_show = show;
                Update::full()
            }
            Msg::PaletteSelected(event) => {
                ctx.state.palette_last_selected = Some(event.item.value);
                Update::full()
            }
            Msg::PaletteActivated(event) => {
                ctx.state.palette_last_selected = Some(event.item.value);
                ctx.state.palette_show = false;
                Update::full()
            }

            Msg::ControlledQueryChanged(ev) => {
                ctx.state
                    .controlled_query
                    .set_text(ev.value.as_ref().to_string());
                ctx.state.controlled_query.set_cursor(ev.cursor);
                ctx.state.controlled_query.set_anchor(ev.anchor);
                Update::full()
            }
            Msg::ControlledActivated(ev) => {
                ctx.state.controlled_last_activated = Some(ev.item.value);
                Update::full()
            }

            Msg::DeleteToggle(show) => {
                ctx.state.delete_show = show;
                ctx.state.delete_pending = None;
                Update::full()
            }
            Msg::DeleteSelected(event) => {
                ctx.state.delete_pending = None;
                ctx.state.delete_selected_index = Some(event.item.value);
                Update::full()
            }
            Msg::DeleteActivated(_event) => Update::none(),
            Msg::DeleteRequest => {
                let Some(selected) = ctx.state.delete_selected_index else {
                    return Update::none();
                };

                if ctx.state.delete_pending == Some(selected) {
                    ctx.state.delete_items.remove(selected);
                    ctx.state.delete_pending = None;

                    if ctx.state.delete_items.is_empty() {
                        ctx.state.delete_selected_index = None;
                    } else if selected >= ctx.state.delete_items.len() {
                        ctx.state.delete_selected_index = Some(ctx.state.delete_items.len() - 1);
                    }
                } else {
                    ctx.state.delete_pending = Some(selected);
                }

                Update::full()
            }

            Msg::NextPlacement => {
                ctx.state.placement_index = (ctx.state.placement_index + 1) % PLACEMENTS.len();
                Update::full()
            }
            Msg::PrevPlacement => {
                let len = PLACEMENTS.len();
                ctx.state.placement_index = (ctx.state.placement_index + len - 1) % len;
                Update::full()
            }
            Msg::ToggleDescriptionHighlight => {
                ctx.state.description_selection = !ctx.state.description_selection;
                Update::full()
            }
            Msg::ToggleDescriptionOverflow => {
                ctx.state.description_wrap = !ctx.state.description_wrap;
                Update::full()
            }
            Msg::MultiHighlightChanged(index) => {
                ctx.state.multi_active_index = index;
                Update::full()
            }
            Msg::MultiChanged(event) => {
                ctx.state.multi_selected = event.selected_indices;
                Update::full()
            }
            Msg::MultiCommit(event) => {
                ctx.state.placement_status =
                    format!("Committed {} selected items", event.selected_indices.len());
                Update::full()
            }
            Msg::PlacementActivated(event) => {
                ctx.state.placement_status = format!("Activated {}", event.item.label);
                Update::full()
            }

            Msg::PopoverToggle => {
                ctx.state.popover_open = !ctx.state.popover_open;
                Update::full()
            }
            Msg::PopoverClose => {
                ctx.state.popover_open = false;
                Update::full()
            }
            Msg::NextPopoverPlacement => {
                ctx.state.popover_placement_index =
                    (ctx.state.popover_placement_index + 1) % POPOVER_PLACEMENTS.len();
                Update::full()
            }
            Msg::PrevPopoverPlacement => {
                let len = POPOVER_PLACEMENTS.len();
                ctx.state.popover_placement_index =
                    (ctx.state.popover_placement_index + len - 1) % len;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let ctrl_only = key.mods.ctrl && !key.mods.alt && !key.mods.shift;
        if ctrl_only {
            let tab_index = match key.code {
                KeyCode::Char('1') => Some(0),
                KeyCode::Char('2') => Some(1),
                KeyCode::Char('3') => Some(2),
                KeyCode::Char('4') => Some(3),
                KeyCode::Char('5') => Some(4),
                _ => None,
            };

            if let Some(index) = tab_index {
                ctx.state.active_tab = index;
                return KeyUpdate::handled(Update::full());
            }
        }

        match ctx.state.active_tab {
            0 => {
                if ctx.state.palette_show && matches!(key.code, KeyCode::Char('t')) {
                    let plain = key.mods == KeyMods::default();
                    if plain || ctrl_only {
                        ctx.state.palette_transparent_frame = !ctx.state.palette_transparent_frame;
                        return KeyUpdate::handled(Update::full());
                    }
                }

                match key.code {
                    KeyCode::Char('/') if !ctx.state.palette_show => {
                        ctx.state.palette_show = true;
                        KeyUpdate::handled(Update::full())
                    }
                    KeyCode::Char('q') if !ctx.state.palette_show => {
                        ctx.quit();
                        KeyUpdate::handled(Update::full())
                    }
                    _ => KeyUpdate::unhandled(Update::none()),
                }
            }
            1 => {
                if key.code == KeyCode::Char('q') && key.mods == KeyMods::default() {
                    ctx.quit();
                    KeyUpdate::handled(Update::full())
                } else {
                    KeyUpdate::unhandled(Update::none())
                }
            }
            2 => match key.code {
                KeyCode::Char('/') if !ctx.state.delete_show => {
                    ctx.state.delete_show = true;
                    KeyUpdate::handled(Update::full())
                }
                KeyCode::Char('q') if !ctx.state.delete_show => {
                    ctx.quit();
                    KeyUpdate::handled(Update::full())
                }
                _ => KeyUpdate::unhandled(Update::none()),
            },
            3 => {
                if key.mods != KeyMods::default() {
                    return KeyUpdate::unhandled(Update::none());
                }

                let msg = match key.code {
                    KeyCode::Left | KeyCode::Char('h') => Some(Msg::PrevPlacement),
                    KeyCode::Right | KeyCode::Char('l') => Some(Msg::NextPlacement),
                    KeyCode::Char('d') => Some(Msg::ToggleDescriptionHighlight),
                    KeyCode::Char('w') => Some(Msg::ToggleDescriptionOverflow),
                    KeyCode::Char('q') => {
                        ctx.quit();
                        return KeyUpdate::handled(Update::full());
                    }
                    _ => None,
                };

                if let Some(msg) = msg {
                    return KeyUpdate::handled(self.update(msg, ctx));
                }

                KeyUpdate::unhandled(Update::none())
            }
            4 => {
                if key.mods != KeyMods::default() {
                    return KeyUpdate::unhandled(Update::none());
                }

                if key.code == KeyCode::Esc && ctx.state.popover_open {
                    ctx.state.popover_open = false;
                    return KeyUpdate::handled(Update::full());
                }

                let msg = match key.code {
                    KeyCode::Char('p') => Some(Msg::PopoverToggle),
                    KeyCode::Left | KeyCode::Char('h') => Some(Msg::PrevPopoverPlacement),
                    KeyCode::Right | KeyCode::Char('l') => Some(Msg::NextPopoverPlacement),
                    KeyCode::Char('q') => {
                        ctx.quit();
                        return KeyUpdate::handled(Update::full());
                    }
                    _ => None,
                };

                if let Some(msg) = msg {
                    return KeyUpdate::handled(self.update(msg, ctx));
                }

                KeyUpdate::unhandled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn default_delete_items() -> Vec<(Arc<str>, Arc<str>)> {
    vec![
        (
            Arc::from("src/lib.rs"),
            Arc::from("Crate root & re-exports"),
        ),
        (Arc::from("src/app.rs"), Arc::from("Application entrypoint")),
        (
            Arc::from("src/style/theme.rs"),
            Arc::from("Style and theming primitives"),
        ),
        (
            Arc::from("src/widgets/list/mod.rs"),
            Arc::from("List widget"),
        ),
        (
            Arc::from("src/widgets/search_palette/mod.rs"),
            Arc::from("Search palette widget"),
        ),
        (
            Arc::from("src/widgets/text_area/mod.rs"),
            Arc::from("Multi-line text editor"),
        ),
        (
            Arc::from("examples/search_palette_hub.rs"),
            Arc::from("Search palette hub demo"),
        ),
        (
            Arc::from("examples/markdown_hub.rs"),
            Arc::from("Markdown rendering demo"),
        ),
    ]
}

fn main() -> Result<()> {
    App::new()
        .title("SearchPalette Hub")
        .mount(SearchPaletteHub)
        .run()
}
