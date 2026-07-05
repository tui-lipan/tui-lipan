//! Example showcasing hover styles across all interactive widgets.
//!
//! Run with: cargo run --example hover

use tui_lipan::prelude::*;

struct HoverDemo;

#[derive(Default)]
struct State {
    input_value: String,
    input_cursor: usize,
    list_selected: usize,
    table_selected: usize,
    tab_active: usize,
    progress: f64,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    InputChanged(InputEvent),
    ListSelected(ListEvent),
    TableSelected(TableEvent),
    TabChanged(TabsEvent),
    ProgressChanged(ProgressEvent),
}

impl Component for HoverDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            progress: 0.65,
            status: "Move your mouse over widgets to see hover effects. Press 'q' to quit.".into(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::InputChanged(ev) => {
                ctx.state.input_value = ev.value.to_string();
                ctx.state.input_cursor = ev.cursor;
                ctx.state.status = format!("Input: cursor at {}", ev.cursor);
            }
            Msg::ListSelected(ev) => {
                ctx.state.list_selected = ev.index;
                ctx.state.status = format!("List: selected item {}", ev.index);
            }
            Msg::TableSelected(ev) => {
                ctx.state.table_selected = ev.index;
                ctx.state.status = format!("Table: selected row {}", ev.index);
            }
            Msg::TabChanged(ev) => {
                ctx.state.tab_active = ev.index;
                ctx.state.status = format!("Tabs: switched to tab {}", ev.index);
            }
            Msg::ProgressChanged(ev) => {
                ctx.state.progress = ev.progress;
                ctx.state.status = format!("Progress: {:.0}%", ev.progress * 100.0);
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let base_style = Style::new().bg(Color::indexed(235)).fg(Color::indexed(252));
        let hover_highlight = Style::new().bg(Color::indexed(238));
        let hover_border = BorderStyle::Double;
        let item_hover = Style::new().bg(Color::indexed(240)).fg(Color::LightCyan);

        Frame::new()
            .title("Hover Demo")
            .status(ctx.state.status.clone())
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(base_style)
            .title_style(Style::new().fg(Color::LightBlue).bold())
            .status_style(Style::new().fg(Color::indexed(244)).dim())
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(self.input_section(ctx, hover_highlight, hover_border))
                    .child(
                        HStack::new()
                            .gap(1)
                            .child(self.list_section(ctx, hover_highlight, item_hover))
                            .child(self.table_section(ctx, hover_highlight, item_hover)),
                    )
                    .child(self.tabs_section(ctx, hover_highlight))
                    .child(self.progress_section(ctx, hover_highlight))
                    .child(self.frame_section(ctx)),
            )
            .into()
    }
}

impl HoverDemo {
    fn input_section(
        &self,
        ctx: &Context<Self>,
        hover_style: Style,
        hover_border: BorderStyle,
    ) -> Element {
        HStack::new()
            .gap(1)
            .height(Length::Px(3))
            .child(
                Input::new(ctx.state.input_value.clone())
                    .cursor(ctx.state.input_cursor)
                    .placeholder("Type here...")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(hover_style)
                    .hover_border_style(hover_border)
                    .focus_style(Style::new().bg(Color::indexed(238)))
                    .on_change(ctx.link().callback(Msg::InputChanged))
                    .key("input"),
            )
            .into()
    }

    fn list_section(&self, ctx: &Context<Self>, hover_style: Style, item_hover: Style) -> Element {
        Frame::new()
            .title("List (hover items)")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .hover_style(hover_style)
            .style(Style::new().bg(Color::indexed(236)))
            .child(
                List::new()
                    .item("Apple")
                    .item("Banana")
                    .item("Cherry")
                    .item("Date")
                    .item("Elderberry")
                    .item("Fig")
                    .selected(ctx.state.list_selected)
                    .border(false)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(hover_style)
                    .item_hover_style(item_hover)
                    .selection_style(Style::new().bg(Color::indexed(25)).fg(Color::White).bold())
                    .on_select(ctx.link().callback(Msg::ListSelected))
                    .key("list"),
            )
            .into()
    }

    fn table_section(&self, ctx: &Context<Self>, hover_style: Style, item_hover: Style) -> Element {
        let header = TableRow::new(["Name", "Type", "Size"]);

        Frame::new()
            .title("Table (hover rows)")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .hover_style(hover_style)
            .style(Style::new().bg(Color::indexed(236)))
            .child(
                Table::new()
                    .header(header)
                    .row(TableRow::new(["main.rs", "Rust", "2.4 KB"]))
                    .row(TableRow::new(["Cargo.toml", "TOML", "512 B"]))
                    .row(TableRow::new(["README.md", "Markdown", "1.1 KB"]))
                    .row(TableRow::new(["lib.rs", "Rust", "8.2 KB"]))
                    .widths([
                        ColumnWidth::Fill(1),
                        ColumnWidth::Fixed(10),
                        ColumnWidth::Fixed(8),
                    ])
                    .selected(ctx.state.table_selected)
                    .border(false)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(hover_style)
                    .item_hover_style(item_hover)
                    .selection_style(Style::new().bg(Color::indexed(25)).fg(Color::White).bold())
                    .on_select(ctx.link().callback(Msg::TableSelected))
                    .key("table"),
            )
            .into()
    }

    fn tabs_section(&self, ctx: &Context<Self>, hover_style: Style) -> Element {
        let tab_hover = Style::new().fg(Color::LightYellow).bold();

        HStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(Text::new("Tabs:").style(Style::new().fg(Color::indexed(244))))
            .child(
                Tabs::new()
                    .tab("Files")
                    .tab("Search")
                    .tab("Git")
                    .tab("Settings")
                    .active(ctx.state.tab_active)
                    .border(false)
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(hover_style)
                    .tab_hover_style(tab_hover)
                    .active_style(Style::new().fg(Color::LightBlue).bold().reverse())
                    .on_change(ctx.link().callback(Msg::TabChanged))
                    .key("tabs"),
            )
            .into()
    }

    fn progress_section(&self, ctx: &Context<Self>, hover_style: Style) -> Element {
        HStack::new()
            .height(Length::Px(1))
            .gap(1)
            .child(Text::new("Progress:").style(Style::new().fg(Color::indexed(244))))
            .child(
                ProgressBar::new(ctx.state.progress)
                    .progress_style(ProgressStyle::Block)
                    .filled_style(Style::new().fg(Color::LightGreen))
                    .empty_style(Style::new().fg(Color::indexed(238)))
                    .hover_style(hover_style)
                    .draggable(true)
                    .on_change(ctx.link().callback(Msg::ProgressChanged))
                    .key("progress"),
            )
            .into()
    }

    fn frame_section(&self, _ctx: &Context<Self>) -> Element {
        let frame_hover = Style::new().bg(Color::indexed(239)).fg(Color::LightMagenta);

        HStack::new()
            .gap(1)
            .height(Length::Px(5))
            .child(
                Frame::new()
                    .title("Hoverable Frame 1")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(frame_hover)
                    .child(Text::new("Hover me!"))
                    .key("frame1"),
            )
            .child(
                Frame::new()
                    .title("Hoverable Frame 2")
                    .border(true)
                    .border_style(BorderStyle::Plain)
                    .style(Style::new().bg(Color::indexed(236)))
                    .hover_style(frame_hover)
                    .child(Text::new("Me too!"))
                    .key("frame2"),
            )
            .child(
                Frame::new()
                    .title("No Hover Style")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(236)))
                    .child(Text::new("I don't change"))
                    .key("frame3"),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new().mount(HoverDemo).run()
}
