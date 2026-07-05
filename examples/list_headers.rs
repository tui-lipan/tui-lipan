use tui_lipan::prelude::*;

struct ListHeadersDemo;

struct State {
    selected: usize,
    activated: Option<usize>,
    pill_selected: usize,
    pill_full_width: bool,
    pill_padding: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            selected: 1,
            activated: None,
            pill_selected: 0,
            pill_full_width: false,
            pill_padding: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Msg {
    Select(ListEvent),
    Activate(ListEvent),
    SelectPill(ListEvent),
}

fn demo_items() -> Vec<ListItem> {
    vec![
        ListItem::header("Frontend"),
        ListItem::new("src/ui/header.rs").gutter(ListItemGutter::text("●")),
        ListItem::new("src/ui/list.rs").gutter(ListItemGutter::text("●")),
        ListItem::new("src/ui/theme.rs"),
        ListItem::spacer(),
        ListItem::header("Backend"),
        ListItem::new("src/api/routes.rs").gutter(ListItemGutter::text("●")),
        ListItem::new("src/api/service.rs"),
        ListItem::new("src/db/migrations.rs").gutter(ListItemGutter::text("●")),
    ]
}

fn pill_items() -> Vec<ListItem> {
    vec![
        ListItem::new("Overview"),
        ListItem::new("Activity"),
        ListItem::new("Settings"),
        ListItem::new("Members"),
    ]
}

impl Component for ListHeadersDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('f') | KeyCode::Char('F') => {
                ctx.state.pill_full_width = !ctx.state.pill_full_width;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                ctx.state.pill_padding = !ctx.state.pill_padding;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let items = demo_items();
        let selected_bg = Color::rgb(70, 150, 255);
        // Pill demo palette: caps are tinted to `pill_bg` over `panel_bg`.
        let panel_bg = Color::rgb(30, 30, 46);
        let pill_bg = Color::rgb(137, 180, 250);
        let selected_label = items
            .get(ctx.state.selected)
            .filter(|item| item.is_selectable())
            .map(ListItem::plain_content)
            .unwrap_or_else(|| "(none)".to_string());

        let activated_label = ctx
            .state
            .activated
            .and_then(|idx| items.get(idx))
            .map(ListItem::plain_content)
            .unwrap_or_else(|| "(none)".to_string());

        Frame::new()
            .title("List Headers + Gutters")
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .height(Length::Auto)
                    .child(
                        Text::new(
                            "Headers stay left-aligned while selectable rows share a gutter marker column; gutter_gap(1) adds the separator.",
                        )
                        .style(Style::new().fg(Color::DarkGray)),
                    )
                    .child(
                        List::new()
                            .items(items)
                            .selected(ctx.state.selected)
                            .height(Length::Auto)
                            .border(true)
                            .scrollbar(true)
                            .show_scroll_indicators(true)
                            .symbol_column(false)
                            .gutter_gap(1)
                            .gutter_for_non_selectable(false)
                            .selection_style(
                                Style::new()
                                    .bg(selected_bg)
                                    .fg(Color::White)
                                    .contrast_policy(ContrastPolicy::Wcag),
                            )
                            .on_select(ctx.link().callback(Msg::Select))
                            .on_activate(ctx.link().callback(Msg::Activate)),
                    )
                    .child(Text::new(format!("Selected: {}", selected_label)))
                    .child(Text::new(format!("Activated: {}", activated_label)))
                    .child(
                        Text::new(
                            "Pill selection: selection_symbol (left cap) + selection_symbol_right (right cap), tinted to the highlight color with the row background, so the highlight reads as a rounded capsule.",
                        )
                        .style(Style::new().fg(Color::DarkGray)),
                    )
                    .child(
                        Text::new(format!(
                            "[f] selection_full_width = {}    [p] item_horizontal_padding = {}    (cap hugs the label, or jumps to the row edge once the highlight fills the row)",
                            ctx.state.pill_full_width,
                            if ctx.state.pill_padding { "(0, 1)" } else { "(0, 0)" },
                        ))
                        .style(Style::new().fg(Color::rgb(180, 190, 130))),
                    )
                    .child(
                        List::new()
                            .items(pill_items())
                            .selected(ctx.state.pill_selected)
                            .height(Length::Auto)
                            .border(true)
                            .symbol_column(true)
                            .selection_full_width(ctx.state.pill_full_width)
                            .item_horizontal_padding(if ctx.state.pill_padding {
                                Padding::from((0u16, 1u16))
                            } else {
                                Padding::default()
                            })
                            .style(Style::new().bg(panel_bg))
                            // Left/right caps: U+E0B6 / U+E0B4 (Powerline rounded halves).
                            .selection_symbol(Some("\u{e0b6}"))
                            .selection_symbol_right(Some("\u{e0b4}"))
                            // Caps share this style: fg = highlight color, bg = row background.
                            .selection_symbol_style(Style::new().fg(pill_bg).bg(panel_bg))
                            .selection_style(Style::new().bg(pill_bg).fg(panel_bg).bold())
                            .on_select(ctx.link().callback(Msg::SelectPill)),
                    ),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(event) => {
                ctx.state.selected = event.index;
                Update::full()
            }
            Msg::Activate(event) => {
                ctx.state.activated = Some(event.index);
                Update::full()
            }
            Msg::SelectPill(event) => {
                ctx.state.pill_selected = event.index;
                Update::full()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - List Headers")
        .mount(ListHeadersDemo)
        .run()
}
