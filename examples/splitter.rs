use tui_lipan::prelude::*;

struct SplitterDemo;

#[derive(Default)]
struct State {
    mode_tab: usize,
    list_selected: usize,
    table_selected: usize,
}

#[derive(Clone, Copy, Debug)]
enum Msg {
    ModeTabChanged(TabsEvent),
    ListSelect(ListEvent),
    TableSelect(TableEvent),
}

impl Component for SplitterDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ModeTabChanged(ev) => {
                ctx.state.mode_tab = ev.index.min(1);
                Update::full()
            }
            Msg::ListSelect(ev) => {
                ctx.state.list_selected = ev.index;
                Update::full()
            }
            Msg::TableSelect(ev) => {
                ctx.state.table_selected = ev.index;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let joined = ctx.state.mode_tab == 1;
        let handle_mode = if joined {
            SplitterHandleMode::Border
        } else {
            SplitterHandleMode::Gutter
        };

        let mode_label = if joined { "Frame Join" } else { "Classic" };
        let editor_hint = if joined {
            "// Drag merged frame borders to resize panes."
        } else {
            "// Drag the splitter gutter to resize panes."
        };

        let items = [
            "Cargo.toml",
            "src/main.rs",
            "src/lib.rs",
            "src/widgets/splitter.rs",
            "docs/WIDGET_REPORTS.md",
            "README.md",
        ]
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();

        let list = List::new()
            .items(items)
            .selected(ctx.state.list_selected)
            .selection_symbol(Some("> "))
            .scrollbar(true)
            .on_select(ctx.link().callback(Msg::ListSelect));

        let editor = TextArea::new(format!(
            "fn main() {{\n    println!(\"Hello, Splitter!\");\n}}\n\n{}",
            editor_hint
        ))
        .line_numbers(true)
        .wrap(true)
        .border(false);

        let table = Table::new()
            .header(TableRow::new(["Key", "Value"]).style(Style::new().bold()))
            .rows([
                TableRow::new(["Language", "Rust"]),
                TableRow::new(["Target", "tui-lipan"]),
                TableRow::new(["Mode", mode_label]),
            ])
            .widths([ColumnWidth::Fixed(10), ColumnWidth::Fill(1)])
            .column_spacing(1)
            .selected(ctx.state.table_selected)
            .selection_symbol(Some("> "))
            .scrollbar(true)
            .on_select(ctx.link().callback(Msg::TableSelect));

        let right = Splitter::horizontal()
            .handle_mode(handle_mode)
            .weights(vec![0.65, 0.35])
            .child(
                Frame::new()
                    .title("Editor")
                    .join_frame(joined)
                    .padding(1)
                    .border(true)
                    .child(editor),
            )
            .child(
                Frame::new()
                    .title("Metadata")
                    .join_frame(joined)
                    .padding(1)
                    .border(true)
                    .child(table),
            );

        let root = Splitter::vertical()
            .handle_mode(handle_mode)
            .weights(vec![0.3, 0.7])
            .child(
                Frame::new()
                    .title("Files")
                    .join_frame(joined)
                    .padding(1)
                    .border(true)
                    .child(list),
            )
            .child(right);

        let tabs = Tabs::new()
            .tabs(vec![Tab::new("Classic"), Tab::new("Frame Join")])
            .active(ctx.state.mode_tab.min(1))
            .on_change(ctx.link().callback(Msg::ModeTabChanged));

        Frame::new()
            .title("Splitter Demo")
            .border(true)
            .padding(1)
            .child(VStack::new().gap(1).child(tabs).child(root))
            .into()
    }
}

fn main() -> Result<()> {
    App::new().title("Splitter Demo").mount(SplitterDemo).run()
}
