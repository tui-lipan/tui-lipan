use super::*;

fn joined(rows: StyledDiagramRows) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain_rows(rows: StyledDiagramRows) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn span_containing<'a>(rows: &'a StyledDiagramRows, needle: &str) -> &'a Span {
    rows.iter()
        .flatten()
        .find(|span| span.content.contains(needle))
        .unwrap_or_else(|| panic!("expected span containing {needle:?}"))
}

fn gantt_bar_span_for_label<'a>(rows: &'a StyledDiagramRows, label: &str) -> &'a Span {
    rows.iter()
        .find(|row| row.iter().any(|span| span.content.contains(label)))
        .and_then(|row| {
            row.iter()
                .find(|span| span.content.contains('█') || span.content.contains('◆'))
        })
        .unwrap_or_else(|| panic!("expected gantt bar for {label:?}"))
}

fn flow_node(id: &str, label: &str, shape: FlowNodeShape) -> FlowNodeSpec {
    FlowNodeSpec {
        id: id.into(),
        label: label.into(),
        shape,
        style: NodeStyle::default(),
    }
}

fn default_styles() -> DocumentStyles {
    DocumentStyles::default()
}

#[test]
fn rasterize_gantt_contains_labels_bars_and_milestone() {
    use crate::widgets::gantt_diagram::{GanttSection, GanttTask};

    let spec = GanttSpec::new().title("Release").section(
        GanttSection::new("Build")
            .task(
                GanttTask::new("Design")
                    .id("design")
                    .start_date("2026-01-01")
                    .duration_days(2)
                    .done(),
            )
            .task(GanttTask::new("Ship").after("design").milestone()),
    );

    let rows = joined(rasterize_gantt(&spec, &default_styles()));
    assert!(rows.contains("Release"));
    assert!(rows.contains("Build"));
    assert!(rows.contains("Design"));
    assert!(rows.contains('█'));
    assert!(rows.contains('◆'));
}

#[test]
fn rasterize_gantt_uses_distinct_theme_derived_status_shades() {
    use crate::widgets::gantt_diagram::{GanttSection, GanttTask};

    let styles = DocumentStyles {
        diagram_node_fill_style: Style::new().bg(Color::Rgb(10, 12, 18)),
        diagram_node_border_style: Style::new().fg(Color::Rgb(99, 102, 241)),
        diagram_node_label_style: Style::new().fg(Color::Rgb(226, 232, 240)),
        diagram_edge_style: Style::new().fg(Color::Rgb(125, 211, 252)),
        diagram_muted_style: Style::new().fg(Color::Rgb(100, 116, 139)),
        ..DocumentStyles::default()
    };
    let spec = GanttSpec::new().section(
        GanttSection::new("Status")
            .task(
                GanttTask::new("Pending")
                    .id("pending")
                    .start_date("2026-01-01")
                    .duration_days(1),
            )
            .task(
                GanttTask::new("Active")
                    .id("active")
                    .after("pending")
                    .duration_days(1)
                    .active(),
            )
            .task(
                GanttTask::new("Done")
                    .id("done")
                    .after("active")
                    .duration_days(1)
                    .done(),
            )
            .task(
                GanttTask::new("Critical")
                    .id("critical")
                    .after("done")
                    .duration_days(1)
                    .critical(),
            )
            .task(GanttTask::new("Milestone").after("critical").milestone()),
    );

    let rows = rasterize_gantt(&spec, &styles);
    let rendered = joined(rows.clone());
    let pending = gantt_bar_span_for_label(&rows, "Pending").style;
    let active = gantt_bar_span_for_label(&rows, "Active").style;
    let done = gantt_bar_span_for_label(&rows, "Done").style;
    let critical = gantt_bar_span_for_label(&rows, "Critical").style;
    let milestone = gantt_bar_span_for_label(&rows, "Milestone").style;

    assert!(!rendered.contains('░'));
    assert!(!rendered.contains('▓'));
    assert!(!rendered.contains('▒'));
    assert_ne!(pending.fg, active.fg);
    assert_ne!(active.fg, done.fg);
    assert_ne!(done.fg, critical.fg);
    assert_ne!(critical.fg, milestone.fg);
    assert_ne!(pending.fg, styles.diagram_muted_style.fg);
    assert_ne!(pending.fg, styles.diagram_edge_style.fg);
    assert_ne!(done.fg, styles.diagram_edge_style.fg);
    assert_ne!(pending.fg, styles.diagram_node_label_style.fg);
    assert_eq!(active.fg, styles.diagram_node_border_style.fg);
    assert_eq!(pending.fg, Some(Color::Rgb(99, 107, 202).into()));
    assert_ne!(done.fg, Some(Color::LightGreen.into()));
    assert_ne!(critical.fg, Some(Color::LightRed.into()));
    assert_ne!(milestone.fg, Some(Color::LightMagenta.into()));
    assert_eq!(critical.bold, Some(true));
    assert_eq!(milestone.bold, Some(true));
    assert_eq!(pending.bg, None);
    assert_eq!(active.bg, None);
    assert_eq!(done.bg, None);
    assert_eq!(critical.bg, None);
    assert_eq!(milestone.bg, None);
}

#[test]
fn decision_flowchart_attaches_branch_arrows_to_child_tops() {
    // Regression: the `Yes`/`No` branches of a TopDown diamond used to
    // enter the sibling boxes from the side (rendering as `◀`/`▶`)
    // because port selection was purely geometric.
    let rows = joined(rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![
                flow_node("A", "Start", FlowNodeShape::Rect),
                flow_node("B", "Decision", FlowNodeShape::Diamond),
                flow_node("C", "Do Something", FlowNodeShape::Rect),
                flow_node("D", "Do Nothing", FlowNodeShape::Rect),
                flow_node("E", "End", FlowNodeShape::Rect),
            ],
            edges: vec![
                FlowEdgeSpec {
                    from: "A".into(),
                    to: "B".into(),
                    label: None,
                    dashed: false,
                },
                FlowEdgeSpec {
                    from: "B".into(),
                    to: "C".into(),
                    label: Some("Yes".into()),
                    dashed: false,
                },
                FlowEdgeSpec {
                    from: "B".into(),
                    to: "D".into(),
                    label: Some("No".into()),
                    dashed: false,
                },
                FlowEdgeSpec {
                    from: "C".into(),
                    to: "E".into(),
                    label: None,
                    dashed: false,
                },
                FlowEdgeSpec {
                    from: "D".into(),
                    to: "E".into(),
                    label: None,
                    dashed: false,
                },
            ],
        },
        &default_styles(),
    ));
    assert!(
        !rows.contains('◀') && !rows.contains('▶'),
        "branch arrows in a TopDown chart should be ▼, not ◀/▶:\n{rows}",
    );
    assert!(rows.contains('▼'));
    let row_lines = rows.lines().collect::<Vec<_>>();
    let yes_row = row_lines
        .iter()
        .position(|row| row.contains("Yes"))
        .expect("Yes label row");
    let no_row = row_lines
        .iter()
        .position(|row| row.contains("No"))
        .expect("No label row");
    // Sibling labels share a coalesced trunk row in the new fan-out
    // routing — but the row must not also carry the ▼ arrowheads,
    // otherwise the labels would be sitting on the arrow row.
    assert!(
        !row_lines[yes_row].contains('▼'),
        "Yes label row must not carry arrow heads:\n{rows}",
    );
    assert!(
        !row_lines[no_row].contains('▼'),
        "No label row must not carry arrow heads:\n{rows}",
    );
}

#[test]
fn flowchart_renders_boxes_and_connectors_not_source_summary() {
    let rows = joined(rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![
                flow_node("A", "Start", FlowNodeShape::Rect),
                flow_node("B", "End", FlowNodeShape::Round),
            ],
            edges: vec![FlowEdgeSpec {
                from: "A".into(),
                to: "B".into(),
                label: Some("go".into()),
                dashed: false,
            }],
        },
        &default_styles(),
    ));
    assert!(rows.contains('┌') || rows.contains('╭'));
    assert!(rows.chars().any(|c| matches!(c, '▶' | '◀' | '▼' | '▲')));
    assert!(rows.contains("go"));
    assert!(!rows.contains("flowchart"));
    assert!(!rows.contains("nodes:"));
}

#[test]
fn flowchart_cylinder_renders_database_cue() {
    let rows = joined(rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![flow_node("I", "Database", FlowNodeShape::Cylinder)],
            edges: Vec::new(),
        },
        &default_styles(),
    ));

    assert!(
        rows.contains("( Database") && rows.contains(')'),
        "cylinder node should render curved side cues:\n{rows}",
    );
}

#[test]
fn flowchart_node_with_fill_emits_bg_styled_spans() {
    let fill = Color::Rgb(0x4c, 0xaf, 0x50);
    let rows = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![FlowNodeSpec {
                id: "A".into(),
                label: "Start".into(),
                shape: FlowNodeShape::Rect,
                style: NodeStyle {
                    fill: Some(fill),
                    ..NodeStyle::default()
                },
            }],
            edges: Vec::new(),
        },
        &default_styles(),
    );

    assert!(
        rows.iter()
            .flatten()
            .any(|span| span.content.contains("Start") && span.style.bg == Some(fill.into())),
        "expected label span to carry node fill background"
    );
}

#[test]
fn flowchart_label_color_separate_from_border_color() {
    let fill = Color::Rgb(1, 2, 3);
    let label = Color::Rgb(255, 255, 255);
    let border = Color::Rgb(51, 51, 51);
    let rows = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![FlowNodeSpec {
                id: "A".into(),
                label: "Node".into(),
                shape: FlowNodeShape::Rect,
                style: NodeStyle {
                    fill: Some(fill),
                    label_fg: Some(label),
                    border_fg: Some(border),
                },
            }],
            edges: Vec::new(),
        },
        &default_styles(),
    );

    let border_span = rows
        .iter()
        .flatten()
        .find(|span| span.content.contains('┌'))
        .expect("border span");
    assert_eq!(border_span.style.fg, Some(border.into()));
    assert_eq!(border_span.style.bg, Some(fill.into()));

    let label_span = rows
        .iter()
        .flatten()
        .find(|span| span.content.contains("Node"))
        .expect("label span");
    assert_eq!(label_span.style.fg, Some(label.into()));
    assert_eq!(label_span.style.bg, Some(fill.into()));
}

#[test]
fn diagram_rasterizers_apply_document_style_slots() {
    let theme_fill = Color::Rgb(10, 20, 30);
    let theme_border = Color::Rgb(40, 50, 60);
    let theme_label = Color::Rgb(70, 80, 90);
    let theme_edge = Color::Rgb(100, 110, 120);
    let explicit_fill = Color::Rgb(130, 140, 150);
    let styles = DocumentStyles {
        diagram_node_fill_style: Style::new().bg(theme_fill),
        diagram_node_border_style: Style::new().fg(theme_border),
        diagram_node_label_style: Style::new().fg(theme_label),
        diagram_edge_style: Style::new().fg(theme_edge),
        ..DocumentStyles::default()
    };

    let flow_rows = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![
                FlowNodeSpec {
                    id: "A".into(),
                    label: "Start".into(),
                    shape: FlowNodeShape::Rect,
                    style: NodeStyle {
                        fill: Some(explicit_fill),
                        ..NodeStyle::default()
                    },
                },
                flow_node("B", "End", FlowNodeShape::Rect),
            ],
            edges: vec![FlowEdgeSpec {
                from: "A".into(),
                to: "B".into(),
                label: Some("go".into()),
                dashed: false,
            }],
        },
        &styles,
    );
    let flow_label = span_containing(&flow_rows, "Start");
    assert_eq!(flow_label.style.bg, Some(explicit_fill.into()));
    assert_eq!(flow_label.style.fg, Some(theme_label.into()));
    assert_eq!(
        span_containing(&flow_rows, "go").style.fg,
        Some(theme_edge.into())
    );

    let sequence_rows = rasterize_sequence(
        &SequenceSpec {
            participants: vec![
                SequenceParticipantSpec {
                    id: "A".into(),
                    label: "Alice".into(),
                    actor: false,
                },
                SequenceParticipantSpec {
                    id: "B".into(),
                    label: "Bob".into(),
                    actor: false,
                },
            ],
            messages: vec![SequenceMessageSpec {
                from: "A".into(),
                to: "B".into(),
                label: "hello".into(),
                dashed: false,
                open_arrow: false,
            }],
        },
        &styles,
    );
    let participant = span_containing(&sequence_rows, "Alice");
    assert_eq!(participant.style.bg, Some(theme_fill.into()));
    assert_eq!(participant.style.fg, Some(theme_label.into()));
    assert_eq!(
        span_containing(&sequence_rows, "hello").style.fg,
        Some(theme_edge.into())
    );

    let class_rows = rasterize_class(
        &ClassSpec {
            classes: vec![
                ClassNodeSpec {
                    name: "Animal".into(),
                    members: Vec::new(),
                },
                ClassNodeSpec {
                    name: "Dog".into(),
                    members: Vec::new(),
                },
            ],
            relations: vec![ClassRelationSpec {
                from: "Dog".into(),
                to: "Animal".into(),
                arrow: "-->".into(),
                from_cardinality: None,
                to_cardinality: None,
                label: Some("is".into()),
            }],
        },
        &styles,
    );
    assert_eq!(
        span_containing(&class_rows, "Animal").style.bg,
        Some(theme_fill.into())
    );
    assert_eq!(
        span_containing(&class_rows, "is").style.fg,
        Some(theme_edge.into())
    );

    let state_rows = rasterize_state(
        &StateSpec {
            states: vec![
                StateNodeSpec {
                    id: "Idle".into(),
                    label: "Idle".into(),
                    kind: StateKindSpec::State,
                },
                StateNodeSpec {
                    id: "Busy".into(),
                    label: "Busy".into(),
                    kind: StateKindSpec::State,
                },
            ],
            transitions: vec![StateTransitionSpec {
                from: "Idle".into(),
                to: "Busy".into(),
                label: Some("work".into()),
            }],
        },
        &styles,
    );
    assert_eq!(
        span_containing(&state_rows, "Idle").style.bg,
        Some(theme_fill.into())
    );
    assert_eq!(
        span_containing(&state_rows, "work").style.fg,
        Some(theme_edge.into())
    );

    let er_rows = rasterize_er(
        &ErSpec {
            entities: vec![
                ErEntitySpec {
                    name: "CUSTOMER".into(),
                    attributes: Vec::new(),
                },
                ErEntitySpec {
                    name: "ORDER".into(),
                    attributes: Vec::new(),
                },
            ],
            relations: vec![ErRelationSpec {
                left: "CUSTOMER".into(),
                right: "ORDER".into(),
                left_cardinality: "||".into(),
                right_cardinality: "}o".into(),
                label: Some("places".into()),
            }],
        },
        &styles,
    );
    assert_eq!(
        span_containing(&er_rows, "CUSTOMER").style.bg,
        Some(theme_fill.into())
    );
    assert_eq!(
        span_containing(&er_rows, "places").style.fg,
        Some(theme_edge.into())
    );
}

#[test]
#[cfg(feature = "markdown")]
fn mermaid_flowchart_regression_keeps_node_text_and_edge_labels_separate() {
    use super::super::mermaid::parse;

    let ParsedDiagram::Flowchart(spec) = parse(
        "graph TD\n\
             A[Client Request] --> B{API Gateway}\n\
             B -->|Authenticate| C[Auth Service]\n\
             B -->|Route| D[Load Balancer]\n\
             C -->|Valid Token| D\n\
             C -->|Invalid| E[401 Unauthorized]\n\
             D --> F[Service A]\n\
             D --> G[Service B]\n\
             D --> H[Service C]\n\
             F --> I[(Database)]\n\
             G --> J[Cache Layer]\n\
             H --> K[Message Queue]\n\
             K --> L[Worker Pool]\n\
             L --> M[Async Processor]\n\
             M --> I\n\
             J --> F\n\
             style A fill:#4CAF50,stroke:#333,color:#fff\n\
             style B fill:#2196F3,stroke:#333,color:#fff\n\
             style C fill:#FF9800,stroke:#333,color:#fff\n\
             style E fill:#F44336,stroke:#333,color:#fff\n\
             style I fill:#9C27B0,stroke:#333,color:#fff",
    )
    .unwrap() else {
        panic!("expected flowchart");
    };

    let rows = joined(rasterize_flowchart(&spec, &default_styles()));
    for label in [
        "Client Request",
        "API Gateway",
        "Auth Service",
        "Load Balancer",
        "401 Unauthorized",
        "Service A",
        "Service B",
        "Service C",
        "Cache Layer",
        "Message Queue",
        "Worker Pool",
        "Async Processor",
    ] {
        assert!(
            rows.contains(label),
            "missing node label {label:?}:\n{rows}"
        );
    }
    assert!(
        rows.contains("Valid Token"),
        "missing Valid Token label:\n{rows}"
    );
    assert!(rows.contains("Invalid"), "missing Invalid label:\n{rows}");
    assert!(rows.contains("( Database"), "missing cylinder cue:\n{rows}");
    assert!(
        !rows.contains("Valid TokenInvalid"),
        "edge labels collapsed into one run:\n{rows}"
    );
    assert!(
        !rows.contains("▼Valid Token"),
        "label should keep a space after arrowhead:\n{rows}"
    );
    assert!(
        !rows.contains("Valid Token┘"),
        "label should keep a space before route corner:\n{rows}"
    );
    assert!(
        !rows.contains("RouteService"),
        "edge label overpainted node text:\n{rows}"
    );
    let route_row = rows
        .lines()
        .find(|row| row.contains("Route"))
        .expect("Route label row");
    assert!(
        !route_row.contains("│ Route"),
        "Route should use the open left side of its vertical edge:\n{rows}"
    );
    assert!(
        !rows.contains("─Route─") && !rows.contains("─Invalid─"),
        "labels should clear line glyphs at both flanks:\n{rows}"
    );
    assert!(
        !rows.contains("─Async Processor"),
        "arrow or edge overpainted Async Processor:\n{rows}"
    );
    assert!(
        !rows.contains("└┼─┐"),
        "Route and Valid Token edges should not cross before Load Balancer:\n{rows}"
    );
    assert!(
        !rows.contains("└┐Valid Token") && !rows.contains("└┐ ▼"),
        "Route and Valid Token edges should not curl or disconnect before Load Balancer:\n{rows}"
    );
    assert!(
        !rows.contains("┌────────────▼"),
        "incoming arrowheads should sit above Load Balancer, not on its border:\n{rows}"
    );
    assert!(
        !rows.contains("┌─────▼") && !rows.contains("┌▼"),
        "incoming arrowheads should stay off service box borders:\n{rows}"
    );
    assert!(
        !rows.contains("└─┴┬") && !rows.contains("└─┼┐"),
        "Load Balancer fan-out branches should stagger instead of overlapping:\n{rows}"
    );
    assert!(
        !rows.contains("┌───┴──┬"),
        "Cache Layer -> Service A should not share the Service B/C fan-out lane:\n{rows}"
    );
    assert!(
        !rows.contains("▼─") && !rows.contains("─▼"),
        "arrowheads should not be cut by adjacent horizontal route segments:\n{rows}"
    );
}

#[test]
fn sequence_renders_participant_boxes_lifelines_and_messages() {
    let rows = joined(rasterize_sequence(
        &SequenceSpec {
            participants: vec![
                SequenceParticipantSpec {
                    id: "A".into(),
                    label: "Alice".into(),
                    actor: false,
                },
                SequenceParticipantSpec {
                    id: "B".into(),
                    label: "Bob".into(),
                    actor: false,
                },
            ],
            messages: vec![SequenceMessageSpec {
                from: "A".into(),
                to: "B".into(),
                label: "hello".into(),
                dashed: false,
                open_arrow: false,
            }],
        },
        &default_styles(),
    ));
    assert!(rows.contains('┌'));
    assert!(rows.contains('│'));
    assert!(rows.chars().any(|c| matches!(c, '▶' | '◀' | '▼' | '▲')));
    assert!(rows.contains("hello"));
    assert!(!rows.contains("sequenceDiagram"));
    assert!(!rows.contains("participants:"));
}

#[test]
fn class_and_er_relations_render_as_visual_edges() {
    let class_rows = joined(rasterize_class(
        &ClassSpec {
            classes: vec![
                ClassNodeSpec {
                    name: "Animal".into(),
                    members: Vec::new(),
                },
                ClassNodeSpec {
                    name: "Dog".into(),
                    members: Vec::new(),
                },
            ],
            relations: vec![ClassRelationSpec {
                from: "Dog".into(),
                to: "Animal".into(),
                arrow: "<|--".into(),
                from_cardinality: None,
                to_cardinality: None,
                label: None,
            }],
        },
        &default_styles(),
    ));
    assert!(class_rows.contains('┌'));
    assert!(
        class_rows
            .chars()
            .any(|c| matches!(c, '▷' | '◁' | '▽' | '△'))
    );
    assert!(!class_rows.contains("classDiagram"));
    assert!(!class_rows.contains("Dog <|-- Animal"));

    let er_rows = joined(rasterize_er(
        &ErSpec {
            entities: vec![
                ErEntitySpec {
                    name: "CUSTOMER".into(),
                    attributes: Vec::new(),
                },
                ErEntitySpec {
                    name: "ORDER".into(),
                    attributes: Vec::new(),
                },
            ],
            relations: vec![ErRelationSpec {
                left: "CUSTOMER".into(),
                right: "ORDER".into(),
                left_cardinality: "||".into(),
                right_cardinality: "}o".into(),
                label: Some("places".into()),
            }],
        },
        &default_styles(),
    ));
    assert!(er_rows.contains('┌'));
    assert!(
        er_rows.contains("||") || er_rows.contains('┿'),
        "expected `||` or vertical `┿` cardinality glyph"
    );
    assert!(
        er_rows.contains("}o") || er_rows.contains('┳') || er_rows.contains('┻'),
        "expected `}}o` or vertical crow's-foot glyph"
    );
    assert!(er_rows.contains("places"));
    assert!(!er_rows.contains("erDiagram"));
    assert!(!er_rows.contains("CUSTOMER ||--}o ORDER"));
}

#[test]
#[cfg(feature = "markdown")]
fn mermaid_er_regression_routes_orders_fanout_labels_apart() {
    use super::super::mermaid::parse;

    let ParsedDiagram::Er(spec) = parse(
        r#"erDiagram
    CUSTOMERS ||--o{ ORDERS : places
    ORDERS ||--|{ ORDER_ITEMS : contains
    PRODUCTS ||--o{ ORDER_ITEMS : "included in"
    ORDERS ||--|| PAYMENTS : "has one"
    CUSTOMERS ||--o{ ADDRESSES : "has many"
    CATEGORIES ||--o{ PRODUCTS : "contains"
    SUPPLIERS ||--o{ PRODUCTS : "supplies"
    ORDERS }|--|| SHIPPING : "ships via"

    CUSTOMERS {
        string id PK
        string email
        string first_name
        string last_name
        datetime created_at
        string phone
    }

    ORDERS {
        string order_id PK
        string customer_id FK
        datetime order_date
        decimal total_amount
        string status
        string shipping_address_id FK
    }

    ORDER_ITEMS {
        string id PK
        string order_id FK
        string product_id FK
        int quantity
        decimal unit_price
    }

    PRODUCTS {
        string sku PK
        string name
        decimal price
        int stock_quantity
        string category_id FK
        string supplier_id FK
    }

    PAYMENTS {
        string payment_id PK
        string order_id FK
        string method
        decimal amount
        string status
        datetime processed_at
    }

    ADDRESSES {
        string id PK
        string customer_id FK
        string street
        string city
        string state
        string zip_code
        string country
    }

    CATEGORIES {
        string id PK
        string name
        string description
        string parent_id FK
    }

    SUPPLIERS {
        string id PK
        string company_name
        string contact_email
        string phone
    }

    SHIPPING {
        string tracking_id PK
        string order_id FK
        string carrier
        datetime shipped_date
        datetime estimated_delivery
        string status
    }
"#,
    )
    .unwrap() else {
        panic!("expected ER diagram");
    };

    let rows = joined(rasterize_er(&spec, &default_styles()));
    let boxes = spec
        .entities
        .iter()
        .map(|entity| {
            let mut rows = vec![entity.name.clone()];
            rows.extend(entity.attributes.iter().map(|a| {
                let keys = a
                    .keys
                    .iter()
                    .map(|k| k.as_ref())
                    .collect::<Vec<_>>()
                    .join(",");
                if keys.is_empty() {
                    Arc::from(format!("{} {}", a.ty, a.name))
                } else {
                    Arc::from(format!("{} {} {keys}", a.ty, a.name))
                }
            }));
            SimpleDiagramBox {
                id: entity.name.clone(),
                rows,
                divider_after: vec![0],
                fill_style: Style::default(),
                border_style_fg: Style::default(),
                label_style: Style::default(),
                border_style: BorderStyle::Plain,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect::<Vec<_>>();
    let edges = spec
        .relations
        .iter()
        .map(|relation| SimpleDiagramEdge {
            from: relation.left.clone(),
            to: relation.right.clone(),
            label: relation.label.clone(),
            from_label: None,
            to_label: None,
            line_style: Style::default(),
            label_style: Style::default(),
            dashed: false,
            from_glyph: er_cardinality_glyph(&relation.left_cardinality),
            to_glyph: er_cardinality_glyph(&relation.right_cardinality),
            prefer_vertical_backedge_labels: true,
        })
        .collect::<Vec<_>>();
    let output = build_simple_diagram_output(&boxes, &edges, SIMPLE_DIAGRAM_PADDING, 3, 4);
    let contains = output
        .edges
        .iter()
        .find(|edge| {
            let spec = &edges[edge.spec_index];
            spec.from.as_ref() == "ORDERS" && spec.to.as_ref() == "ORDER_ITEMS"
        })
        .expect("ORDERS -> ORDER_ITEMS route");
    let ships_via = output
        .edges
        .iter()
        .find(|edge| {
            let spec = &edges[edge.spec_index];
            spec.from.as_ref() == "ORDERS" && spec.to.as_ref() == "SHIPPING"
        })
        .expect("ORDERS -> SHIPPING route");
    let has_one = output
        .edges
        .iter()
        .find(|edge| {
            let spec = &edges[edge.spec_index];
            spec.from.as_ref() == "ORDERS" && spec.to.as_ref() == "PAYMENTS"
        })
        .expect("ORDERS -> PAYMENTS route");
    let contains_source_x = contains.from_pos.expect("contains source").0;
    let ships_via_source_x = ships_via.from_pos.expect("ships via source").0;
    let has_one_source_x = has_one.from_pos.expect("has one source").0;
    let has_one_label_x = has_one.label_pos.expect("has one label").0;
    assert!(rows.contains("contains"));
    assert!(rows.contains("ships via"));
    assert!(rows.contains("has one"));
    assert!(
        has_one_label_x < has_one_source_x,
        "has one should use the open left side of the ORDERS -> PAYMENTS route:\n{rows}",
    );
    assert!(
        ships_via_source_x < contains_source_x,
        "ships via should use the source port left of contains:\n{rows}",
    );
    assert!(
        !has_perpendicular_overlap(contains, ships_via),
        "contains and ships via routes should not cross:\n{rows}",
    );
}

#[cfg(feature = "markdown")]
fn has_perpendicular_overlap(
    a: &crate::widgets::common::simple_diagram::SimplePositionedEdge,
    b: &crate::widgets::common::simple_diagram::SimplePositionedEdge,
) -> bool {
    a.cells.iter().any(|left| {
        b.cells.iter().any(|right| {
            let left_horizontal = is_horizontal(left.bits);
            let left_vertical = is_vertical(left.bits);
            let right_horizontal = is_horizontal(right.bits);
            let right_vertical = is_vertical(right.bits);
            left.x == right.x
                && left.y == right.y
                && ((left_horizontal && right_vertical) || (left_vertical && right_horizontal))
        })
    })
}

#[cfg(feature = "markdown")]
fn is_horizontal(bits: u8) -> bool {
    use crate::widgets::common::box_glyphs::{EAST, WEST};

    bits & (EAST | WEST) != 0
}

#[cfg(feature = "markdown")]
fn is_vertical(bits: u8) -> bool {
    use crate::widgets::common::box_glyphs::{NORTH, SOUTH};

    bits & (NORTH | SOUTH) != 0
}

#[test]
#[cfg(feature = "markdown")]
fn mermaid_state_regression_separates_ready_and_error_feedback_lanes() {
    use super::super::mermaid::parse;

    let ParsedDiagram::State(spec) = parse(
        r#"stateDiagram-v2
  [*] --> Idle
  Idle --> Loading
  Loading --> Ready
  Loading --> Error
  Error --> Idle
  Ready --> Loading
  Ready --> [*]
"#,
    )
    .unwrap() else {
        panic!("expected state diagram");
    };

    let rows = joined(rasterize_state(&spec, &default_styles()));
    assert!(rows.contains("Ready"));
    assert!(rows.contains("Error"));

    let boxes = spec
        .states
        .iter()
        .map(|state| SimpleDiagramBox {
            id: state.id.clone(),
            rows: vec![Arc::from(match state.kind {
                StateKindSpec::Start | StateKindSpec::End => {
                    display_state_id(&state.id).to_string()
                }
                StateKindSpec::Choice => "◇".to_string(),
                StateKindSpec::State => state.label.to_string(),
            })],
            divider_after: Vec::new(),
            fill_style: default_styles().diagram_node_fill_style,
            border_style_fg: default_styles().diagram_node_border_style,
            label_style: default_styles().diagram_node_label_style,
            border_style: BorderStyle::Plain,
            shape: SimpleDiagramBoxShape::Rect,
        })
        .collect::<Vec<_>>();
    let edges = spec
        .transitions
        .iter()
        .map(|transition| SimpleDiagramEdge {
            from: transition.from.clone(),
            to: transition.to.clone(),
            label: transition.label.clone(),
            from_label: None,
            to_label: None,
            line_style: default_styles().diagram_edge_style,
            label_style: default_styles().diagram_edge_style,
            dashed: false,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        })
        .collect::<Vec<_>>();
    let (extra_layer, extra_node) = congestion_gap_extras(&edges);
    let output = build_simple_diagram_output(
        &boxes,
        &edges,
        SIMPLE_DIAGRAM_PADDING,
        3u16.saturating_add(extra_layer),
        4u16.saturating_add(extra_node),
    );
    let ready_loading = output
        .edges
        .iter()
        .find(|edge| {
            let spec = &edges[edge.spec_index];
            spec.from.as_ref() == "Ready" && spec.to.as_ref() == "Loading"
        })
        .expect("Ready -> Loading route");
    let error_idle = output
        .edges
        .iter()
        .find(|edge| {
            let spec = &edges[edge.spec_index];
            spec.from.as_ref() == "Error" && spec.to.as_ref() == "Idle"
        })
        .expect("Error -> Idle route");
    let ready_exit_y = back_edge_source_exit_lane_y(ready_loading).expect("Ready back-edge exit");
    let error_exit_y = back_edge_source_exit_lane_y(error_idle).expect("Error back-edge exit");

    assert_ne!(
        ready_exit_y, error_exit_y,
        "Ready -> Loading and Error -> Idle should not share one feedback lane:\n{rows}",
    );
}

#[cfg(feature = "markdown")]
fn back_edge_source_exit_lane_y(
    edge: &crate::widgets::common::simple_diagram::SimplePositionedEdge,
) -> Option<i16> {
    use crate::widgets::common::box_glyphs::{EAST, WEST};

    let source_y = edge.from_pos?.1;
    edge.cells
        .iter()
        .filter_map(|cell| {
            ((cell.bits & (EAST | WEST)) == (EAST | WEST) && cell.y > source_y).then_some(cell.y)
        })
        .min()
}

#[test]
#[cfg(feature = "markdown")]
fn mermaid_class_diagram_with_cardinality_relations_renders_classes_not_source_rows() {
    use super::super::mermaid::parse;

    let ParsedDiagram::Class(spec) = parse(
        "classDiagram\n\
             class User {\n\
                 +String id\n\
                 +String username\n\
                 +String email\n\
                 +DateTime createdAt\n\
                 +login() Boolean\n\
                 +logout() void\n\
                 +updateProfile() User\n\
             }\n\
             class Order {\n\
                 +String orderId\n\
                 +DateTime orderDate\n\
                 +Float totalAmount\n\
                 +OrderStatus status\n\
                 +calculateTotal() Float\n\
                 +cancel() void\n\
                 +ship() void\n\
             }\n\
             class Product {\n\
                 +String sku\n\
                 +String name\n\
                 +Float price\n\
                 +Integer stock\n\
                 +updateStock() void\n\
                 +applyDiscount() Float\n\
             }\n\
             class OrderItem {\n\
                 +Integer quantity\n\
                 +Float unitPrice\n\
                 +getSubtotal() Float\n\
             }\n\
             class Payment {\n\
                 +String paymentId\n\
                 +PaymentMethod method\n\
                 +Float amount\n\
                 +process() Boolean\n\
                 +refund() void\n\
             }\n\
             User \"1\" --> \"*\" Order : places\n\
             Order \"1\" *-- \"*\" OrderItem : contains\n\
             OrderItem \"*\" --> \"1\" Product : references\n\
             Order \"1\" --> \"1\" Payment : has\n\
             Payment ..> User : made by",
    )
    .unwrap() else {
        panic!("expected class diagram");
    };

    let rows = joined(rasterize_class(&spec, &default_styles()));
    for text in [
        "User",
        "Order",
        "Product",
        "OrderItem",
        "Payment",
        "+id: String",
        "+username: String",
        "+orderId: String",
        "+quantity: Integer",
        "+paymentId: String",
        "1",
        "*",
        "places",
        "contains",
        "references",
        "made by",
    ] {
        assert!(
            rows.contains(text),
            "missing expected class diagram text {text:?}:\n{rows}"
        );
    }
    for text in [
        "User \"1\" --> \"*\" Order",
        "Order \"1\" *-- \"*\" OrderItem",
        "Payment ..> User",
        "+String: id",
        "+Integer: quantity",
    ] {
        assert!(
            !rows.contains(text),
            "unexpected source-summary artifact {text:?}:\n{rows}"
        );
    }
    assert!(
        rows.lines()
            .any(|row| row.contains("made by") && row.chars().any(|ch| matches!(ch, '┄' | '┆'))),
        "dependency label should stay attached to its dashed edge:\n{rows}"
    );
    for clutter in ["1 ◆ 1", "▼ *", "▼*", "│┆", "┐┆"] {
        assert!(
            !rows.contains(clutter),
            "unexpected class-diagram clutter pattern {clutter:?}:\n{rows}"
        );
    }
}

#[test]
fn state_renders_marker_boxes_and_display_end_marker() {
    let rows = joined(rasterize_state(
        &StateSpec {
            states: vec![
                StateNodeSpec {
                    id: "[*]".into(),
                    label: "[*]".into(),
                    kind: StateKindSpec::Start,
                },
                StateNodeSpec {
                    id: "Idle".into(),
                    label: "Idle".into(),
                    kind: StateKindSpec::State,
                },
                StateNodeSpec {
                    id: "[*]$end".into(),
                    label: "[*]".into(),
                    kind: StateKindSpec::End,
                },
            ],
            transitions: vec![
                StateTransitionSpec {
                    from: "[*]".into(),
                    to: "Idle".into(),
                    label: Some("begin".into()),
                },
                StateTransitionSpec {
                    from: "Idle".into(),
                    to: "[*]$end".into(),
                    label: Some("done".into()),
                },
            ],
        },
        &default_styles(),
    ));
    assert!(rows.contains('┌'));
    assert!(rows.chars().any(|c| matches!(c, '▶' | '◀' | '▼' | '▲')));
    assert!(rows.contains("[*]"));
    assert!(!rows.contains("[*]$end"));
    assert!(!rows.contains("stateDiagram-v2"));
}

#[test]
fn endpoint_glyphs_do_not_land_on_box_borders() {
    let rows = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![
                flow_node("A", "Start", FlowNodeShape::Rect),
                flow_node("B", "Ready?", FlowNodeShape::Rect),
            ],
            edges: vec![FlowEdgeSpec {
                from: "A".into(),
                to: "B".into(),
                label: None,
                dashed: false,
            }],
        },
        &default_styles(),
    );
    let rows = plain_rows(rows);
    for row in &rows {
        let on_border =
            row.contains('┌') || row.contains('└') || row.contains('┐') || row.contains('┘');
        if on_border {
            for arrow in ['▶', '◀', '▼', '▲'] {
                assert!(
                    !row.contains(arrow),
                    "endpoint arrow {arrow} drawn on box border row: {row:?}"
                );
            }
        }
    }
    assert!(
        rows.iter()
            .any(|r| r.chars().any(|c| matches!(c, '▶' | '◀' | '▼' | '▲'))),
        "expected arrow somewhere"
    );
}

#[test]
fn arrow_glyph_orients_to_approach_direction() {
    let down = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![
                flow_node("A", "A", FlowNodeShape::Rect),
                flow_node("B", "B", FlowNodeShape::Rect),
            ],
            edges: vec![FlowEdgeSpec {
                from: "A".into(),
                to: "B".into(),
                label: None,
                dashed: false,
            }],
        },
        &default_styles(),
    );
    let joined = joined(down);
    assert!(joined.contains('▼'), "top-down edge should use ▼: {joined}");
    assert!(
        !joined.contains('▶'),
        "should not use right arrow: {joined}"
    );
}

#[test]
fn boxes_have_no_empty_padding_rows() {
    let rows = rasterize_flowchart(
        &FlowchartSpec {
            direction: DiagramDirection::TopDown,
            nodes: vec![flow_node("A", "Start", FlowNodeShape::Rect)],
            edges: Vec::new(),
        },
        &default_styles(),
    );
    let rows = plain_rows(rows);
    let top = rows.iter().position(|r| r.contains('┌')).expect("top");
    let bottom = rows.iter().position(|r| r.contains('└')).expect("bottom");
    assert_eq!(
        bottom - top,
        2,
        "expected 3-row box (top/content/bottom), got {} rows",
        bottom - top + 1
    );
    assert!(
        rows[top + 1].contains("Start"),
        "label row missing: {:?}",
        rows[top + 1]
    );
}

#[test]
#[cfg(feature = "markdown")]
fn sequence_arrow_filled_for_double_tip() {
    use super::super::mermaid::parse;
    let ParsedDiagram::Sequence(spec) = parse("sequenceDiagram\n    A->>B: hi").unwrap() else {
        panic!("expected sequence");
    };
    assert!(
        !spec.messages[0].open_arrow,
        "->> should be filled, not open"
    );

    let ParsedDiagram::Sequence(spec2) = parse("sequenceDiagram\n    A->B: hi").unwrap() else {
        panic!("expected sequence");
    };
    assert!(spec2.messages[0].open_arrow, "-> should be open");
}

#[test]
fn sequence_message_label_does_not_overwrite_actor_box_bottom_border() {
    let rows = rasterize_sequence(
        &SequenceSpec {
            participants: vec![
                SequenceParticipantSpec {
                    id: "U".into(),
                    label: "User".into(),
                    actor: false,
                },
                SequenceParticipantSpec {
                    id: "A".into(),
                    label: "App".into(),
                    actor: false,
                },
            ],
            messages: vec![SequenceMessageSpec {
                from: "U".into(),
                to: "A".into(),
                label: "open docs".into(),
                dashed: false,
                open_arrow: false,
            }],
        },
        &default_styles(),
    );
    let rows = plain_rows(rows);
    let bottom_border = rows
        .iter()
        .find(|r| r.contains('└'))
        .expect("bottom border row");
    assert!(
        !bottom_border.contains("open docs"),
        "label collided with actor box bottom border: {bottom_border:?}"
    );
}

#[test]
fn sequence_messages_render_over_intermediate_lifelines() {
    let rows = rasterize_sequence(
        &SequenceSpec {
            participants: vec![
                SequenceParticipantSpec {
                    id: "A".into(),
                    label: "API".into(),
                    actor: false,
                },
                SequenceParticipantSpec {
                    id: "B".into(),
                    label: "AuthSvc".into(),
                    actor: false,
                },
                SequenceParticipantSpec {
                    id: "C".into(),
                    label: "Cache".into(),
                    actor: false,
                },
            ],
            messages: vec![SequenceMessageSpec {
                from: "A".into(),
                to: "C".into(),
                label: "checkRateLimit(userId)".into(),
                dashed: false,
                open_arrow: false,
            }],
        },
        &default_styles(),
    );
    let rows = plain_rows(rows);
    let arrow_row = rows
        .iter()
        .find(|row| row.contains('▶'))
        .expect("message arrow row");
    let arrow_start = arrow_row.find('─').expect("arrow start");
    let arrow_end = arrow_row.find('▶').expect("arrow end");

    assert!(
        !arrow_row[arrow_start..arrow_end].contains('│'),
        "intermediate lifeline should not overwrite message arrow: {arrow_row:?}"
    );
}

#[test]
#[cfg(feature = "markdown")]
fn sequence_mermaid_note_renders_as_diagram_row() {
    use super::super::mermaid::parse;
    let ParsedDiagram::Sequence(spec) = parse(
            "sequenceDiagram\n    participant Queue\n    Note over Queue: Async processing\n    Queue->>Worker: consume(orderPlaced)",
        )
        .unwrap()
        else {
            panic!("expected sequence");
        };

    let rows = joined(rasterize_sequence(&spec, &default_styles()));
    assert!(
        rows.contains("[ Async processing ]"),
        "missing note: {rows}"
    );
    assert!(
        rows.contains("consume(orderPlaced)"),
        "missing message: {rows}"
    );
}
