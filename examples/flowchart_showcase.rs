use tui_lipan::prelude::*;

struct FlowchartShowcase;

enum Msg {
    NodeClicked(FlowchartNodeEvent),
    EdgeClicked(FlowchartEdgeEvent),
    ItemHovered(String),
}

#[derive(Default)]
struct State {
    status: String,
}

impl Component for FlowchartShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        ctx.state.status = match msg {
            Msg::NodeClicked(event) => format!("clicked node {} ({})", event.id, event.label),
            Msg::EdgeClicked(event) => format!("clicked edge {} -> {}", event.from, event.to),
            Msg::ItemHovered(label) => format!("hovered {label}"),
        };
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let node_click = ctx.link().callback(Msg::NodeClicked);
        let edge_click = ctx.link().callback(Msg::EdgeClicked);
        let node_hover = ctx
            .link()
            .callback(|event: FlowchartNodeEvent| Msg::ItemHovered(format!("node {}", event.id)));
        let edge_hover = ctx.link().callback(|event: FlowchartEdgeEvent| {
            Msg::ItemHovered(format!("edge {} -> {}", event.from, event.to))
        });

        let chart = Flowchart::new(FlowDirection::TopDown)
            .node("A", "Start", NodeShape::Stadium)
            .node("B", "Decide?", NodeShape::Diamond)
            .node("C", "Do work", NodeShape::Rect)
            .node("D", "End", NodeShape::Stadium)
            .node("E", "Retry", NodeShape::Hexagon)
            .edge(FlowchartEdge::solid("A", "B"))
            .edge(FlowchartEdge::solid("B", "C").label("yes"))
            .edge(FlowchartEdge::dashed("B", "D").label("no"))
            .edge(FlowchartEdge::thick("C", "D"))
            .edge(FlowchartEdge::solid("C", "A").label("loop"))
            .edge(FlowchartEdge::dashed("E", "C").arrow_to(EdgeArrow::Open))
            .subgraph("processing", "Processing", |b| {
                b.node("X", "Step 1", NodeShape::Subroutine)
                    .node("Y", "Step 2", NodeShape::Cylinder)
                    .edge(FlowchartEdge::solid("X", "Y"))
                    .subgraph("nested", "Nested", |b| {
                        b.node("Z", "Inner", NodeShape::Parallelogram)
                    })
            })
            .class_def("highlight", Style::new().fg(Color::Black).bg(Color::Yellow))
            .assign_class("B", "highlight")
            .node_style(Style::new().fg(Color::LightCyan))
            .edge_style(Style::new().fg(Color::Gray))
            .item_hover_style(Style::new().fg(Color::Black).bg(Color::LightGreen))
            .border(true)
            .padding(1)
            .on_node_click(node_click)
            .on_edge_click(edge_click)
            .on_node_hover(node_hover)
            .on_edge_hover(edge_hover);

        // graph TD
        //     A[Start] --> B{Decision}
        //     B -->|Yes| C[Do Something]
        //     B -->|No| D[Do Nothing]
        //     C --> E[End]
        //     D --> E
        let decision_chart = Flowchart::new(FlowDirection::TopDown)
            .node("A", "Start", NodeShape::Rect)
            .node("B", "Decision", NodeShape::Diamond)
            .node("C", "Do Something", NodeShape::Rect)
            .node("D", "Do Nothing", NodeShape::Rect)
            .node("E", "End", NodeShape::Rect)
            .edge(FlowchartEdge::solid("A", "B"))
            .edge(FlowchartEdge::solid("B", "C").label("Yes"))
            .edge(FlowchartEdge::solid("B", "D").label("No"))
            .edge(FlowchartEdge::solid("C", "E"))
            .edge(FlowchartEdge::solid("D", "E"))
            .node_style(Style::new().fg(Color::LightYellow))
            .edge_style(Style::new().fg(Color::Gray))
            .item_hover_style(Style::new().fg(Color::Black).bg(Color::LightGreen))
            .border(true)
            .padding(1);

        VStack::new()
            .gap(1)
            .child(
                HStack::new()
                    .gap(1)
                    .child(
                        Frame::new()
                            .title("Flowchart showcase")
                            .border(true)
                            .padding(1)
                            .child(chart),
                    )
                    .child(
                        Frame::new()
                            .title("Decision flowchart")
                            .border(true)
                            .padding(1)
                            .child(decision_chart),
                    ),
            )
            .child(Text::new(format!("Status: {}", ctx.state.status)))
            .into()
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new()
        .title("tui-lipan - Flowchart Showcase")
        .mount(FlowchartShowcase)
        .run()
}
