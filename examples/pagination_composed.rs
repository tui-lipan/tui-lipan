use std::sync::Arc;

use tui_lipan::prelude::*;

const PAGE_SIZES: &[usize] = &[5, 10, 20, 50];

struct PaginationDemo;

struct State {
    items: Vec<String>,
    pagination: PaginationState,
    page_size_index: usize,
    page_size_open: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    Navigate(PaginationAction),
    PageSizeToggle(bool),
    PageSizeSelect(usize),
}

impl Component for PaginationDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let items = (1..=137)
            .map(|index| format!("Record {:03}", index))
            .collect::<Vec<_>>();
        let page_size_index = 1;
        let pagination = PaginationState::new(items.len(), PAGE_SIZES[page_size_index]);

        State {
            items,
            pagination,
            page_size_index,
            page_size_open: false,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let (start, end) = ctx.state.pagination.range();
        let page_items = ctx.state.items[start..end]
            .iter()
            .map(|item| ListItem::new(item.clone()))
            .collect::<Vec<_>>();

        Frame::new()
            .title("Pagination (Composed Controls)")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        HStack::new()
                            .gap(1)
                            .child(
                                PaginationBar::new(ctx.state.pagination)
                                    .button_variant(ButtonVariant::Outlined)
                                    .button_border_style(BorderStyle::Rounded)
                                    .button_style(Style::new().fg(Color::Cyan))
                                    .button_hover_style(
                                        Style::new().fg(Color::White).bg(Color::Blue),
                                    )
                                    .button_focus_style(
                                        Style::new().fg(Color::Black).bg(Color::Yellow).bold(),
                                    )
                                    .button_disabled_style(Style::new().dim())
                                    .next_button_overrides(
                                        PaginationButtonOverrides::new()
                                            .style(Style::new().fg(Color::Green))
                                            .focus_style(
                                                Style::new()
                                                    .fg(Color::Black)
                                                    .bg(Color::Green)
                                                    .bold(),
                                            ),
                                    )
                                    .prev_button_overrides(
                                        PaginationButtonOverrides::new()
                                            .style(Style::new().fg(Color::Magenta)),
                                    )
                                    .info_formatter(|info| {
                                        Arc::from(format!(
                                            "Pg {} / {}  ·  {}..{} / {}",
                                            info.page_number,
                                            info.total_pages,
                                            if info.total_items == 0 {
                                                0
                                            } else {
                                                info.start + 1
                                            },
                                            info.end,
                                            info.total_items,
                                        ))
                                    })
                                    .info_style(Style::new().bold())
                                    .on_action(ctx.link().callback(Msg::Navigate)),
                            )
                            .child(Text::new("Per page:"))
                            .child(
                                Select::new()
                                    .options(PAGE_SIZES.iter().map(|size| size.to_string()))
                                    .selected(Some(ctx.state.page_size_index))
                                    .expanded(ctx.state.page_size_open)
                                    .on_toggle(ctx.link().callback(Msg::PageSizeToggle))
                                    .on_select(ctx.link().callback(Msg::PageSizeSelect))
                                    .on_change(ctx.link().callback(Msg::PageSizeSelect))
                                    .width(Length::Px(8)),
                            ),
                    )
                    .child(
                        List::new()
                            .items(page_items)
                            .height(Length::Flex(1))
                            .border(true),
                    ),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Navigate(action) => match action {
                PaginationAction::First => ctx.state.pagination.first_page(),
                PaginationAction::Prev => ctx.state.pagination.prev_page(),
                PaginationAction::Next => ctx.state.pagination.next_page(),
                PaginationAction::Last => ctx.state.pagination.last_page(),
            },
            Msg::PageSizeToggle(open) => ctx.state.page_size_open = open,
            Msg::PageSizeSelect(index) => {
                let index = index.min(PAGE_SIZES.len().saturating_sub(1));
                ctx.state.page_size_index = index;
                ctx.state.page_size_open = false;
                ctx.state.pagination.set_per_page(PAGE_SIZES[index]);
                ctx.state.pagination.first_page();
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if matches!(key.code, KeyCode::Char('q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Pagination")
        .mount(PaginationDemo)
        .run()
}
