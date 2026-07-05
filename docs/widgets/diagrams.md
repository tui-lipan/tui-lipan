# Diagram Widgets

Structural and Mermaid-style diagram widgets. All are direct-paint, read-only
display primitives that take builder-owned data (`Arc<str>` labels, plain Rust
enums, `Style`, `BorderStyle`, `Padding`, and `Length`), so external syntaxes
such as Mermaid can be lowered into widget data without backend types.

To render Mermaid fenced code blocks inside rich text instead of building these
widgets directly, see [`DocumentView`](display.md#documentview), which renders a
supported Mermaid subset (flowchart, sequence, class, state, ER, pie, gantt).

| Widget | Description |
|--------|-------------|
| `Graph` | Node-edge structural visualization with clickable, focusable tree nodes |
| `Flowchart` | Directed flowcharts with shapes, edge labels, subgraphs, classes, and item callbacks |
| `SequenceDiagram` | Participant/message timelines with fragments, notes, and item callbacks |
| `ClassDiagram` | UML class diagrams with compartments and relation glyphs |
| `StateDiagram` | UML state diagrams with transitions and pseudo-state glyphs |
| `ErDiagram` | Entity-relationship diagrams with crow's-foot cardinality glyphs |
| `GanttDiagram` | Timeline schedules with sections, task bars, dependencies, and milestones |

---

## Graph

Node-edge structural visualization for trees, with boxed labels and orthogonal box-drawing edges.

| Prop | Type | Description |
|------|------|-------------|
| `root` | `GraphNode` | Root tree node to render |
| `direction` | `GraphDirection` | `TopDown` (default) or `LeftRight` |
| `layout` | `GraphLayout` | `Tree` tidy layered layout |
| `gap_x` | `u16` | Horizontal spacing between nodes / layers |
| `gap_y` | `u16` | Vertical spacing between nodes / layers |
| `max_node_width` | `u16` | Maximum node label width before wrapping; explicit newlines are also honored |
| `node_padding` | `impl Into<Padding>` | Padding inside each node label box |
| `node_border` | `bool` | Draw borders around nodes by default |
| `node_border_style` | `BorderStyle` | Node box border appearance (`Plain` or `Rounded` are most useful) |
| `style` | `Style` | Base graph style |
| `node_style` | `Style` | Default node style |
| `node_hover_style` | `Style` | Style patched onto the hovered node |
| `node_focus_style` | `Style` | Style patched onto the internally focused node |
| `edge_style` | `Style` | Edge line style |
| `edge_border_style` | `BorderStyle` | Edge elbow glyph style (`Rounded` uses `╭╮╰╯`) |
| `focusable` | `bool` | Participate in focus traversal as one widget with internal roving node focus |
| `focused_path` | `GraphNodePath` | Controlled internally focused node path |
| `on_node_click` | `Callback<GraphNodeEvent>` | Fired when a node box is clicked |
| `on_node_hover` | `Callback<GraphNodeEvent>` | Fired when hover moves to a different node |
| `on_node_focus` | `Callback<GraphNodeEvent>` | Fired when keyboard or pointer interaction moves internal node focus |
| `on_node_activate` | `Callback<GraphNodeEvent>` | Fired when the focused node is activated with Enter or Space |
| `padding` | `impl Into<Padding>` | Inner padding around the graph content |
| `border` | `bool` | Draw outer graph border |
| `border_style` | `BorderStyle` | Outer border appearance |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

`GraphNode` supports `.child(...)`, `.children(...)`, `.style(...)`,
`.hover_style(...)`, `.focus_style(...)`, `.extend_focus_style(...)`,
`.inherit_focus_style(...)`, and per-node `.border(...)` overrides. Labels wrap
at `Graph::max_node_width(24)` by default and can also include explicit `\n`
line breaks. The current layout is tree-first; the API includes `GraphLayout`
so future DAG-style layouts can be added without reshaping builders.

`Graph` is focusable as a single tab stop when `.focusable(true)`,
`.on_node_focus(...)`, or `.on_node_activate(...)` is set. Static graphs remain
unfocusable by default, and pointer-only callbacks such as `.on_node_click(...)`
or `.on_node_hover(...)` do not opt the graph into keyboard focus. Once focused,
the graph keeps an internal roving node focus, styled by `.node_focus_style(...)`
and optionally controlled with `.focused_path(...)`. Per-node focus styles patch
on top of the graph-wide focused-node style. As elsewhere in the framework,
hover color transforms remain transient and compose over focused node colors.

Keyboard navigation follows the rendered tree direction:

| Direction | Parent / first child | Siblings |
|-----------|----------------------|----------|
| `TopDown` | `Up` / `Down` | `Left` / `Right` |
| `LeftRight` | `Left` / `Right` | `Up` / `Down` |

`Enter` and `Space` emit `on_node_activate` for the focused node. `Home` and
`End` move to the first and last rendered node. When a keyboard-focused `Graph`
lives inside a `PanView`, navigation auto-pans the nearest ancestor `PanView` to
keep the newly focused graph node visible.

```rust
let tree = GraphNode::new("A")
    .child(
        GraphNode::new("B")
            .children([GraphNode::new("D"), GraphNode::new("E")]),
    )
    .child(GraphNode::new("C"));

Graph::new()
    .root(tree)
    .direction(GraphDirection::TopDown)
    .gap_x(2)
    .gap_y(1)
    .max_node_width(16)
    .node_border(true)
    .node_border_style(BorderStyle::Rounded)
    .node_hover_style(Style::new().fg(Color::Black).bg(Color::Cyan))
    .node_focus_style(Style::new().fg(Color::Black).bg(Color::Yellow))
    .on_node_click(ctx.link().callback(Msg::NodeClicked))
    .on_node_hover(ctx.link().callback(Msg::NodeHovered))
    .on_node_focus(ctx.link().callback(Msg::NodeFocused))
    .on_node_activate(ctx.link().callback(Msg::NodeActivated))
    .edge_style(Style::new().fg(Color::Blue))
    .edge_border_style(BorderStyle::Rounded)
    .border(true)
    .padding(1)
```

---

## Flowchart

`Flowchart` renders Mermaid-style directed flowcharts as a direct-paint widget.
It supports shaped nodes, solid/dashed/thick/invisible edges, labels, arrowhead
variants, subgraph boxes, class-style assignments, bundled themes, and item
click/hover callbacks.

| Prop | Type | Description |
|------|------|-------------|
| `new` | `FlowDirection` | **Constructor** - `TopDown`, `BottomUp`, `LeftRight`, or `RightLeft` |
| `node` | `(id, label, NodeShape)` | Add or replace a node |
| `styled_node` | `(id, label, NodeShape, Style)` | Add a node with a style override |
| `node_hover_style` | `(id, Style)` | Set a per-node hover style override; also available on subgraph builders |
| `edge` | `FlowchartEdge` (`Edge`) | Add a directed edge |
| `subgraph` | `(id, label, builder)` | Add a grouped subgraph; builders can nest subgraphs |
| `class_def` | `(name, Style)` | Define a named class style |
| `assign_class` | `(id, class)` | Apply a class to a node or subgraph |
| `theme` | `FlowchartTheme` | `classic()`, `minimal()`, or `ascii()` glyph/style presets |
| `node_style` | `Style` | Default node style |
| `edge_style` | `Style` | Default edge style |
| `subgraph_style` | `Style` | Default subgraph style |
| `label_style` | `Style` | Default edge label style |
| `item_hover_style` | `Style` | Style patched onto the hovered item |
| `node_gap` | `u16` | Gap between nodes within a layer |
| `layer_gap` | `u16` | Gap between Sugiyama layers |
| `subgraph_padding` | `impl Into<Padding>` | Padding around subgraph contents |
| `max_node_width` | `u16` | Wrap node labels at this cell width |
| `border` / `border_style` | `bool` / `BorderStyle` | Outer flowchart border |
| `padding` | `impl Into<Padding>` | Padding inside the outer border |
| `width` / `height` | `Length` | Requested widget size |
| `on_node_click` / `on_node_hover` | `Callback<FlowchartNodeEvent>` | Node interactions |
| `on_edge_click` / `on_edge_hover` | `Callback<FlowchartEdgeEvent>` | Edge interactions |
| `on_subgraph_click` / `on_subgraph_hover` | `Callback<FlowchartSubgraphEvent>` | Subgraph header interactions |

```rust
Flowchart::new(FlowDirection::TopDown)
    .node("A", "Start", NodeShape::Stadium)
    .node("B", "Decide?", NodeShape::Diamond)
    .node("C", "Do work", NodeShape::Rect)
    .node("D", "End", NodeShape::Stadium)
    .edge(FlowchartEdge::solid("A", "B"))
    .edge(FlowchartEdge::solid("B", "C").label("yes"))
    .edge(FlowchartEdge::dashed("B", "D").label("no"))
    .edge(FlowchartEdge::thick("C", "D"))
    .subgraph("processing", "Processing", |b| {
        b.node("X", "Step 1", NodeShape::Rect)
            .node("Y", "Step 2", NodeShape::Rect)
            .edge(FlowchartEdge::solid("X", "Y"))
    })
    .class_def("highlight", Style::new().fg(Color::Yellow))
    .assign_class("B", "highlight")
    .node_hover_style("B", Style::new().bg(Color::Blue))
    .on_node_click(ctx.link().callback(Msg::NodeClicked))
```

`FlowchartEdge` is the prelude alias for the public `Edge` type to avoid a name
collision with the styling `Edge` type already exported by the prelude. Outside
the prelude, use `tui_lipan::widgets::Edge` directly.

---

## SequenceDiagram

Mermaid-style UML sequence diagrams with participants, messages, activations,
notes, fragments, autonumbering, and per-item pointer callbacks.

| Prop | Type | Description |
|------|------|-------------|
| `participant` | `impl Into<Arc<str>>` | Add a participant using the same alias and display label |
| `participant_aliased` | `(alias, label)` | Add a participant whose message key differs from its rendered label |
| `actor_kind` | `(actor, ActorKind)` | Render an actor as `Participant` (box) or `Actor` (stick figure) |
| `message` | `SequenceMessage` | Append a message step (`sync`, `async_`, `reply`, etc.) |
| `step` | `SequenceStep` / `Step` | Append any step, including messages, notes, activations, and fragments |
| `variant` | `SequenceDiagramVariant` | Select boxed Mermaid-style participants or compact minimal rendering |
| `theme` | `SequenceDiagramTheme` | Apply a bundled style/glyph theme; presets include `classic()`, `minimal()`, and `ascii()` |
| `minimal` | `()` | Shortcut for `.variant(SequenceDiagramVariant::Minimal).theme(SequenceDiagramTheme::minimal())` |
| `boxed` | `()` | Shortcut for `.variant(SequenceDiagramVariant::Boxed).theme(SequenceDiagramTheme::classic())` |
| `actor_glyph` | `impl Into<Arc<str>>` | Override the glyph used for `ActorKind::Actor` in minimal mode; defaults to `"○ "` |
| `autonumber` | `bool` | Show numbered chips beside message steps |
| `autonumber_format` | `impl Into<Arc<str>>` | Format autonumber chips using `{n}` as the message number placeholder |
| `max_label_cells` | `Option<u16>` | Preferred message-label reservation and note wrap width; defaults to `Some(32)` |
| `message_label_overflow` | `Overflow` | Arrow/self-message label overflow policy; defaults to `Ellipsis`, use `Wrap` for multi-line labels |
| `message_kind_style` | `(MessageStyle, Style)` | Style all messages of one arrow kind before per-message overrides |
| `fragment_kind_style` | `(FragmentKind, Style)` | Style all fragments of one kind before per-fragment overrides |
| `lifeline_glyph` | `char` | Override the theme lifeline glyph |
| `activation_glyph` | `char` | Override the theme activation fill glyph |
| `item_hover_style` | `Style` | Style patched onto the hovered item |
| `on_item_click` | `Callback<SequenceItemEvent>` | Fired when a participant, message, note, fragment label, or self-message is clicked |
| `on_item_hover` | `Callback<SequenceItemEvent>` | Fired when hover moves to a different item |
| `style` | `Style` | Base diagram style |
| `participant_style` | `Style` | Default participant box/actor style |
| `lifeline_style` | `Style` | Vertical lifeline style |
| `message_label_style` | `Style` | Default message label style |
| `note_style` | `Style` | Default note box style |
| `fragment_style` | `Style` | Default fragment box/background style |
| `activation_style` | `Style` | Activation bar style |
| `autonumber_style` | `Style` | Autonumber chip style |
| `padding` | `impl Into<Padding>` | Inner padding around diagram content |
| `border` | `bool` | Draw outer diagram border |
| `border_style` | `BorderStyle` | Outer border appearance |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

`SequenceMessage` constructors cover the common Mermaid arrow styles:
`SequenceMessage::sync(...)`, `SequenceMessage::async_(...)`, and `SequenceMessage::reply(...)`.
Use `SequenceMessage::line_style(...)` for per-arrow line styling. Message labels use
`Overflow::Ellipsis` by default: `max_label_cells` reserves a compact preferred width,
but the final label can flex wider when the current arrow span is wider, and the ellipsis
is calculated for that final width. Set `.message_label_overflow(Overflow::Wrap)` to wrap
arrow labels over multiple visual rows instead of truncating. Notes continue to wrap to
multiple rows at `max_label_cells`.
Use `Step`/`SequenceStep` for non-message rows such as `Step::self_msg(...)`,
`Step::note(...)`, `Step::note_over(...)`, `Step::activate(...)`,
`Step::deactivate(...)`, `Step::fragment_begin(...)`,
`Step::fragment_branch(...)`, and `Step::fragment_end()`.

`SequenceDiagramTheme` groups every diagram-local style, border, and glyph knob
so a single value can switch between visual systems.
`SequenceDiagramTheme::classic()` is the default,
`SequenceDiagramTheme::minimal()` pairs with the compact variant, and
`SequenceDiagramTheme::ascii()` avoids box-drawing glyphs for terminals or logs
that need plain ASCII. The existing flat setters such as
`.participant_style(...)`, `.lifeline_style(...)`, `.message_label_style(...)`,
`.note_style(...)`, `.fragment_style(...)`, `.activation_style(...)`,
`.autonumber_style(...)`, and `.item_hover_style(...)` remain supported as
shortcuts that mutate the corresponding theme slot.

```rust
SequenceDiagram::new()
    .participant("Client")
    .participant("Service")
    .participant_aliased("DB", "Database")
    .actor_kind("Client", ActorKind::Actor)
    .message(SequenceMessage::sync("Client", "Service", "GET /items").activate_target(true))
    .message(SequenceMessage::async_("Service", "DB", "query"))
    .message(SequenceMessage::reply("DB", "Service", "rows"))
    .step(Step::note_over(["Client", "Service"], "cached response allowed"))
    .step(Step::fragment_begin(FragmentKind::Alt, "cache hit"))
    .message(SequenceMessage::reply("Service", "Client", "200 OK"))
    .step(Step::fragment_branch(FragmentKind::Alt, "else miss"))
    .message(SequenceMessage::sync("Service", "DB", "refresh"))
    .step(Step::fragment_end())
    .autonumber(true)
    .item_hover_style(Style::new().fg(Color::Black).bg(Color::LightCyan))
    .on_item_click(ctx.link().callback(Msg::SequenceClicked))
    .on_item_hover(ctx.link().callback(Msg::SequenceHovered))
    .border(true)
    .padding(1)
```

Wrap long arrow labels when vertical growth is preferable to truncation:

```rust
SequenceDiagram::new()
    .participant("Gateway")
    .participant("Policy Engine")
    .message(SequenceMessage::sync(
        "Gateway",
        "Policy Engine",
        "evaluate risk score with long rule explanation",
    ))
    .max_label_cells(Some(24))
    .message_label_overflow(Overflow::Wrap)
```

Switch to the ASCII preset when box-drawing support is unavailable:

```rust
SequenceDiagram::new()
    .participant("Client")
    .participant("API")
    .message(SequenceMessage::sync("Client", "API", "request"))
    .message(SequenceMessage::reply("API", "Client", "response"))
    .theme(SequenceDiagramTheme::ascii())
```

The ASCII preset covers diagram-local borders and glyphs. The optional outer
widget border configured by `.border(true)` / `.border_style(...)` is separate;
leave it off or choose a custom ASCII `BorderStyle` when plain output is
required.

Use granular setters when one semantic kind should share a style across the
whole diagram. Per-message `SequenceMessage::line_style(...)` and
`SequenceMessage::label_style(...)` still win for one-off highlights.

```rust
SequenceDiagram::new()
    .participant("Client")
    .participant("API")
    .message(SequenceMessage::sync("Client", "API", "start"))
    .message(SequenceMessage::lost("API", "Client", "timeout"))
    .message_kind_style(MessageStyle::Lost, Style::new().fg(Color::Red).bold())
```

Use minimal mode when horizontal space is tight or you want a lighter-weight
diagram presentation. It keeps message arrows, notes, and interaction targets,
but renders participants without boxes; actor participants use `"○ "` by
default and can be customized with `actor_glyph`. Prefer glyphs that occupy a
predictable number of terminal cells; Nerd Font symbols are opt-in because cell
width support depends on the user's terminal and font.

```rust
SequenceDiagram::new()
    .participant("Client")
    .participant("API")
    .actor_kind("Client", ActorKind::Actor)
    .message(SequenceMessage::sync("Client", "API", "ping"))
    .message(SequenceMessage::reply("API", "Client", "pong"))
    .minimal()
    .actor_glyph("󰋦 ")
```

For long diagrams, wrap `SequenceDiagram` in a `ScrollView`; the widget itself is
static content and does not keep internal scroll state.

---

## ClassDiagram, StateDiagram, and ErDiagram

Static diagram primitives for UML class diagrams, UML state diagrams, and
entity-relationship diagrams. They use builder-owned data (`Arc<str>` labels,
plain Rust enums, `Style`, `BorderStyle`, `Padding`, and `Length`) so parsers can
lower external syntaxes such as Mermaid into widget data without backend types.

```rust
ClassDiagram::new()
    .class("Animal")
    .attribute("Animal", ClassVisibility::Protected, "name", "String")
    .method("Animal", ClassVisibility::Public, "speak", "()")
    .class("Dog")
    .relation(
        "Animal",
        "Dog",
        ClassRelationKind::Inheritance,
        None::<std::sync::Arc<str>>,
        None::<std::sync::Arc<str>>,
        None::<std::sync::Arc<str>>,
    )
```

`ClassDiagram::max_node_width(width)`, `StateDiagram::max_node_width(width)`,
and `ErDiagram::max_node_width(width)` wrap long node rows before measuring each
node; the default is 32 terminal cells.

```rust
StateDiagram::new()
    .max_node_width(20)
    .start_to("Idle")
    .state("Idle")
    .state("Processing")
    .transition("Idle", "Processing", Some("submit".into()))
    .end_from("Processing")
```

```rust
ErDiagram::new()
    .max_node_width(24)
    .entities([
        ErEntity::new("CUSTOMER").attribute(ErAttribute::new("int", "id").pk()),
        ErEntity::new("ORDER").attribute(ErAttribute::new("int", "customer_id").fk()),
    ])
    .relation(
        "CUSTOMER",
        "ORDER",
        ErCardinality::ExactlyOne,
        ErCardinality::ZeroOrMore,
        Some("places".into()),
    )
```

These initial widgets are static display primitives; they do not emit pointer
callbacks yet.

---

## GanttDiagram

Static Mermaid-style timeline schedules with sections, task bars, dependencies,
milestones, and day-based date arithmetic.

| Prop | Type | Description |
|------|------|-------------|
| `title` | `impl Into<Arc<str>>` | Optional schedule title |
| `section` | `GanttSection` | Append a section with one or more tasks |
| `spec` | `GanttSpec` | Replace the full schedule model |
| `theme` | `GanttDiagramTheme` | Apply semantic styles for axis, labels, bars, and milestones |
| `style` | `Style` | Base diagram style |
| `title_style` | `Style` | Override title text style |
| `axis_style` | `Style` | Override date-range and tick-axis style |
| `section_style` | `Style` | Override section header style |
| `task_style` | `Style` | Override task labels and bars |
| `max_timeline_width` | `u16` | Compress long schedules to this many timeline cells; default is `80` |
| `padding` | `impl Into<Padding>` | Inner padding around the rendered rows |
| `width` / `height` | `Length` | Widget dimensions |

```rust
GanttDiagram::new()
    .title("Sample Schedule")
    .section(
        GanttSection::new("Build")
            .task(
                GanttTask::new("Design")
                    .id("a1")
                    .start_date("2026-05-01")
                    .duration_days(3)
                    .done(),
            )
            .task(
                GanttTask::new("Implement")
                    .id("a2")
                    .after("a1")
                    .duration_days(4)
                    .active(),
            )
            .task(GanttTask::new("Release").after("a2").milestone()),
    )
    .max_timeline_width(40)
    .padding(1)
```

`GanttTask::after(id)` starts a task at the exclusive end of the referenced task.
Date parsing currently supports `YYYY-MM-DD`; `GanttDate::parse_ymd(...)` is
available when you need fallible parsing before constructing a task.
