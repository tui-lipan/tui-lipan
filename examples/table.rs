use tui_lipan::prelude::*;

struct TableDemo;

#[derive(Default)]
struct State {
    selected: usize,
    active_tab: usize,
}

#[derive(Clone, Copy, Debug)]
enum Msg {
    Select(TableEvent),
    ScrollTo(usize),
    TabChanged(TabsEvent),
}

impl Component for TableDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let content: Element = match ctx.state.active_tab {
            0 => {
                let header = TableRow::new(vec!["ID", "Name", "Role", "Active"])
                    .style(Style::new().bold().bg(Color::DarkGray).fg(Color::White))
                    .bottom_margin(1);

                let rows: Vec<TableRow> = (0..50)
                    .map(|i| {
                        TableRow::new(vec![
                            TableCell::new(format!("{:03}", i)),
                            TableCell::new(format!("User {}", i)),
                            TableCell::new(if i % 3 == 0 { "Admin" } else { "User" }).style(
                                Style::new().fg(if i % 3 == 0 { Color::Red } else { Color::Green }),
                            ),
                            TableCell::new(if i % 2 == 0 { "Yes" } else { "No" }),
                        ])
                    })
                    .collect();

                Table::new()
                    .header(header)
                    .rows(rows)
                    .widths(vec![
                        ColumnWidth::Fixed(5),
                        ColumnWidth::Min(10),
                        ColumnWidth::Min(10),
                        ColumnWidth::Fixed(6),
                    ])
                    .column_spacing(2)
                    .selection_symbol(Some(">> "))
                    .selection_style(Style::new().bg(Color::Blue).fg(Color::White))
                    .alternating_row_style(Style::new().bg(Color::indexed(236)))
                    .row_style_full_width(true)
                    .selected(ctx.state.selected)
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .scrollbar(true)
                    .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Integrated))
                    .on_select(ctx.link().callback(Msg::Select))
                    .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                    .into()
            }
            _ => Table::new()
                .inspector_preset()
                .border(true)
                .scrollbar(true)
                .show_scroll_indicators(true)
                .header(TableRow::new(["Property", "Value"]).style(Style::new().bold()))
                .rows([
                    TableRow::section("Metadata"),
                    TableRow::key_value("Name", "tui-lipan"),
                    TableRow::key_value("Version", "0.1.0"),
                    TableRow::key_value("Language", "Rust"),
                    TableRow::separator(),
                    TableRow::section("Runtime"),
                    TableRow::key_value("Mouse", "enabled"),
                    TableRow::key_value("Focus", "single-root tree"),
                    TableRow::key_value("Events", "callbacks + message queue"),
                    TableRow::separator(),
                    TableRow::section("Cluster")
                        .depth(0)
                        .disclosure(TableDisclosureState::Expanded),
                    TableRow::key_value("Namespace", "default").depth(1),
                    TableRow::key_value("Replicas", "3").depth(1),
                    TableRow::key_value("Autoscaling", "on")
                        .depth(1)
                        .disclosure(TableDisclosureState::Collapsed),
                ])
                .selected(6)
                .into(),
        };

        let title = match ctx.state.active_tab {
            0 => "Table Demo",
            _ => "Table Inspector",
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Tabs::new()
                    .tab("Basic")
                    .tab("Inspector")
                    .active(ctx.state.active_tab)
                    .border(true)
                    .height(Length::Px(3))
                    .on_change(ctx.link().callback(Msg::TabChanged)),
            )
            .child(
                Frame::new()
                    .title(title)
                    .padding(1)
                    .border(true)
                    .height(Length::Flex(1))
                    .child(content),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(ev) => {
                ctx.state.selected = ev.index;
                Update::full()
            }
            Msg::ScrollTo(index) => {
                ctx.state.selected = index;
                Update::full()
            }
            Msg::TabChanged(ev) => {
                ctx.state.active_tab = ev.index;
                Update::full()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Table Demo")
        .mount(TableDemo)
        .run()
}
