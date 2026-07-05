use tui_lipan::prelude::*;

struct GraphShowcase;

enum Msg {
    NodeClicked(GraphNodeEvent),
    NodeHovered(GraphNodeEvent),
}

#[derive(Default)]
struct State {
    last_clicked: Option<GraphNodeEvent>,
    last_hovered: Option<GraphNodeEvent>,
}

impl Component for GraphShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tree = sample_tree();
        let click = ctx.link().callback(Msg::NodeClicked);
        let hover = ctx.link().callback(Msg::NodeHovered);
        let hover_style = Style::new().fg(Color::Black).bg(Color::LightCyan);

        let top_down = Graph::new()
            .root(tree.clone())
            .direction(GraphDirection::TopDown)
            .gap_x(3)
            .gap_y(1)
            .node_border(true)
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let left_right = Graph::new()
            .root(tree.clone())
            .direction(GraphDirection::LeftRight)
            .gap_x(4)
            .gap_y(1)
            .node_border(true)
            .edge_style(Style::new().fg(Color::LightGreen))
            .node_style(Style::new().fg(Color::LightYellow))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let compact_left_right = Graph::new()
            .root(tree.clone())
            .direction(GraphDirection::LeftRight)
            .gap_x(4)
            .gap_y(1)
            .node_border(false)
            .edge_style(Style::new().fg(Color::LightGreen))
            .node_style(Style::new().fg(Color::LightYellow))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(Style::new().lighten_by(0.0));

        let compact = Graph::new()
            .root(tree.clone())
            .direction(GraphDirection::TopDown)
            .gap_x(2)
            .gap_y(1)
            .node_border(false)
            .edge_style(Style::new().fg(Color::Gray))
            .node_style(Style::new().fg(Color::White))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let rounded_nodes = Graph::new()
            .root(tree.clone())
            .gap_x(3)
            .gap_y(1)
            .node_border_style(BorderStyle::Rounded)
            .edge_style(Style::new().fg(Color::Gray))
            .node_style(Style::new().fg(Color::LightBlue))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let rounded_edges = Graph::new()
            .root(tree.clone())
            .gap_x(3)
            .gap_y(1)
            .edge_border_style(BorderStyle::Rounded)
            .edge_style(Style::new().fg(Color::LightCyan))
            .node_style(Style::new().fg(Color::White))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let wrapped_labels = Graph::new()
            .root(wrapped_label_tree())
            .direction(GraphDirection::TopDown)
            .gap_x(3)
            .gap_y(1)
            .max_node_width(12)
            .node_border_style(BorderStyle::Rounded)
            .edge_border_style(BorderStyle::Rounded)
            .edge_style(Style::new().fg(Color::LightBlue))
            .node_style(Style::new().fg(Color::LightYellow))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click.clone())
            .on_node_hover(hover.clone())
            .node_hover_style(hover_style);

        let rounded_both = Graph::new()
            .root(tree)
            .gap_x(3)
            .gap_y(1)
            .node_border_style(BorderStyle::Rounded)
            .edge_border_style(BorderStyle::Rounded)
            .edge_style(Style::new().fg(Color::LightMagenta))
            .node_style(Style::new().fg(Color::LightYellow))
            .border(true)
            .padding(1)
            .width(Length::Auto)
            .height(Length::Auto)
            .on_node_click(click)
            .on_node_hover(hover)
            .node_hover_style(hover_style);

        let clicked = describe_event("clicked", ctx.state.last_clicked.as_ref());
        let hovered = describe_event("hovered", ctx.state.last_hovered.as_ref());

        let showcase = VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("Theme-driven top-down tree")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(top_down),
            )
            .child(
                Frame::new()
                    .title("Left-right tree")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(left_right),
            )
            .child(
                Frame::new()
                    .title("Compact left-right tree")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(compact_left_right),
            )
            .child(
                Frame::new()
                    .title("Compact nodes")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(compact),
            )
            .child(
                Frame::new()
                    .title("Rounded nodes only")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(rounded_nodes),
            )
            .child(
                Frame::new()
                    .title("Rounded edges only")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(rounded_edges),
            )
            .child(
                Frame::new()
                    .title("Wrapped multi-line labels")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(wrapped_labels),
            )
            .child(
                Frame::new()
                    .title("Rounded nodes and edges")
                    .border(true)
                    .height(Length::Auto)
                    .padding(1)
                    .child(rounded_both),
            );

        VStack::new()
            .child(
                ScrollView::new()
                    .scrollbar(true)
                    .scroll_keys(ScrollKeymap::DEFAULT)
                    .show_scroll_indicators(false)
                    .padding(1)
                    .child(showcase),
            )
            .child(
                Frame::new()
                    .title("Pointer status")
                    .border(true)
                    .height(Length::Auto)
                    .padding((0, 1))
                    .child(Text::new(format!("{clicked} | {hovered}"))),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::NodeClicked(event) => ctx.state.last_clicked = Some(event),
            Msg::NodeHovered(event) => ctx.state.last_hovered = Some(event),
        }
        Update::full()
    }
}

fn describe_event(label: &str, event: Option<&GraphNodeEvent>) -> String {
    match event {
        Some(event) => format!("{label}: {} @ {:?}", event.label, event.path.segments()),
        None => format!("{label}: none"),
    }
}

fn sample_tree() -> GraphNode {
    GraphNode::new("App")
        .style(Style::new().fg(Color::LightMagenta))
        .child(
            GraphNode::new("UI")
                .child(GraphNode::new("Header"))
                .child(GraphNode::new("Panels"))
                .child(GraphNode::new("Status")),
        )
        .child(
            GraphNode::new("Data")
                .child(GraphNode::new("Cache"))
                .child(GraphNode::new("API").style(Style::new().fg(Color::LightGreen))),
        )
        .child(GraphNode::new("Tasks").child(GraphNode::new("Worker")))
}

fn wrapped_label_tree() -> GraphNode {
    GraphNode::new("Application Runtime")
        .style(Style::new().fg(Color::LightMagenta))
        .child(
            GraphNode::new("Terminal renderer with cached cells")
                .child(GraphNode::new("Diff buffer"))
                .child(GraphNode::new("Paint\ncommands")),
        )
        .child(
            GraphNode::new("Input routing")
                .child(GraphNode::new("Mouse hit testing"))
                .child(GraphNode::new("Keyboard focus")),
        )
        .child(GraphNode::new("Component update loop"))
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Graph Showcase")
        .mount(GraphShowcase)
        .run()
}
