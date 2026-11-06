//! Context menus resolve semantic targets without changing primary-click state.
use std::cell::RefCell;
use std::rc::Rc;

use crate::callback::Callback;
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Length, ScrollbarConfig, ScrollbarVariant};
use crate::widgets::*;
use crate::{FlowchartItemEvent, TestBackend};

type Captured<T> = Rc<RefCell<Vec<T>>>;
fn capture<T: 'static>() -> (Captured<T>, Callback<T>) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    (
        events,
        Callback::new(move |event| sink.borrow_mut().push(event)),
    )
}

struct View(Element);
impl Component for View {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        self.0.clone()
    }
}
fn mount(element: impl Into<Element>) -> TestBackend<View> {
    let mut backend = TestBackend::new(View(VStack::new().child(element).into()));
    backend.render();
    backend
}
fn gesture(backend: &mut TestBackend<View>, x: u16, y: u16, button: MouseButton) {
    for kind in [
        MouseKind::Down(button),
        MouseKind::Drag(button),
        MouseKind::Up(button),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind,
                mods: KeyMods::CTRL,
            })
            .unwrap();
    }
}
fn right_click(backend: &mut TestBackend<View>, x: u16, y: u16) {
    gesture(backend, x, y, MouseButton::Right);
}

#[test]
fn context_menu_list_gap_is_excluded_from_primary_and_secondary_clicks() {
    let (menus, on_menu) = capture();
    let (selected, on_select) = capture();
    let mut backend = mount(
        List::new()
            .items((0..10).map(|i| ListItem::new(format!("item {i}"))))
            .width(Length::Px(12))
            .height(Length::Px(3))
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().gap(2))
            .on_select(on_select)
            .on_context_menu(on_menu),
    );
    for x in [9, 10, 11] {
        right_click(&mut backend, x, 1);
        if x < 11 {
            gesture(&mut backend, x, 1, MouseButton::Left);
        }
    }
    assert!(menus.borrow().is_empty());
    assert!(selected.borrow().is_empty());
    right_click(&mut backend, 8, 1);
    assert_eq!(menus.borrow()[0].0.index, 1);
    gesture(&mut backend, 8, 1, MouseButton::Left);
    assert_eq!(selected.borrow()[0].index, 1);
}

#[test]
fn context_menu_list_keeps_content_width_with_integrated_scrollbar() {
    let (menus, callback) = capture();
    let mut backend = mount(
        List::new()
            .items((0..10).map(|i| ListItem::new(format!("item {i}"))))
            .border(true)
            .width(Length::Px(12))
            .height(Length::Px(5))
            .scrollbar(true)
            .scrollbar_config(
                ScrollbarConfig::new()
                    .variant(ScrollbarVariant::Integrated)
                    .gap(2),
            )
            .on_context_menu(callback),
    );
    right_click(&mut backend, 10, 2);
    assert_eq!(menus.borrow()[0].0.index, 1);
    right_click(&mut backend, 11, 2);
    assert_eq!(menus.borrow().len(), 1);
}

#[test]
fn context_menu_only_list_does_not_request_a_hand_pointer() {
    let (_, callback) = capture::<(ListEvent, MouseEvent)>();
    let backend = mount(
        List::new()
            .item(ListItem::new("item"))
            .on_context_menu(callback),
    );
    let node = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::List(_)))
        .unwrap();
    assert!(node.kind.has_on_click());
    assert_eq!(
        node.kind
            .interactive_pointer_shape_at(node.rect.x, node.rect.y, node.rect),
        None
    );
}

#[test]
fn context_menu_table_resolves_scrolled_multiline_rows_and_excludes_chrome() {
    let (menus, callback) = capture();
    let (selections, on_select) = capture();
    let mut backend = mount(
        Table::new()
            .header(TableRow::new(["header"]))
            .rows([
                TableRow::new(["first\ncontinuation"]),
                TableRow::new(["second"]),
                TableRow::new(["third"]),
            ])
            .row_gap(1)
            .selected(Some(0))
            .border(true)
            .width(Length::Px(20))
            .height(Length::Px(8))
            .on_select(on_select)
            .on_context_menu(callback),
    );
    let id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Table(_)))
        .unwrap()
        .id;
    if let NodeKind::Table(table) = &mut backend.core.tree.node_mut(id).kind {
        table.offset = 1;
        table.scroll_override = Some(1);
    }
    let focus = backend.focused();
    // Header and header gap take two lines. First visible data row is source row 1.
    right_click(&mut backend, 4, 3);
    assert_eq!(menus.borrow()[0].0.index, 1);
    assert_eq!(menus.borrow()[0].1.mods, KeyMods::CTRL);
    assert_eq!(backend.focused(), focus);
    for (x, y) in [(0, 3), (4, 1), (4, 2), (4, 4), (4, 6), (4, 7)] {
        right_click(&mut backend, x, y);
    }
    assert_eq!(menus.borrow().len(), 1);
    assert!(selections.borrow().is_empty());
    let NodeKind::Table(table) = &backend.core.tree.node(id).kind else {
        unreachable!()
    };
    assert_eq!(table.selected, Some(0));
}

#[test]
fn context_menu_table_excludes_scrollbar_gap_and_section_rows() {
    let (menus, callback) = capture();
    let mut backend = mount(
        Table::new()
            .rows((0..12).map(|i| TableRow::new([format!("row {i}")])))
            .width(Length::Px(12))
            .height(Length::Px(3))
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().gap(2))
            .on_context_menu(callback),
    );
    for x in [9, 10, 11] {
        right_click(&mut backend, x, 1);
    }
    assert!(menus.borrow().is_empty());
    right_click(&mut backend, 8, 1);
    assert_eq!(menus.borrow()[0].0.index, 1);
    let id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Table(_)))
        .unwrap()
        .id;
    if let NodeKind::Table(table) = &mut backend.core.tree.node_mut(id).kind {
        table.rows = vec![TableRow::section("section"), TableRow::separator()].into();
    }
    right_click(&mut backend, 4, 0);
    right_click(&mut backend, 4, 1);
    assert_eq!(menus.borrow().len(), 1);
}

#[test]
fn context_menu_tabs_reports_hit_tab_without_switching() {
    let (menus, callback) = capture();
    let (changes, on_change) = capture();
    let mut backend = mount(
        Tabs::new()
            .tabs([Tab::new("one"), Tab::new("two")])
            .width(Length::Px(30))
            .active(0)
            .on_change(on_change)
            .on_context_menu(callback),
    );
    // Derive a target column through the same public geometry as rendering.
    let (x, y) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::Tabs(tabs) = &node.kind else {
                return None;
            };
            let inner = node.rect.inner(tabs.border, tabs.padding);
            (0..inner.w)
                .find(|x| {
                    Tabs::index_at_col(
                        &tabs.tabs,
                        tabs.divider,
                        tabs.overflow,
                        inner.w as usize,
                        *x as usize,
                    ) == Some(1)
                })
                .map(|x| ((inner.x + x as i16) as u16, inner.y as u16))
        })
        .unwrap();
    right_click(&mut backend, x, y);
    right_click(&mut backend, 29, y);
    assert_eq!(
        menus
            .borrow()
            .iter()
            .map(|event| event.0)
            .collect::<Vec<_>>(),
        [Some(1), None]
    );
    assert!(changes.borrow().is_empty());
}

#[test]
fn context_menu_multiselect_preserves_checked_and_active_items() {
    let (menus, callback) = capture();
    let (toggles, on_toggle) = capture();
    let (changes, on_change) = capture();
    let mut backend = mount(
        MultiSelect::new()
            .items(["one", "two", "three"])
            .selected_indices([0, 2])
            .active_index(0)
            .on_toggle(on_toggle)
            .on_change(on_change)
            .on_context_menu(callback),
    );
    let (x, y) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::List(list) = &node.kind else {
                return None;
            };
            let inner = node.rect.inner(list.border, list.padding);
            Some((inner.x as u16, (inner.y + 1) as u16))
        })
        .unwrap();
    right_click(&mut backend, x, y);
    assert_eq!(menus.borrow()[0].0.index, 1);
    assert!(toggles.borrow().is_empty());
    assert!(changes.borrow().is_empty());
    let list = backend
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::List(list) => Some(list),
            _ => None,
        })
        .unwrap();
    assert_eq!(list.selected, Some(0));
}

#[cfg(feature = "markdown")]
#[test]
fn context_menu_document_reports_wrapped_source_line_and_link() {
    let (menus, callback) = capture();
    let mut backend = mount(
        DocumentView::new("[linked words](https://example.com)\nnext line")
            .formatter(MarkdownFormatter::default())
            .wrap(true)
            .width(Length::Px(8))
            .height(Length::Px(6))
            .on_context_menu(callback),
    );
    let node = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .unwrap();
    let rect = node.rect;
    right_click(&mut backend, rect.x as u16 + 1, rect.y as u16 + 2);
    let events = menus.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0.source_line, 0);
    assert_eq!(events[0].0.link.as_deref(), Some("https://example.com"));
}

#[test]
fn context_menu_editors_do_not_move_caret_or_selection() {
    let (input_menus, input_callback) = capture();
    let mut backend = mount(
        Input::new("abc def")
            .cursor(3)
            .anchor(Some(0))
            .on_context_menu(input_callback),
    );
    let id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Input(_)))
        .unwrap()
        .id;
    right_click(&mut backend, 5, 1);
    assert_eq!(input_menus.borrow().len(), 1);
    let NodeKind::Input(input) = &backend.core.tree.node(id).kind else {
        unreachable!()
    };
    assert_eq!((input.cursor, input.anchor), (3, Some(0)));

    let (area_menus, area_callback) = capture();
    let mut backend = mount(
        TextArea::new("abc\ndef")
            .cursor(2)
            .on_context_menu(area_callback),
    );
    let (id, content) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::TextArea(area) = &node.kind else {
                return None;
            };
            Some((node.id, area.content_rect(node.rect)))
        })
        .unwrap();
    right_click(&mut backend, content.x as u16, content.y as u16 + 1);
    assert_eq!(area_menus.borrow().len(), 1);
    let NodeKind::TextArea(area) = &backend.core.tree.node(id).kind else {
        unreachable!()
    };
    assert_eq!(area.cursor, 2);
}

#[test]
fn context_menu_disabled_widgets_bubble_to_the_ancestor() {
    let (menus, callback) = capture();
    let (ancestors, ancestor_callback) = capture();
    let mut backend = mount(
        MouseRegion::new()
            .child(
                List::new()
                    .item(ListItem::new("one"))
                    .disabled(true)
                    .on_context_menu(callback),
            )
            .on_context_menu(ancestor_callback),
    );
    right_click(&mut backend, 0, 0);
    assert!(menus.borrow().is_empty());
    assert_eq!(ancestors.borrow().len(), 1);
}

#[cfg(feature = "terminal")]
#[test]
fn context_menu_terminal_preempts_mouse_forwarding_for_the_whole_gesture() {
    let (menus, callback) = capture();
    let (forwarded, forward_callback) = capture();
    let mut terminal = Terminal::new()
        .content("terminal")
        .on_mouse_forward_event(forward_callback)
        .on_context_menu(callback);
    terminal.mouse_mode = MouseModeState {
        mode: MouseMode::AnyEvent,
        encoding: MouseEncoding::Sgr,
        ..Default::default()
    };
    let mut backend = mount(terminal);
    right_click(&mut backend, 1, 0);
    assert_eq!(menus.borrow().len(), 1);
    assert!(forwarded.borrow().is_empty());
}

#[test]
fn context_menu_diagrams_report_semantic_items() {
    let (graph_events, graph_callback) = capture();
    let mut backend = mount(
        Graph::new()
            .root(GraphNode::new("root"))
            .on_context_menu(graph_callback),
    );
    let (x, y) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::Graph(graph) = &node.kind else {
                return None;
            };
            let item = graph.output.nodes.first()?;
            let content = node.rect.inner(graph.border, graph.padding);
            Some((
                (content.x + item.rect.x) as u16,
                (content.y + item.rect.y) as u16,
            ))
        })
        .unwrap();
    right_click(&mut backend, x, y);
    assert_eq!(graph_events.borrow()[0].0.label.as_ref(), "root");

    let (flow_events, flow_callback) = capture();
    let mut backend = mount(
        Flowchart::new(FlowDirection::TopDown)
            .node("n", "Node", NodeShape::Rect)
            .on_context_menu(flow_callback),
    );
    let (x, y) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::Flowchart(flow) = &node.kind else {
                return None;
            };
            let item = flow.output.nodes.first()?;
            let content = node.rect.inner(flow.border, flow.padding);
            Some((
                (content.x + item.rect.x) as u16,
                (content.y + item.rect.y) as u16,
            ))
        })
        .unwrap();
    right_click(&mut backend, x, y);
    let events = flow_events.borrow();
    let FlowchartItemEvent::Node(event) = &events[0].0 else {
        panic!("expected node")
    };
    assert_eq!(event.label.as_ref(), "Node");

    let (sequence_events, sequence_callback) = capture();
    let mut backend = mount(
        SequenceDiagram::new()
            .participant("Alice")
            .participant("Bob")
            .message(SequenceMessage::new("Alice", "Bob", "hello"))
            .on_context_menu(sequence_callback),
    );
    let (x, y) = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::SequenceDiagram(sequence) = &node.kind else {
                return None;
            };
            let content = sequence.content_rect(node.rect);
            (0..content.h).find_map(|y| {
                (0..content.w).find_map(|x| {
                    let path = sequence.hit_test(x, y)?;
                    let event = sequence.item_event(path)?;
                    (event.label.as_ref() == "hello")
                        .then_some(((content.x + x as i16) as u16, (content.y + y as i16) as u16))
                })
            })
        })
        .unwrap();
    right_click(&mut backend, x, y);
    assert_eq!(sequence_events.borrow()[0].0.label.as_ref(), "hello");
}

#[cfg(feature = "diff-view")]
#[test]
fn context_menu_diff_maps_source_anchors_in_both_backends_and_modes() {
    for backend_kind in [DiffViewBackend::TextArea, DiffViewBackend::DocumentView] {
        for mode in [DiffViewMode::Unified, DiffViewMode::Split] {
            let (menus, callback) = capture();
            let mut backend = mount(
                DiffView::with_content(
                    "alpha\nold beta with enough words to wrap\nomega\n",
                    "alpha\nnew beta with enough words to wrap\nomega\n",
                )
                .backend(backend_kind)
                .mode(mode)
                .wrap(true)
                .width(Length::Px(40))
                .height(Length::Px(20))
                .on_context_menu(callback),
            );
            let targets = backend
                .core
                .tree
                .iter()
                .filter_map(|node| match &node.kind {
                    NodeKind::TextArea(area) => {
                        let config = area.diff_line_click.as_ref()?;
                        let lines = area.visual_cache.latest_lines()?;
                        let (visual, line) = lines.iter().enumerate().find(|(_, line)| {
                            config
                                .events_by_source_line
                                .get(line.line_num.saturating_sub(1))
                                .copied()
                                .flatten()
                                .is_some_and(|event| {
                                    event.anchor.new_line == Some(2)
                                        || event.anchor.old_line == Some(2)
                                })
                        })?;
                        let event = config.events_by_source_line[line.line_num.saturating_sub(1)]?;
                        let inner = node.rect.inner(area.border, area.padding);
                        Some((inner.x as u16, inner.y as u16 + visual as u16, event))
                    }
                    NodeKind::DocumentView(doc) => {
                        let config = doc.diff_line_click.as_ref()?;
                        let (visual, source) =
                            doc.visual_cache.source_line_map.iter().enumerate().find(
                                |(_, source)| {
                                    config
                                        .events_by_source_line
                                        .get(**source)
                                        .copied()
                                        .flatten()
                                        .is_some_and(|event| {
                                            event.anchor.new_line == Some(2)
                                                || event.anchor.old_line == Some(2)
                                        })
                                },
                            )?;
                        let event = config.events_by_source_line[*source]?;
                        let inner = node.rect.inner(doc.border, doc.padding);
                        Some((inner.x as u16, inner.y as u16 + visual as u16, event))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                targets.len(),
                if mode == DiffViewMode::Split { 2 } else { 1 }
            );
            for (x, y, mut expected) in targets {
                right_click(&mut backend, x, y);
                expected.mods = KeyMods::CTRL;
                let events = menus.borrow();
                let (actual, mouse) = events.last().expect("source row emits context menu");
                assert_eq!(*actual, expected, "{backend_kind:?} {mode:?}");
                assert_eq!((mouse.x, mouse.y), (x, y));
            }
        }
    }
}

#[cfg(feature = "terminal")]
#[test]
fn context_menu_managed_terminal_forwards_the_opt_in_callback() {
    let (menus, callback) = capture();
    let mut backend = mount(crate::child(
        ManagedTerminal::new,
        ManagedTerminalProps {
            auto_start: false,
            placeholder: None,
            on_context_menu: Some(callback),
            ..Default::default()
        },
    ));
    right_click(&mut backend, 1, 1);
    assert_eq!(menus.borrow().len(), 1);
}

#[cfg(feature = "diff-view")]
#[test]
fn context_menu_diff_ignores_synthetic_wrap_alignment_rows() {
    for backend_kind in [DiffViewBackend::TextArea, DiffViewBackend::DocumentView] {
        let (menus, callback) = capture();
        let mut backend = mount(
            DiffView::with_content(
                "old\n",
                "new with many more words that wrap over several visual rows\n",
            )
            .backend(backend_kind)
            .mode(DiffViewMode::Split)
            .wrap(true)
            .width(Length::Px(40))
            .height(Length::Px(20))
            .on_context_menu(callback),
        );
        let target = backend.core.tree.iter().find_map(|node| match &node.kind {
            NodeKind::TextArea(area) => {
                let visual = area.visual_cache.latest_lines()?.iter()
                    .position(|line| line.continuation && line.start == line.end)?;
                let inner = node.rect.inner(area.border, area.padding);
                Some((inner.x as u16, inner.y as u16 + visual as u16))
            }
            NodeKind::DocumentView(doc) => {
                use crate::widgets::document_view::node::VisualLineKind;
                let visual = doc.visual_cache.lines.iter().position(|line|
                    matches!(&line.kind, VisualLineKind::Text { continuation: true, spans, .. } if spans.is_empty()))?;
                let inner = node.rect.inner(doc.border, doc.padding);
                Some((inner.x as u16, inner.y as u16 + visual as u16))
            }
            _ => None,
        }).expect("asymmetric source lines must produce alignment padding");
        right_click(&mut backend, target.0, target.1);
        assert!(
            menus.borrow().is_empty(),
            "{backend_kind:?}: padding has no source target"
        );
    }
}
