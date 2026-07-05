use tui_lipan::prelude::*;

struct DiagramShowcase;

enum AppMsg {
    TabChanged(TabsEvent),
    ItemClicked(SequenceItemEvent),
    ItemHovered(SequenceItemEvent),
}

#[derive(Default)]
struct State {
    active_tab: usize,
    last_clicked: Option<SequenceItemEvent>,
    last_hovered: Option<SequenceItemEvent>,
}

impl Component for DiagramShowcase {
    type Message = AppMsg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tabs = Tabs::new()
            .tab("Sequence")
            .tab("Class")
            .tab("ER")
            .tab("State")
            .tab("Gantt")
            .active(ctx.state.active_tab.min(4))
            .on_change(ctx.link().callback(AppMsg::TabChanged));

        let content = match ctx.state.active_tab {
            0 => sequence_tab(ctx),
            1 => class_tab(),
            2 => er_tab(),
            3 => state_tab(),
            _ => gantt_tab(),
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new("Diagram Showcase").style(Style::new().fg(Color::DarkGray)))
            .child(tabs)
            .child(content)
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            AppMsg::TabChanged(event) => ctx.state.active_tab = event.index.min(4),
            AppMsg::ItemClicked(event) => ctx.state.last_clicked = Some(event),
            AppMsg::ItemHovered(event) => ctx.state.last_hovered = Some(event),
        }
        Update::full()
    }
}

fn sequence_tab(ctx: &Context<DiagramShowcase>) -> Element {
    let click = ctx.link().callback(AppMsg::ItemClicked);
    let hover = ctx.link().callback(AppMsg::ItemHovered);
    let hover_style =
        Style::new().transform_fg(ColorTransform::Tint(Color::Rgb(167, 243, 208), 0.75));

    let request_reply = SequenceDiagram::new()
        .participant("Browser")
        .participant("API")
        .participant_aliased("DB", "Database")
        .actor_kind("Browser", ActorKind::Actor)
        .message(SequenceMessage::sync("Browser", "API", "POST /orders").activate_target(true))
        .message(SequenceMessage::sync("API", "DB", "insert order").activate_target(true))
        .message(SequenceMessage::reply("DB", "API", "order id").deactivate_source(true))
        .message(SequenceMessage::reply("API", "Browser", "201 Created").deactivate_source(true))
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let async_and_self = SequenceDiagram::new()
        .participant("Scheduler")
        .participant("Worker")
        .participant_aliased("Q", "Queue")
        .actor_kind("Scheduler", ActorKind::Actor)
        .message(SequenceMessage::async_("Scheduler", "Q", "enqueue job"))
        .message(SequenceMessage::async_("Q", "Worker", "deliver job").activate_target(true))
        .step(Step::self_msg("Worker", "validate cache"))
        .step(Step::note_over(
            ["Worker"],
            "Self messages stay anchored to one lifeline",
        ))
        .message(SequenceMessage::reply("Worker", "Q", "ack").deactivate_source(true))
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Auto)
        .height(Length::Auto);

    let branching_steps: Vec<SequenceStep> = vec![
        Step::message(SequenceMessage::sync("Client", "Service", "poll status")),
        Step::fragment_begin(FragmentKind::Loop, "while pending"),
        Step::message(SequenceMessage::sync("Service", "Cache", "read state")),
        Step::fragment_begin(FragmentKind::Alt, "cache hit"),
        Step::message(SequenceMessage::reply("Cache", "Service", "pending")),
        Step::fragment_branch(FragmentKind::Alt, "else cache miss"),
        Step::message(SequenceMessage::sync("Service", "Store", "load state")),
        Step::message(SequenceMessage::reply("Store", "Service", "pending")),
        Step::fragment_end(),
        Step::fragment_end(),
    ];

    let loop_alt = branching_steps
        .into_iter()
        .fold(
            SequenceDiagram::new()
                .participant("Client")
                .participant("Service")
                .participant("Cache")
                .participant("Store"),
            |diagram, step| diagram.step(step),
        )
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Auto)
        .height(Length::Auto);

    let parallel_notes = SequenceDiagram::new()
        .participant("Coordinator")
        .participant("Index")
        .participant("BlobStore")
        .step(Step::note_over(
            ["Coordinator", "BlobStore"],
            "Notes can span multiple participants",
        ))
        .step(Step::fragment_begin(FragmentKind::Par, "fan out"))
        .message(SequenceMessage::async_(
            "Coordinator",
            "Index",
            "rebuild index",
        ))
        .step(Step::fragment_branch(FragmentKind::Par, "and"))
        .message(SequenceMessage::async_(
            "Coordinator",
            "BlobStore",
            "compact blobs",
        ))
        .step(Step::fragment_end())
        .step(Step::note(
            NotePlacement::RightOf,
            ["BlobStore"],
            "Right-of note",
        ))
        .step(Step::activate("Coordinator"))
        .message(SequenceMessage::sync(
            "Coordinator",
            "Index",
            "collect result",
        ))
        .message(SequenceMessage::sync(
            "Coordinator",
            "BlobStore",
            "collect result",
        ))
        .step(Step::deactivate("Coordinator"))
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let minimal_flow = SequenceDiagram::new()
        .participant("Client")
        .participant("Gateway")
        .participant("Worker")
        .actor_kind("Client", ActorKind::Actor)
        .message(SequenceMessage::sync("Client", "Gateway", "submit"))
        .message(SequenceMessage::async_("Gateway", "Worker", "dispatch"))
        .message(SequenceMessage::reply("Worker", "Client", "accepted"))
        .minimal()
        .actor_glyph("󰋦 ")
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let ascii_flow = SequenceDiagram::new()
        .participant("CLI")
        .participant("API")
        .participant("Worker")
        .message(SequenceMessage::sync("CLI", "API", "start export"))
        .message(SequenceMessage::async_("API", "Worker", "queue job"))
        .message(SequenceMessage::reply("Worker", "CLI", "done"))
        .theme(SequenceDiagramTheme::ascii())
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let styled_failures = SequenceDiagram::new()
        .participant("Client")
        .participant("Gateway")
        .participant("Upstream")
        .message(SequenceMessage::sync("Client", "Gateway", "fetch profile"))
        .message(SequenceMessage::lost("Gateway", "Upstream", "timeout"))
        .message(SequenceMessage::open("Gateway", "Client", "retry later"))
        .message_kind_style(
            MessageStyle::Lost,
            Style::new().fg(Color::Rgb(239, 68, 68)).bold(),
        )
        .message_kind_style(MessageStyle::Open, Style::new().fg(Color::Rgb(234, 179, 8)))
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let long_labels_and_wrapping = SequenceDiagram::new()
            .participant_aliased("U", "User Agent")
            .participant_aliased("GW", "Gateway")
            .participant_aliased("POL", "Policy Engine")
            .participant_aliased("AUD", "Audit Log")
            .actor_kind("U", ActorKind::Actor)
            .message(SequenceMessage::sync(
                "U",
                "GW",
                "POST /sessions with device fingerprint and regional hints",
            ))
            .message(SequenceMessage::sync(
                "GW",
                "POL",
                "evaluate risk score with long rule explanation",
            ))
            .step(Step::note_over(
                ["GW", "POL"],
                "This note intentionally wraps across several visual lines so the diagram has to preserve message rows after tall annotations.",
            ))
            .message(SequenceMessage::async_(
                "GW",
                "AUD",
                "append immutable security audit event",
            ))
            .message(SequenceMessage::reply(
                "POL",
                "GW",
                "allow with step-up challenge",
            ))
            .message(SequenceMessage::reply("GW", "U", "401 challenge required"))
            .max_label_cells(Some(28))
            .message_label_overflow(Overflow::Wrap)
            .autonumber(true)
            .item_hover_style(hover_style)
            .on_item_click(click.clone())
            .on_item_hover(hover.clone())
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .height(Length::Auto);

    let nested_fragments = SequenceDiagram::new()
        .participant("CLI")
        .participant("Controller")
        .participant("Cache")
        .participant("Primary")
        .participant("Replica")
        .message(SequenceMessage::sync("CLI", "Controller", "sync workspace"))
        .step(Step::fragment_begin(
            FragmentKind::Loop,
            "for each open file",
        ))
        .message(SequenceMessage::sync(
            "Controller",
            "Cache",
            "lookup digest",
        ))
        .step(Step::fragment_begin(FragmentKind::Alt, "digest changed"))
        .message(
            SequenceMessage::sync("Controller", "Primary", "write patch").activate_target(true),
        )
        .step(Step::fragment_begin(FragmentKind::Par, "replicate"))
        .message(SequenceMessage::async_(
            "Primary",
            "Replica",
            "stream delta",
        ))
        .step(Step::fragment_branch(FragmentKind::Par, "and"))
        .message(SequenceMessage::async_(
            "Primary",
            "Cache",
            "invalidate digest",
        ))
        .step(Step::fragment_end())
        .message(SequenceMessage::reply("Primary", "Controller", "ok").deactivate_source(true))
        .step(Step::fragment_branch(FragmentKind::Alt, "else unchanged"))
        .step(Step::note(NotePlacement::RightOf, ["Cache"], "Skip write"))
        .step(Step::fragment_end())
        .step(Step::fragment_end())
        .message(SequenceMessage::reply(
            "Controller",
            "CLI",
            "workspace synced",
        ))
        .fragment_kind_style(FragmentKind::Loop, Style::new().fg(Color::LightCyan))
        .fragment_kind_style(FragmentKind::Alt, Style::new().fg(Color::LightYellow))
        .fragment_kind_style(FragmentKind::Par, Style::new().fg(Color::LightMagenta))
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let dense_lifelines = SequenceDiagram::new()
        .participant_aliased("A", "App")
        .participant_aliased("B", "Broker")
        .participant_aliased("W1", "Worker 1")
        .participant_aliased("W2", "Worker 2")
        .participant_aliased("S", "Storage")
        .participant_aliased("N", "Notifier")
        .message(SequenceMessage::async_("A", "B", "publish jobs"))
        .message(SequenceMessage::async_("B", "W1", "job #1").activate_target(true))
        .message(SequenceMessage::async_("B", "W2", "job #2").activate_target(true))
        .step(Step::self_msg("W1", "dedupe"))
        .step(Step::self_msg("W2", "validate"))
        .message(SequenceMessage::sync("W1", "S", "write artifact"))
        .message(SequenceMessage::sync("W2", "S", "write artifact"))
        .message(SequenceMessage::reply("S", "W1", "etag"))
        .message(SequenceMessage::reply("S", "W2", "etag"))
        .message(SequenceMessage::async_("W1", "N", "notify success"))
        .message(SequenceMessage::async_("W2", "N", "notify success"))
        .message(SequenceMessage::reply("W1", "B", "done").deactivate_source(true))
        .message(SequenceMessage::reply("W2", "B", "done").deactivate_source(true))
        .minimal()
        .autonumber(true)
        .item_hover_style(hover_style)
        .on_item_click(click.clone())
        .on_item_hover(hover.clone())
        .border(true)
        .padding(1)
        .width(Length::Flex(1))
        .height(Length::Auto);

    let clicked = describe_event("clicked", ctx.state.last_clicked.as_ref());
    let hovered = describe_event("hovered", ctx.state.last_hovered.as_ref());

    VStack::new()
        .child(
            ScrollView::new()
                .scrollbar(true)
                .scroll_keys(ScrollKeymap::DEFAULT)
                .show_scroll_indicators(false)
                .padding(1)
                .child(
                    VStack::new()
                        .gap(1)
                        .child(section(
                            "Sync request/reply with activations",
                            request_reply,
                        ))
                        .child(section(
                            "Async queue, self message, and note",
                            async_and_self,
                        ))
                        .child(section("Loop with alt/else branches", loop_alt))
                        .child(section(
                            "Parallel branches, notes, and manual activation",
                            parallel_notes,
                        ))
                        .child(section(
                            "Minimal variant with custom actor glyph",
                            minimal_flow,
                        ))
                        .child(section("ASCII theme for plain terminals", ascii_flow))
                        .child(section(
                            "Message-kind styling for failures",
                            styled_failures,
                        ))
                        .child(section(
                            "Wrapped message labels and notes",
                            long_labels_and_wrapping,
                        ))
                        .child(section("Nested loop/alt/par fragments", nested_fragments))
                        .child(section("Dense minimal lifelines", dense_lifelines)),
                ),
        )
        .child(
            Frame::new()
                .title("Sequence item status")
                .border(true)
                .height(Length::Auto)
                .padding((0, 1))
                .child(Text::new(format!("{clicked} | {hovered}"))),
        )
        .into()
}

fn class_tab() -> Element {
    let diagram = ClassDiagram::new()
        .class("Animal")
        .attribute("Animal", ClassVisibility::Protected, "name", "String")
        .method("Animal", ClassVisibility::Public, "speak", "()")
        .class("Dog")
        .attribute("Dog", ClassVisibility::Private, "breed", "String")
        .method("Dog", ClassVisibility::Public, "speak", "()")
        .class("Owner")
        .relation(
            "Animal",
            "Dog",
            ClassRelationKind::Inheritance,
            None::<std::sync::Arc<str>>,
            None::<std::sync::Arc<str>>,
            None::<std::sync::Arc<str>>,
        )
        .relation(
            "Owner",
            "Dog",
            ClassRelationKind::Association,
            Some("1".into()),
            Some("0..*".into()),
            Some("owns".into()),
        )
        .class_style(Style::new().fg(Color::LightCyan))
        .edge_style(Style::new().fg(Color::Gray))
        .padding(1);

    Frame::new()
        .title("ClassDiagram")
        .border(true)
        .padding(1)
        .child(diagram)
        .into()
}

fn er_tab() -> Element {
    let diagram = ErDiagram::new()
        .entities([
            ErEntity::new("CUSTOMER")
                .attribute(ErAttribute::new("int", "id").pk())
                .attribute(ErAttribute::new("string", "name")),
            ErEntity::new("ORDER")
                .attribute(ErAttribute::new("int", "id").pk())
                .attribute(ErAttribute::new("int", "customer_id").fk()),
        ])
        .relation(
            "CUSTOMER",
            "ORDER",
            ErCardinality::ExactlyOne,
            ErCardinality::ZeroOrMore,
            Some("places".into()),
        )
        .entity_style(Style::new().fg(Color::LightYellow))
        .edge_style(Style::new().fg(Color::Gray))
        .padding(1);

    Frame::new()
        .title("ErDiagram")
        .border(true)
        .padding(1)
        .child(diagram)
        .into()
}

fn state_tab() -> Element {
    let diagram = StateDiagram::new()
        .start_to("Idle")
        .state("Idle")
        .state("Processing")
        .choice("Retry?")
        .transition("Idle", "Processing", Some("submit".into()))
        .transition("Processing", "Retry?", Some("done".into()))
        .transition("Retry?", "Processing", Some("retry".into()))
        .end_from("Retry?")
        .state_style(Style::new().fg(Color::LightGreen))
        .edge_style(Style::new().fg(Color::Gray))
        .padding(1);

    Frame::new()
        .title("StateDiagram")
        .border(true)
        .padding(1)
        .child(diagram)
        .into()
}

fn gantt_tab() -> Element {
    let sample_schedule = GanttDiagram::new()
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
                .task(GanttTask::new("Test").id("a3").after("a2").duration_days(2))
                .task(GanttTask::new("Release").id("a4").after("a3").milestone()),
        )
        .max_timeline_width(40)
        .padding(1)
        .width(Length::Flex(1));

    let launch_plan = GanttDiagram::new()
        .title("Product Launch")
        .section(
            GanttSection::new("Discovery")
                .task(
                    GanttTask::new("Research")
                        .id("research")
                        .start_date("2026-06-01")
                        .duration_days(4)
                        .done(),
                )
                .task(
                    GanttTask::new("Design review")
                        .id("design-review")
                        .after("research")
                        .duration_days(2)
                        .done(),
                ),
        )
        .section(
            GanttSection::new("Engineering")
                .task(
                    GanttTask::new("API freeze")
                        .id("api-freeze")
                        .after("design-review")
                        .milestone(),
                )
                .task(
                    GanttTask::new("Feature build")
                        .id("feature-build")
                        .after("api-freeze")
                        .duration_days(7)
                        .active(),
                )
                .task(
                    GanttTask::new("Risk burn-down")
                        .id("risk")
                        .after("feature-build")
                        .duration_days(3)
                        .critical(),
                ),
        )
        .section(
            GanttSection::new("Launch")
                .task(
                    GanttTask::new("QA sign-off")
                        .id("qa")
                        .after("risk")
                        .duration_days(3),
                )
                .task(GanttTask::new("Ship").id("ship").after("qa").milestone()),
        )
        .title_style(Style::new().fg(Color::LightCyan).bold())
        .section_style(Style::new().fg(Color::LightMagenta).bold())
        .axis_style(Style::new().fg(Color::DarkGray))
        .max_timeline_width(48)
        .padding(1)
        .width(Length::Flex(1));

    let compressed_roadmap = GanttDiagram::new()
        .title("Compressed Roadmap")
        .section(
            GanttSection::new("Quarter")
                .task(
                    GanttTask::new("Prototype")
                        .id("prototype")
                        .start_date("2026-07-01")
                        .duration_days(14)
                        .done(),
                )
                .task(
                    GanttTask::new("Private beta")
                        .id("beta")
                        .after("prototype")
                        .duration_days(21)
                        .active(),
                )
                .task(
                    GanttTask::new("Hardening")
                        .id("hardening")
                        .after("beta")
                        .duration_days(18)
                        .critical(),
                )
                .task(
                    GanttTask::new("General availability")
                        .id("ga")
                        .after("hardening")
                        .milestone(),
                ),
        )
        .max_timeline_width(28)
        .padding(1)
        .width(Length::Flex(1));

    ScrollView::new()
        .scrollbar(true)
        .scroll_keys(ScrollKeymap::DEFAULT)
        .show_scroll_indicators(false)
        .padding(1)
        .child(
            VStack::new()
                .gap(1)
                .child(section("Basic dependency chain", sample_schedule))
                .child(section("Multi-section launch with statuses", launch_plan))
                .child(section(
                    "Long range compressed into a narrow timeline",
                    compressed_roadmap,
                )),
        )
        .into()
}

fn section(title: &'static str, child: impl Into<Element>) -> Element {
    Frame::new()
        .title(title)
        .border(true)
        .height(Length::Auto)
        .padding(1)
        .child(child)
        .into()
}

fn describe_event(label: &str, event: Option<&SequenceItemEvent>) -> String {
    match event {
        Some(event) => format!("{label}: {} @ {:?}", event.label, event.path),
        None => format!("{label}: none"),
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Diagram Showcase")
        .mount(DiagramShowcase)
        .run()
}
