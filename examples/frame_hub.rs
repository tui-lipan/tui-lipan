use tui_lipan::prelude::*;

const SEARCH_MIN: u16 = 12;
const SEARCH_MAX: u16 = 28;

struct FrameHub {
    section: usize,
    divider_tab: usize,
}

#[derive(Default)]
struct State {
    divider_search: TextInput,
}

#[derive(Clone, Debug)]
enum Msg {
    SelectSection(usize),
    SelectDividerTab(usize),
    UpdateDividerSearch(InputEvent),
}

impl Component for FrameHub {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let link = ctx.link();
        let section_content = match self.section {
            0 => border_merge_section(),
            1 => decoration_section(),
            _ => features_section(ctx, self.divider_tab),
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new("Frame Hub").style(Style::new().bold()))
            .child(
                Tabs::new()
                    .tab("Border Merge")
                    .tab("Decorations")
                    .tab("Features")
                    .active(self.section)
                    .on_change(link.callback(|event: TabsEvent| Msg::SelectSection(event.index)))
                    .style(Style::new().fg(Color::DarkGray))
                    .active_style(Style::new().fg(Color::White).bold()),
            )
            .child(section_content)
            .child(Text::new("Press 'q' to quit").style(Style::new().fg(Color::DarkGray)))
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SelectSection(index) => {
                self.section = index;
                Update::full()
            }
            Msg::SelectDividerTab(index) => {
                self.divider_tab = index;
                Update::full()
            }
            Msg::UpdateDividerSearch(event) => {
                ctx.state
                    .divider_search
                    .set_text(event.value.as_ref().to_string());
                ctx.state.divider_search.set_cursor(event.cursor);
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.code == KeyCode::Char('q') {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }
}

fn border_merge_section() -> Element {
    ScrollView::new()
        .gap(1)
        .scrollbar(true)
        .child(Text::new("Frame join_frame demo").style(Style::new().bold()))
        .child(
            Text::new("When frames touch (gap = 0), `join_frame(true)` collapses shared seams.")
                .style(Style::new().dim()),
        )
        .child(
            HStack::new()
                .gap(1)
                .child(join_demo(false))
                .child(join_demo(true)),
        )
        .child(Text::new("border_merge_mode demo").style(Style::new().bold()))
        .child(
            Text::new(
                "Two touching frames use different border styles (plain + double). The seam cells collide; `border_merge_mode` decides the resulting glyph.",
            )
            .style(Style::new().dim()),
        )
        .child(
            Text::new("Exact and Fuzzy can look identical when an exact junction glyph exists.")
                .style(Style::new().dim()),
        )
        .child(
            HStack::new()
                .gap(1)
                .height(Length::Px(12))
                .child(merge_mode_demo(BorderMergeMode::Replace, "Replace"))
                .child(merge_mode_demo(BorderMergeMode::Exact, "Exact"))
                .child(merge_mode_demo(BorderMergeMode::Fuzzy, "Fuzzy")),
        )
        .into()
}

fn decoration_section() -> Element {
    ScrollView::new()
        .gap(1)
        .scrollbar(true)
        .child(Text::new("Frame Decoration Examples").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Border (default)")
                        .border(true)
                        .width(Length::Flex(1))
                        .height(Length::Px(6))
                        .padding(1)
                        .child(Text::new("Standard border")),
                )
                .child(
                    Frame::new()
                        .title("None")
                        .border(false)
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(6))
                        .padding(1)
                        .child(Text::new("Plain container")),
                ),
        )
        .child(Text::new("Accent Frames (Border + Accent)").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Info")
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightBlue)),
                        )
                        .border_style(BorderStyle::Rounded)
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Border + Left Accent")),
                )
                .child(
                    Frame::new()
                        .title("Warning")
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::Yellow)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Border + Left Accent")),
                )
                .child(
                    Frame::new()
                        .title("Error")
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightRed)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Border + Left Accent")),
                )
                .child(
                    Frame::new()
                        .title("Success")
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightGreen)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Border + Left Accent")),
                ),
        )
        .child(Text::new("Plain Accent Frames (No Border)").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightBlue).bg(Color::indexed(236))),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding((0, 1))
                        .child(
                            VStack::new()
                                .child(
                                    Text::new("Info")
                                        .style(Style::new().fg(Color::LightBlue).bold()),
                                )
                                .child(Text::new("Left accent bar")),
                        ),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::Yellow)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding((0, 1))
                        .child(
                            VStack::new()
                                .child(
                                    Text::new("Warning")
                                        .style(Style::new().fg(Color::Yellow).bold()),
                                )
                                .child(Text::new("Attention needed")),
                        ),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightRed)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding((0, 1))
                        .child(
                            VStack::new()
                                .child(
                                    Text::new("Error")
                                        .style(Style::new().fg(Color::LightRed).bold()),
                                )
                                .child(Text::new("Something went wrong")),
                        ),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightGreen)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding((0, 1))
                        .child(
                            VStack::new()
                                .child(
                                    Text::new("Success")
                                        .style(Style::new().fg(Color::LightGreen).bold()),
                                )
                                .child(Text::new("Operation complete")),
                        ),
                ),
        )
        .child(Text::new("Other Edge Positions (Border + Accent)").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Top")
                        .decoration(
                            EdgeDecoration::new(Edge::Top)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::Magenta)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Top accent")),
                )
                .child(
                    Frame::new()
                        .title("Bottom")
                        .decoration(
                            EdgeDecoration::new(Edge::Bottom)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::Cyan)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Bottom accent")),
                )
                .child(
                    Frame::new()
                        .title("Right")
                        .decoration(
                            EdgeDecoration::new(Edge::Right)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightYellow)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(5))
                        .padding(1)
                        .child(Text::new("Right accent")),
                ),
        )
        .child(Text::new("Half-Block Accents (No Border)").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::HalfBlock)
                                .style(Style::new().fg(Color::LightBlue)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 1))
                        .child(Text::new("Left (▌)")),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Right)
                                .glyph(DecorationGlyph::HalfBlock)
                                .style(Style::new().fg(Color::LightGreen)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 1))
                        .child(Text::new("Right (▐)")),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Top)
                                .glyph(DecorationGlyph::HalfBlock)
                                .style(Style::new().fg(Color::Yellow)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((1, 0))
                        .child(Text::new("Top (▄)")),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Bottom)
                                .glyph(DecorationGlyph::HalfBlock)
                                .style(Style::new().fg(Color::LightRed)),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 0, 0, 1))
                        .child(Text::new("Bottom (▀)")),
                ),
        )
        .child(Text::new("Custom Accent Thickness (No Border)").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightBlue))
                                .thickness(1),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 1))
                        .child(Text::new("Thickness: 1 (default)")),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightBlue))
                                .thickness(2),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 1))
                        .child(Text::new("Thickness: 2")),
                )
                .child(
                    Frame::new()
                        .border(false)
                        .decoration(
                            EdgeDecoration::new(Edge::Left)
                                .glyph(DecorationGlyph::AutoBlock)
                                .style(Style::new().fg(Color::LightBlue))
                                .thickness(3),
                        )
                        .style(Style::new().bg(Color::indexed(236)))
                        .width(Length::Flex(1))
                        .height(Length::Px(4))
                        .padding((0, 1))
                        .child(Text::new("Thickness: 3")),
                ),
        )
        .into()
}

fn features_section(ctx: &Context<FrameHub>, divider_tab: usize) -> Element {
    let link = ctx.link();
    let search_input = Element::from(
        Input::new(ctx.state.divider_search.text().to_owned())
            .cursor(ctx.state.divider_search.cursor())
            .anchor(ctx.state.divider_search.anchor())
            .placeholder("Type to filter...")
            .prefix("[")
            .suffix("]")
            .focus_prefix_style(Style::new().fg(Color::White))
            .focus_suffix_style(Style::new().fg(Color::White))
            .focus_style(Style::new().fg(Color::LightBlue))
            .placeholder_style(Style::new().fg(Color::DarkGray).dim())
            .focus_placeholder_style(Style::new().fg(Color::White))
            .truncate_head(true)
            .padding(0)
            .width(Length::Auto)
            .border(false)
            .on_change(link.callback(Msg::UpdateDividerSearch)),
    )
    .min_width(Length::Px(SEARCH_MIN))
    .max_width(Length::Px(SEARCH_MAX))
    .key("divider-search");

    ScrollView::new()
        .gap(1)
        .scrollbar(true)
        .scroll_keys(ScrollKeymap::DEFAULT)
        .child(
            Frame::new()
                .title(
                    RichText::new()
                        .span(Span::new("Rich").fg(Color::Red).bold())
                        .span(Span::new(" "))
                        .span(Span::new("Text").fg(Color::Green).bold())
                        .span(Span::new(" Title").fg(Color::Blue)),
                )
                .title_prefix(Span::new("PREFIX").fg(Color::Yellow))
                .title_suffix(Span::new("SUFFIX").fg(Color::Cyan))
                .title_alignment(Align::Center)
                .status(
                    RichText::new()
                        .span(Span::new("Status: ").fg(Color::DarkGray))
                        .span(Span::new("OK").fg(Color::Green).bold()),
                )
                .status_right(Span::new("q to quit").fg(Color::DarkGray))
                .inner_style(Style::new().bg(Color::Indexed(236)))
                .border_style(BorderStyle::Rounded)
                .height(Length::Px(8))
                .child(Text::new(
                    "This frame demonstrates:\n\
                     • Rich text title with multiple colors\n\
                     • Title prefix and suffix\n\
                     • Centered title alignment\n\
                     • Rich text status lines\n\
                     • Inner area background color",
                )),
        )
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Child Align: Start")
                        .child_align(Align::Start)
                        .height(Length::Px(5))
                        .child(Text::new("Top")),
                )
                .child(
                    Frame::new()
                        .title("Child Align: Center")
                        .child_align(Align::Center)
                        .height(Length::Px(5))
                        .child(Text::new("Middle")),
                )
                .child(
                    Frame::new()
                        .title("Child Align: End")
                        .child_align(Align::End)
                        .height(Length::Px(5))
                        .child(Text::new("Bottom")),
                ),
        )
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Left Aligned")
                        .title_alignment(Align::Start)
                        .height(Length::Px(3)),
                )
                .child(
                    Frame::new()
                        .title("Center Aligned")
                        .title_alignment(Align::Center)
                        .height(Length::Px(3)),
                )
                .child(
                    Frame::new()
                        .title("Right Aligned")
                        .title_alignment(Align::End)
                        .height(Length::Px(3)),
                ),
        )
        .child(Text::new("Dashed Border Styles").style(Style::new().bold()))
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Light Double")
                        .border_style(BorderStyle::LightDoubleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                )
                .child(
                    Frame::new()
                        .title("Heavy Double")
                        .border_style(BorderStyle::HeavyDoubleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                )
                .child(
                    Frame::new()
                        .title("Light Triple")
                        .border_style(BorderStyle::LightTripleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                ),
        )
        .child(
            HStack::new()
                .gap(1)
                .child(
                    Frame::new()
                        .title("Heavy Triple")
                        .border_style(BorderStyle::HeavyTripleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                )
                .child(
                    Frame::new()
                        .title("Light Quad")
                        .border_style(BorderStyle::LightQuadrupleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                )
                .child(
                    Frame::new()
                        .title("Heavy Quad")
                        .border_style(BorderStyle::HeavyQuadrupleDashed)
                        .height(Length::Px(4))
                        .child(Text::new("sample")),
                ),
        )
        .child(
            Frame::new()
                .title("Divider Labels")
                .border_style(BorderStyle::Rounded)
                .height(Length::Px(7))
                .child(
                    VStack::new()
                        .align(Align::Center)
                        .gap(0)
                        .padding(1)
                        .child(Text::new("Top panel"))
                        .child(
                            Divider::horizontal()
                                .join_frame(true)
                                .label(
                                    Tabs::new()
                                        .tab("Overview")
                                        .tab("Search")
                                        .tab("Logs")
                                        .active(divider_tab)
                                        .on_change(link.callback(|event: TabsEvent| {
                                            Msg::SelectDividerTab(event.index)
                                        }))
                                        .style(Style::new().fg(Color::DarkGray))
                                        .active_style(Style::new().fg(Color::White).bold())
                                        .divider('·'),
                                )
                                .label_alignment(Align::Center),
                        )
                        .child(Text::new("Bottom panel")),
                ),
        )
        .child(
            Frame::new()
                .title("Divider Search")
                .border_style(BorderStyle::Rounded)
                .height(Length::Px(8))
                .child(
                    VStack::new()
                        .align(Align::Center)
                        .gap(0)
                        .padding(1)
                        .child(Text::new("Results"))
                        .child(
                            Divider::horizontal()
                                .join_frame(true)
                                .label(
                                    HStack::new()
                                        .gap(1)
                                        .child(
                                            Text::new("Search")
                                                .style(Style::new().fg(Color::DarkGray)),
                                        )
                                        .child(search_input),
                                )
                                .label_alignment(Align::Center),
                        )
                        .child(Text::new("Click the search bar to edit the divider label")),
                ),
        )
        .child(
            Frame::new()
                .title("Vertical Divider")
                .border_style(BorderStyle::Rounded)
                .height(Length::Px(7))
                .child(
                    HStack::new()
                        .justify(Justify::Center)
                        .gap(0)
                        .padding((0, 1))
                        .height(Length::Flex(1))
                        .child(Text::new("Left panel"))
                        .child(Divider::vertical().join_frame(true))
                        .child(Text::new("Right panel")),
                ),
        )
        .into()
}

fn join_demo(join: bool) -> Element {
    let title = if join {
        "join_frame = true"
    } else {
        "join_frame = false"
    };

    Frame::new()
        .title(title)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new("Horizontal (gap = 0)").style(Style::new().dim()))
                .child(
                    HStack::new()
                        .gap(0)
                        .height(Length::Px(6))
                        .child(join_panel("Left", Color::LightBlue, join, 6))
                        .child(join_panel("Right", Color::LightGreen, join, 6)),
                )
                .child(Text::new("Vertical (gap = 0)").style(Style::new().dim()))
                .child(
                    VStack::new()
                        .gap(0)
                        .height(Length::Px(12))
                        .child(join_panel("Top", Color::Yellow, join, 6))
                        .child(join_panel("Bottom", Color::LightMagenta, join, 6)),
                ),
        )
        .into()
}

fn join_panel(label: &str, color: Color, join: bool, height: u16) -> Element {
    Frame::new()
        .title(label.to_string())
        .join_frame(join)
        .border_merge_mode(BorderMergeMode::Exact)
        .style(Style::new().fg(color))
        .height(Length::Px(height))
        .padding((0, 1))
        .child(Text::new("content"))
        .into()
}

fn merge_mode_demo(mode: BorderMergeMode, title: &str) -> Element {
    Frame::new()
        .title(title.to_string())
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Px(12))
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new("Inspect seam top/bottom").style(Style::new().dim()))
                .child(
                    HStack::new()
                        .gap(0)
                        .height(Length::Px(7))
                        .child(
                            Frame::new()
                                .title("Plain")
                                .join_frame(true)
                                .border_style(BorderStyle::Plain)
                                .border_merge_mode(mode)
                                .padding((0, 1))
                                .child(Text::new("A")),
                        )
                        .child(
                            Frame::new()
                                .title("Double")
                                .join_frame(true)
                                .border_style(BorderStyle::Double)
                                .border_merge_mode(mode)
                                .padding((0, 1))
                                .child(Text::new("B")),
                        ),
                ),
        )
        .into()
}

fn main() -> Result<()> {
    App::new()
        .focus_policy(FocusPolicy::Auto)
        .title("Frame Hub")
        .mount(FrameHub {
            section: 0,
            divider_tab: 1,
        })
        .run()
}
