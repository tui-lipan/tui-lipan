use tui_lipan::prelude::*;

struct WidgetDemo;

#[derive(Default)]
struct State {
    filter: TextInput,
    tab: usize,
    selected: usize,
    scroll: usize,
    status: String,
    border_style: BorderStyle,
    disabled: bool,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    SetBorderStyle(BorderStyle),
    ToggleDisabled,
    SetStatus(&'static str),
}

#[derive(Clone, Debug)]
enum Msg {
    FilterChanged(InputEvent),
    FilterKey(KeyEvent),
    TabChanged(TabsEvent),
    ListSelected(ListEvent),
    ListActivated(ListEvent),
    ListScrollTo(usize),
    Scrolled(ScrollEvent),
    ViewScrollTo(usize),
    Action(Action),
    ActionKey(Action, KeyEvent),
}

const HELP_LINES: usize = 60;

impl Component for WidgetDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            status: "Tab/Shift-Tab move focus, arrows move between buttons, Enter/Space activates, Esc or Ctrl-Q quits."
                .to_string(),
            border_style: BorderStyle::Rounded,
            ..State::default()
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        // Buttons ignore arrow keys, so an arrow only reaches this bubbling handler
        // when a button holds focus. That is the hook for grid-style navigation the
        // framework itself does not provide - it only knows the linear Tab order.
        match key.code {
            KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                match focused_button(ctx).and_then(|cell| button_neighbor(cell, key.code)) {
                    Some(target) => {
                        ctx.request_focus(target);
                        ctx.state.status = format!("Arrow focus: {target}");
                        KeyUpdate::handled(Update::full())
                    }
                    None => KeyUpdate::unhandled(Update::none()),
                }
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let items = filtered_items(ctx.state.tab, ctx.state.filter.text());
        let selected = if items.is_empty() {
            0
        } else {
            ctx.state.selected.min(items.len().saturating_sub(1))
        };

        let detail = items.get(selected).copied().unwrap_or("(no results)");

        let border_style = ctx.state.border_style;
        let disabled = ctx.state.disabled;

        let panel_style = Style::new().bg(Color::indexed(235)).fg(Color::indexed(252));
        let disabled_style = Style {
            fg: Some(Paint::from(Color::indexed(244))),
            dim: Some(true),
            ..Style::new()
        };

        Frame::new()
            .header_left("Widgets")
            .footer_left(ctx.state.status.clone())
            .padding(1)
            .style(panel_style)
            .header_style(Style::new().fg(Color::rgb(88, 166, 255)).bold())
            .footer_style(Style {
                fg: Some(Paint::from(Color::indexed(244))),
                dim: Some(true),
                ..Style::new()
            })
            .border_style(border_style)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        HStack::new()
                            .gap(1)
                            .height(Length::Auto)
                            .align(Align::Center)
                            .child(
                                Button::outlined("Plain")
                                    .border_style(BorderStyle::Plain)
                                    .hover_border_style(Some(BorderStyle::Thick))
                                    .focus_border_style(Some(BorderStyle::Thick))
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .hover_style(Style::new().fg(Color::LightBlue))
                                    .focus_style(Style::new().fg(Color::LightBlue))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetBorderStyle(BorderStyle::Plain))
                                    }))
                                    .on_key(ctx.link().key_handler(|key| {
                                        activation_key(key).then_some(Msg::ActionKey(
                                            Action::SetBorderStyle(BorderStyle::Plain),
                                            key,
                                        ))
                                    }))
                                    .key("btn_plain"),
                            )
                            .child(
                                Button::outlined("Rounded")
                                    .border_style(BorderStyle::Rounded)
                                    .style(Style::new().fg(Color::rgb(88, 166, 255)))
                                    .hover_style(Style::new().fg(Color::White))
                                    .focus_style(Style::new().fg(Color::White))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetBorderStyle(BorderStyle::Rounded))
                                    }))
                                    .on_key(ctx.link().key_handler(|key| {
                                        activation_key(key).then_some(Msg::ActionKey(
                                            Action::SetBorderStyle(BorderStyle::Rounded),
                                            key,
                                        ))
                                    }))
                                    .key("btn_rounded"),
                            )
                            .child(
                                Button::outlined("Double")
                                    .border_style(BorderStyle::Double)
                                    .style(Style::new().fg(Color::rgb(249, 115, 22)))
                                    .hover_style(Style::new().fg(Color::rgb(253, 224, 71)))
                                    .focus_style(Style::new().fg(Color::White))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetBorderStyle(BorderStyle::Double))
                                    }))
                                    .on_key(ctx.link().key_handler(|key| {
                                        activation_key(key).then_some(Msg::ActionKey(
                                            Action::SetBorderStyle(BorderStyle::Double),
                                            key,
                                        ))
                                    }))
                                    .key("btn_double"),
                            )
                            .child(
                                Button::outlined("Thick")
                                    .border_style(BorderStyle::Thick)
                                    .style(Style::new().fg(Color::rgb(16, 185, 129)))
                                    .hover_style(Style::new().fg(Color::rgb(134, 239, 172)))
                                    .focus_style(Style::new().fg(Color::White))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetBorderStyle(BorderStyle::Thick))
                                    }))
                                    .on_key(ctx.link().key_handler(|key| {
                                        activation_key(key).then_some(Msg::ActionKey(
                                            Action::SetBorderStyle(BorderStyle::Thick),
                                            key,
                                        ))
                                    }))
                                    .key("btn_thick"),
                            )
                            .child(
                                Button::filled(if disabled {
                                    "Enable widgets"
                                } else {
                                    "Disable widgets"
                                })
                                .width(Length::Px(18))
                                .style(Style::new().bg(Color::indexed(239)).fg(Color::White))
                                .on_click(
                                    ctx.link().callback(|_| Msg::Action(Action::ToggleDisabled)),
                                )
                                .on_key(ctx.link().key_handler(|key| {
                                    activation_key(key)
                                        .then_some(Msg::ActionKey(Action::ToggleDisabled, key))
                                }))
                                .key("btn_toggle_disabled"),
                            )
                            .child(
                                Button::new("Bracket (disabled)")
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .disabled(true)
                                    .disabled_style(disabled_style)
                                    .key("btn_disabled_preview"),
                            ),
                    )
                    .child(
                        HStack::new()
                            .gap(1)
                            .height(Length::Auto)
                            .align(Align::Center)
                            .child(
                                Button::new("Bracket")
                                    .width(Length::Px(14))
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetStatus("Clicked bracket button."))
                                    }))
                                    .key("demo_bracket"),
                            )
                            .child(
                                Button::filled("Filled")
                                    .width(Length::Px(14))
                                    .style(
                                        Style::new().bg(Color::rgb(88, 166, 255)).fg(Color::Black),
                                    )
                                    .hover_style(
                                        Style::new().bg(Color::rgb(56, 139, 253)).fg(Color::Black),
                                    )
                                    .focus_style(
                                        Style::new().bg(Color::rgb(56, 139, 253)).fg(Color::Black),
                                    )
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetStatus("Clicked filled button."))
                                    }))
                                    .key("demo_filled"),
                            )
                            .child(
                                Button::outlined("Rounded Hover")
                                    .border_style(BorderStyle::Rounded)
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .hover_style(Style::new().fg(Color::LightBlue))
                                    .focus_style(Style::new().fg(Color::White))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetStatus(
                                            "Clicked outlined (hover: border color change).",
                                        ))
                                    }))
                                    .key("demo_bold_rounded"),
                            )
                            .child(
                                Button::outlined("Rounded→Thick")
                                    .border_style(BorderStyle::Rounded)
                                    .hover_border_style(Some(BorderStyle::Thick))
                                    .focus_border_style(Some(BorderStyle::Thick))
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .hover_style(Style::new().fg(Color::LightBlue))
                                    .focus_style(Style::new().fg(Color::White))
                                    .on_click(ctx.link().callback(|_| {
                                        Msg::Action(Action::SetStatus(
                                            "Clicked outlined (hover: border type + color).",
                                        ))
                                    }))
                                    .key("demo_rounded_thick"),
                            ),
                    )
                    .child(
                        Tabs::new()
                            .tab("All")
                            .tab("Fruits")
                            .tab("Veggies")
                            .active(ctx.state.tab)
                            .border(true)
                            .border_style(border_style)
                            .height(Length::Px(3))
                            .padding(Padding {
                                left: 1,
                                right: 1,
                                top: 0,
                                bottom: 0,
                            })
                            .disabled(disabled)
                            .disabled_style(disabled_style)
                            .focusable(true)
                            .on_change(ctx.link().callback(Msg::TabChanged))
                            .key("tabs"),
                    )
                    .child(
                        Input::new(ctx.state.filter.text().to_owned())
                            .cursor(ctx.state.filter.cursor())
                            .placeholder("Type to filter...")
                            .border_style(border_style)
                            .disabled(disabled)
                            .disabled_style(disabled_style)
                            .on_change(ctx.link().callback(Msg::FilterChanged))
                            .on_key(ctx.link().key_handler(|key| {
                                matches!(key.code, KeyCode::Enter).then(|| Msg::FilterKey(key))
                            }))
                            .key("filter"),
                    )
                    .child(
                        HStack::new()
                            .gap(1)
                            .child(
                                List::new()
                                    .border(true)
                                    .border_style(border_style)
                                    .scrollbar(true)
                                    .scrollbar_config(
                                        ScrollbarConfig::new()
                                            .variant(ScrollbarVariant::Integrated),
                                    )
                                    .show_scroll_indicators(true)
                                    .title("Results")
                                    .style(Style::new().fg(Color::indexed(252)))
                                    .selection_style(
                                        Style::new().bg(Color::indexed(24)).fg(Color::White),
                                    )
                                    .selection_symbol(Some(">> "))
                                    .selection_symbol_style(Style::new().fg(Color::Yellow).bold())
                                    .unselected_symbol(Some("   "))
                                    .items(items.iter().map(|s| ListItem::new(*s)))
                                    .selected(selected)
                                    .disabled(disabled)
                                    .disabled_style(disabled_style)
                                    .width(Length::Flex(2))
                                    .on_select(ctx.link().callback(Msg::ListSelected))
                                    .on_scroll_to(ctx.link().callback(Msg::ListScrollTo))
                                    .on_activate(ctx.link().callback(Msg::ListActivated))
                                    .key("list"),
                            )
                            .child(
                                Frame::new()
                                    .header_left("Details")
                                    .padding(1)
                                    .style(
                                        Style::new()
                                            .bg(Color::indexed(236))
                                            .fg(Color::indexed(252)),
                                    )
                                    .header_style(Style::new().fg(Color::rgb(249, 115, 22)).bold())
                                    .border_style(border_style)
                                    .child(
                                        VStack::new()
                                            .gap(1)
                                            .child(Text::new(format!(
                                                "Border style: {border_style:?}",
                                            )))
                                            .child(Text::new(format!(
                                                "Widgets disabled: {disabled}"
                                            )))
                                            .child(Text::new(format!(
                                                "Filter: {}",
                                                ctx.state.filter.text()
                                            )))
                                            .child(Text::new(format!(
                                                "Cursor (byte): {}",
                                                ctx.state.filter.cursor()
                                            )))
                                            .child(Text::new(format!("Selected: {}", detail)))
                                            .child(
                                                ScrollView::new()
                                                    .border(true)
                                                    .border_style(border_style)
                                                    .style(
                                                        Style::new()
                                                            .bg(Color::indexed(237))
                                                            .fg(Color::indexed(252)),
                                                    )
                                                    .scrollbar(true)
                                                    .padding(1)
                                                    .gap(0)
                                                    .offset(ctx.state.scroll)
                                                    .on_scroll(ctx.link().callback(Msg::Scrolled))
                                                    .on_scroll_to(
                                                        ctx.link().callback(Msg::ViewScrollTo),
                                                    )
                                                    .children((0..HELP_LINES).map(|i| {
                                                        Text::new(format!(
                                                            "Mouse wheel scroll line {}",
                                                            i + 1
                                                        ))
                                                        .into()
                                                    }))
                                                    .key("help"),
                                            ),
                                    ),
                            ),
                    ),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FilterChanged(ev) => {
                ctx.state.filter.set_text(ev.value.as_ref().to_string());
                ctx.state.filter.set_cursor(ev.cursor);
                ctx.state.selected = 0;
                Update::full()
            }
            Msg::FilterKey(key) => {
                ctx.state.status = format!("{:?} pressed in filter", key.code);
                Update::full()
            }
            Msg::TabChanged(ev) => {
                ctx.state.tab = ev.index;
                ctx.state.selected = 0;
                Update::full()
            }
            Msg::ListSelected(ev) => {
                ctx.state.selected = ev.index;
                Update::full()
            }
            Msg::ListActivated(ev) => {
                let items = filtered_items(ctx.state.tab, ctx.state.filter.text());
                if let Some(item) = items.get(ev.index) {
                    ctx.state.status = format!("Activated: {}", item);
                } else {
                    ctx.state.status = "Activated: (out of range)".to_string();
                }
                Update::full()
            }
            Msg::ListScrollTo(index) => {
                ctx.state.selected = index;
                Update::full()
            }
            Msg::Scrolled(ev) => {
                let max = HELP_LINES.saturating_sub(1);
                ctx.state.scroll = ev.offset.min(max);
                Update::full()
            }
            Msg::ViewScrollTo(offset) => {
                let max = HELP_LINES.saturating_sub(1);
                ctx.state.scroll = offset.min(max);
                Update::full()
            }
            Msg::Action(action) => {
                apply_action(action, ctx);
                Update::full()
            }
            Msg::ActionKey(action, key) => {
                apply_action(action, ctx);
                ctx.state.status = format!("{} (via {:?})", ctx.state.status, key.code);
                Update::full()
            }
        }
    }
}

/// The two button rows, in layout order. `btn_disabled_preview` is disabled, so it
/// is not focusable and stays out of the grid.
const BUTTON_GRID: [&[&str]; 2] = [
    &[
        "btn_plain",
        "btn_rounded",
        "btn_double",
        "btn_thick",
        "btn_toggle_disabled",
    ],
    &[
        "demo_bracket",
        "demo_filled",
        "demo_bold_rounded",
        "demo_rounded_thick",
    ],
];

/// Locate the focused button in [`BUTTON_GRID`] as `(row, column)`.
fn focused_button(ctx: &Context<WidgetDemo>) -> Option<(usize, usize)> {
    BUTTON_GRID.iter().enumerate().find_map(|(row, keys)| {
        keys.iter()
            .position(|key| ctx.has_focus_within_key(*key))
            .map(|column| (row, column))
    })
}

/// The button an arrow key moves to, or `None` at the edge of the grid so the key
/// stays unhandled and can still reach app commands and framework bindings.
fn button_neighbor((row, column): (usize, usize), code: KeyCode) -> Option<&'static str> {
    let (row, column) = match code {
        KeyCode::Left => (row, column.checked_sub(1)?),
        KeyCode::Right => (row, next_index(column, BUTTON_GRID[row].len())?),
        KeyCode::Up => (row.checked_sub(1)?, column),
        KeyCode::Down => (next_index(row, BUTTON_GRID.len())?, column),
        _ => return None,
    };
    // Rows differ in length; clamp so a vertical move always lands on a button.
    let keys = BUTTON_GRID[row];
    Some(keys[column.min(keys.len() - 1)])
}

fn next_index(index: usize, len: usize) -> Option<usize> {
    (index + 1 < len).then_some(index + 1)
}

/// Keys that activate a button. Every other key must stay unhandled so focus
/// traversal (Tab) and the quit shortcuts keep working.
fn activation_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
}

fn apply_action(action: Action, ctx: &mut Context<WidgetDemo>) {
    match action {
        Action::SetBorderStyle(border_style) => {
            ctx.state.border_style = border_style;
            ctx.state.status = format!("Border style set to {border_style:?}.");
        }
        Action::ToggleDisabled => {
            ctx.state.disabled = !ctx.state.disabled;
            ctx.state.status = if ctx.state.disabled {
                "Disabled Tabs/Input/List (focus + clicks blocked).".to_string()
            } else {
                "Enabled Tabs/Input/List.".to_string()
            };
        }
        Action::SetStatus(status) => {
            ctx.state.status = status.to_string();
        }
    }
}

fn filtered_items(tab: usize, filter: &str) -> Vec<&'static str> {
    const FRUITS: &[&str] = &["Apple", "Banana", "Cherry", "Grape", "Orange", "Pear"];
    const VEGGIES: &[&str] = &[
        "Asparagus",
        "Broccoli",
        "Carrot",
        "Cucumber",
        "Onion",
        "Pea",
    ];

    let base: &[&str] = match tab {
        1 => FRUITS,
        2 => VEGGIES,
        _ => &[],
    };

    let mut items: Vec<&'static str> = if tab == 0 {
        FRUITS.iter().chain(VEGGIES.iter()).copied().collect()
    } else {
        base.to_vec()
    };

    let query = filter.trim();
    if query.is_empty() {
        return items;
    }

    let query = query.to_ascii_lowercase();
    items.retain(|s| s.to_ascii_lowercase().contains(&query));
    items
}

fn main() -> Result<()> {
    App::new()
        .focus_policy(FocusPolicy::Auto)
        .title("tui-lipan - Widgets")
        .mount(WidgetDemo)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    fn backend() -> TestBackend<WidgetDemo> {
        let mut backend = TestBackend::new(WidgetDemo);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 34,
        });
        backend.render();
        backend
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    #[test]
    fn tab_traversal_skips_the_disabled_preview_button() {
        let mut backend = backend();
        backend.focus_key(&Key::from("btn_toggle_disabled"));
        backend.render();
        backend.send_key(key(KeyCode::Tab)).unwrap();
        backend.render();

        assert_eq!(
            backend.focused_key().map(|key| key.to_string()),
            Some("demo_bracket".to_string()),
        );
    }

    #[test]
    fn tab_moves_focus_off_a_button_with_an_on_key_handler() {
        let mut backend = backend();
        backend.focus_key(&Key::from("btn_plain"));
        backend.send_key(key(KeyCode::Tab)).unwrap();
        backend.render();

        assert_eq!(
            backend.focused_key().map(|key| key.to_string()),
            Some("btn_rounded".to_string()),
        );
    }

    #[test]
    fn tab_moves_focus_off_the_filter_input() {
        let mut backend = backend();
        backend.focus_key(&Key::from("filter"));
        backend.send_key(key(KeyCode::Tab)).unwrap();
        backend.render();

        assert_ne!(
            backend.focused_key().map(|key| key.to_string()),
            Some("filter".to_string()),
        );
    }

    fn focus_after_key(from: &'static str, code: KeyCode) -> Option<String> {
        let mut backend = backend();
        backend.focus_key(&Key::from(from));
        backend.render();
        backend.send_key(key(code)).unwrap();
        backend.render();
        backend.focused_key().map(|key| key.to_string())
    }

    #[test]
    fn arrows_move_focus_within_a_button_row() {
        assert_eq!(
            focus_after_key("btn_plain", KeyCode::Right),
            Some("btn_rounded".to_string())
        );
        assert_eq!(
            focus_after_key("btn_rounded", KeyCode::Left),
            Some("btn_plain".to_string())
        );
    }

    #[test]
    fn arrows_move_focus_between_the_two_button_rows() {
        assert_eq!(
            focus_after_key("btn_double", KeyCode::Down),
            Some("demo_bold_rounded".to_string())
        );
        assert_eq!(
            focus_after_key("demo_bracket", KeyCode::Up),
            Some("btn_plain".to_string())
        );
    }

    #[test]
    fn a_vertical_move_clamps_onto_the_shorter_row() {
        assert_eq!(
            focus_after_key("btn_toggle_disabled", KeyCode::Down),
            Some("demo_rounded_thick".to_string())
        );
    }

    #[test]
    fn an_arrow_at_the_grid_edge_leaves_focus_alone() {
        assert_eq!(
            focus_after_key("btn_plain", KeyCode::Left),
            Some("btn_plain".to_string())
        );
        assert_eq!(
            focus_after_key("btn_plain", KeyCode::Up),
            Some("btn_plain".to_string())
        );
    }

    #[test]
    fn arrows_still_belong_to_the_focused_widget() {
        let mut backend = backend();
        backend.focus_key(&Key::from("tabs"));
        backend.render();
        backend.send_key(key(KeyCode::Right)).unwrap();
        backend.render();

        assert_eq!(backend.state().tab, 1, "Tabs must keep consuming Right");
        assert_eq!(
            backend.focused_key().map(|key| key.to_string()),
            Some("tabs".to_string())
        );
        assert_eq!(
            backend.focused_key().map(|key| key.to_string()),
            Some("tabs".to_string())
        );
    }

    #[test]
    fn a_focused_button_leaves_esc_for_the_app_quit_handler() {
        let mut backend = backend();
        backend.focus_key(&Key::from("btn_plain"));
        let before = backend.state().status.clone();
        backend.send_key(key(KeyCode::Esc)).unwrap();

        assert_eq!(
            backend.state().status,
            before,
            "Esc must not be consumed as a button activation"
        );
    }
}
