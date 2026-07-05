//! Design-first sketch for a terminal HTTP / GraphQL client.
//!
//! This intentionally uses `Mockup` instead of a full `Component`: the goal is
//! to settle the screen composition first, then promote the stable view into a
//! stateful Elm-style app with request commands.
//!
//! Run the sketch loop:
//!
//! ```bash
//! cargo run --example network_client_sketch --features ui-snapshot-png
//! ```
//!
//! PNG output uses the default font-backed renderer so screenshots are suitable
//! for visual review rather than only coarse layout debugging.

use std::fs;
use std::path::{Path, PathBuf};

use tui_lipan::prelude::*;
use tui_lipan::{TestBackend, UiSnapshotOptions};

#[cfg(feature = "ui-snapshot-png")]
use tui_lipan::PngOptions;

#[derive(Clone, Copy)]
enum SketchScenario {
    RestHappy,
    GraphqlLoading,
    OverflowError,
}

impl SketchScenario {
    fn slug(self) -> &'static str {
        match self {
            Self::RestHappy => "rest-happy",
            Self::GraphqlLoading => "graphql-loading",
            Self::OverflowError => "overflow-error",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RestHappy => "REST 200 OK",
            Self::GraphqlLoading => "GraphQL loading",
            Self::OverflowError => "REST 422 validation error",
        }
    }

    fn method(self) -> &'static str {
        match self {
            Self::RestHappy | Self::OverflowError => "POST",
            Self::GraphqlLoading => "GRAPHQL",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::RestHappy => "https://api.acme.dev/v1/invoices/search",
            Self::GraphqlLoading => "https://api.acme.dev/graphql",
            Self::OverflowError => {
                "https://staging.acme.dev/v1/customers/very-long-enterprise-account-id-9d7f/contacts"
            }
        }
    }

    fn status(self) -> ResponseStatus {
        match self {
            Self::RestHappy => ResponseStatus::Ok,
            Self::GraphqlLoading => ResponseStatus::Loading,
            Self::OverflowError => ResponseStatus::Error,
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::RestHappy => REST_BODY,
            Self::GraphqlLoading => GRAPHQL_BODY,
            Self::OverflowError => OVERFLOW_BODY,
        }
    }

    fn response(self) -> &'static str {
        match self {
            Self::RestHappy => REST_RESPONSE,
            Self::GraphqlLoading => GRAPHQL_RESPONSE,
            Self::OverflowError => ERROR_RESPONSE,
        }
    }
}

#[derive(Clone, Copy)]
enum ResponseStatus {
    Ok,
    Loading,
    Error,
}

fn main() -> Result<()> {
    let out_dir = export_dir();
    fs::create_dir_all(&out_dir)?;

    for scenario in [
        SketchScenario::RestHappy,
        SketchScenario::GraphqlLoading,
        SketchScenario::OverflowError,
    ] {
        render_scenario(&out_dir, scenario)?;
    }

    println!(
        "Wrote network client sketch artifacts to {}",
        out_dir.display()
    );

    #[cfg(not(feature = "ui-snapshot-png"))]
    println!("Enable `--features ui-snapshot-png` to also write PNG renderings.");

    Ok(())
}

fn export_dir() -> PathBuf {
    std::env::temp_dir().join("tui-lipan-network-client-sketch")
}

fn render_scenario(out_dir: &Path, scenario: SketchScenario) -> Result<()> {
    let mut backend = TestBackend::new(Mockup::new(move || network_client_view(scenario)));

    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 118,
        h: 34,
    });
    backend.render();

    // Give the snapshot a visible focus state instead of judging an idle tree.
    backend.focus_next();
    backend.focus_next();
    backend.render();

    let snapshot = backend.capture_ui_snapshot_with_margin(20, 8, &UiSnapshotOptions::default());
    let markdown_path = out_dir.join(format!("{}.md", scenario.slug()));
    fs::write(&markdown_path, snapshot.to_markdown())?;
    println!("Wrote {}", markdown_path.display());

    #[cfg(feature = "ui-snapshot-png")]
    {
        let png_options = PngOptions::default();

        let tight_path = out_dir.join(format!("{}-tight.png", scenario.slug()));
        let tight = backend.capture_frame_with_margin(0, 0).to_png(&png_options);
        fs::write(&tight_path, tight)?;
        println!("Wrote {}", tight_path.display());

        let roomy_path = out_dir.join(format!("{}-roomy.png", scenario.slug()));
        let roomy = backend
            .capture_frame_with_margin(20, 8)
            .to_png(&png_options);
        fs::write(&roomy_path, roomy)?;
        println!("Wrote {}", roomy_path.display());

        let real_path = out_dir.join(format!("{}-118x34.png", scenario.slug()));
        let real = backend.capture_frame().to_png(&png_options);
        fs::write(&real_path, real)?;
        println!("Wrote {}", real_path.display());
    }

    Ok(())
}

fn network_client_view(scenario: SketchScenario) -> Element {
    VStack::new()
        .style(app_bg())
        .gap(1)
        .padding(1)
        .child(top_bar(scenario))
        .child(
            HStack::new()
                .gap(1)
                .child(collections_panel(scenario))
                .child(
                    VStack::new().gap(1).child(request_panel(scenario)).child(
                        HStack::new()
                            .gap(1)
                            .child(body_panel(scenario))
                            .child(response_panel(scenario)),
                    ),
                ),
        )
        .child(status_bar(scenario))
        .into()
}

fn top_bar(scenario: SketchScenario) -> Element {
    HStack::new()
        .height(Length::Px(3))
        .align(Align::Center)
        .gap(1)
        .child(
            Text::new("API Lab")
                .style(Style::new().fg(accent()).bold())
                .width(Length::Px(14)),
        )
        .child(
            Text::new("Terminal HTTP + GraphQL client sketch")
                .style(Style::new().fg(Color::indexed(250)))
                .overflow(Overflow::Ellipsis),
        )
        .child(
            Text::new(scenario.label())
                .style(status_style(scenario.status()).bold())
                .width(Length::Px(30)),
        )
        .into()
}

fn collections_panel(scenario: SketchScenario) -> Element {
    Frame::new()
        .title("Collections")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .width(Length::Px(28))
        .style(surface())
        .title_style(Style::new().fg(accent()).bold())
        .status("Tab | Enter")
        .status_style(muted())
        .child(
            VStack::new()
                .gap(1)
                .child(
                    Input::new("acme billing")
                        .placeholder("Filter collections")
                        .prefix("/")
                        .border_style(BorderStyle::Rounded)
                        .style(input_style())
                        .focus_style(focus_surface())
                        .key("collection-filter"),
                )
                .child(
                    List::new()
                        .items(collection_items())
                        .selected(match scenario {
                            SketchScenario::RestHappy => 3,
                            SketchScenario::GraphqlLoading => 8,
                            SketchScenario::OverflowError => 5,
                        })
                        .border(false)
                        .scrollbar(true)
                        .show_scroll_indicators(true)
                        .selection_full_width(true)
                        .selection_style(Style::new().bg(Color::indexed(24)).fg(Color::White))
                        .selection_symbol(Some("> "))
                        .selection_symbol_style(Style::new().fg(accent()).bold())
                        .unselected_symbol(Some("  "))
                        .key("collections-list"),
                ),
        )
        .into()
}

fn collection_items() -> Vec<ListItem> {
    vec![
        ListItem::header("Pinned"),
        request_item("GET", "/health", "18 ms", Color::Green),
        request_item("POST", "/invoices", "201", Color::Cyan),
        request_item("POST", "/inv/search", "200", accent()),
        ListItem::spacer(),
        ListItem::header("Customers"),
        request_item("POST", "/contacts/{id}", "422", Color::Red),
        request_item("PATCH", "/customers/{id}", "82 ms", Color::Yellow),
        ListItem::spacer(),
        ListItem::header("GraphQL"),
        request_item("GQL", "CustOverview", "loading", Color::Magenta),
        request_item("GQL", "InvTimeline", "cached", Color::LightBlue),
    ]
}

fn request_item(method: &str, path: &str, meta: &str, color: Color) -> ListItem {
    ListItem::new(path)
        .gutter(ListItemGutter::text(method))
        .style(Style::new().fg(Color::indexed(252)))
        .description(meta)
        .description_style(Style::new().fg(color))
}

fn request_panel(scenario: SketchScenario) -> Element {
    Frame::new()
        .title("Request")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .height(Length::Px(10))
        .style(surface())
        .title_style(Style::new().fg(Color::rgb(125, 211, 252)).bold())
        .child(
            VStack::new()
                .gap(1)
                .child(
                    HStack::new()
                        .gap(1)
                        .height(Length::Px(3))
                        .child(method_button(
                            "GET",
                            scenario.method() == "GET",
                            Color::Green,
                        ))
                        .child(method_button("POST", scenario.method() == "POST", accent()))
                        .child(method_button(
                            "GQL",
                            scenario.method() == "GRAPHQL",
                            Color::Magenta,
                        ))
                        .child(
                            Input::new(scenario.url())
                                .prefix("URL ")
                                .border_style(BorderStyle::Rounded)
                                .style(input_style())
                                .focus_style(focus_surface())
                                .focus_content_style(Style::new().fg(Color::White))
                                .key("url-input"),
                        )
                        .child(
                            Button::filled(
                                if matches!(scenario.status(), ResponseStatus::Loading) {
                                    "Wait"
                                } else {
                                    "Send"
                                },
                            )
                            .icon(">")
                            .style(Style::new().bg(accent()).fg(Color::Black).bold())
                            .focus_style(Style::new().bg(Color::White).fg(Color::Black).bold())
                            .width(Length::Px(12))
                            .key("send-button"),
                        ),
                )
                .child(
                    Tabs::new()
                        .tab("Params")
                        .tab("Headers")
                        .tab("Body")
                        .tab("Auth")
                        .active(2)
                        .height(Length::Px(1))
                        .active_style(Style::new().fg(Color::Black).bg(accent()).bold())
                        .style(muted())
                        .divider(' ')
                        .key("request-tabs"),
                )
                .child(header_chips()),
        )
        .into()
}

fn method_button(label: &str, selected: bool, color: Color) -> Element {
    let base = if selected {
        Button::filled(label).style(Style::new().bg(color).fg(Color::Black).bold())
    } else {
        Button::outlined(label).style(Style::new().fg(color))
    };

    base.width(Length::Px(8))
        .border_style(BorderStyle::Rounded)
        .key(format!("method-{label}"))
}

fn header_chips() -> Element {
    HStack::new()
        .gap(1)
        .height(Length::Px(2))
        .child(chip("Authorization", "Bearer ***", Color::Yellow))
        .child(chip("Content-Type", "application/json", Color::Cyan))
        .child(chip("X-Trace", "ui-sketch", Color::Magenta))
        .into()
}

fn chip(label: &'static str, value: &'static str, color: Color) -> Element {
    Frame::new()
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(250)))
        .title(label)
        .title_style(Style::new().fg(color).bold())
        .height(Length::Px(2))
        .child(Text::new(value).style(muted()).overflow(Overflow::Ellipsis))
        .into()
}

fn body_panel(scenario: SketchScenario) -> Element {
    Frame::new()
        .tab_titles(["Body", "Headers", "Variables"])
        .active_tab(if matches!(scenario, SketchScenario::GraphqlLoading) {
            2
        } else {
            0
        })
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(surface())
        .active_tab_style(Style::new().fg(Color::Black).bg(accent()).bold())
        .inactive_tab_style(muted())
        .status("edit body | focusable TextArea")
        .status_style(muted())
        .child(
            TextArea::new(scenario.body())
                .line_numbers(true)
                .language("json")
                .border(false)
                .padding((0, 1))
                .style(editor_style())
                .line_number_style(Style::new().fg(Color::indexed(244)))
                .focus_style(focus_surface())
                .scrollbar(true)
                .key("request-body"),
        )
        .into()
}

fn response_panel(scenario: SketchScenario) -> Element {
    Frame::new()
        .title("Response")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(surface())
        .title_style(status_style(scenario.status()).bold())
        .status(response_status_line(scenario.status()))
        .status_style(status_style(scenario.status()))
        .child(
            VStack::new()
                .gap(1)
                .child(response_metrics(scenario.status()))
                .child(
                    TextArea::new(scenario.response())
                        .line_numbers(true)
                        .language("json")
                        .read_only(true)
                        .border(true)
                        .border_style(BorderStyle::Rounded)
                        .padding((0, 1))
                        .style(editor_style())
                        .line_number_style(Style::new().fg(Color::indexed(244)))
                        .focus_style(focus_surface())
                        .scrollbar(true)
                        .key("response-body"),
                ),
        )
        .into()
}

fn response_metrics(status: ResponseStatus) -> Element {
    HStack::new()
        .gap(1)
        .height(Length::Px(3))
        .child(metric("Status", status_label(status), status_style(status)))
        .child(metric(
            "Time",
            match status {
                ResponseStatus::Ok => "86 ms",
                ResponseStatus::Loading => "pending",
                ResponseStatus::Error => "142 ms",
            },
            Style::new().fg(Color::rgb(125, 211, 252)),
        ))
        .child(metric(
            "Size",
            match status {
                ResponseStatus::Ok => "9.8 KB",
                ResponseStatus::Loading => "--",
                ResponseStatus::Error => "1.4 KB",
            },
            Style::new().fg(Color::rgb(167, 139, 250)),
        ))
        .into()
}

fn metric(label: &'static str, value: &'static str, value_style: Style) -> Element {
    Frame::new()
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(250)))
        .title(label)
        .title_style(muted())
        .child(Text::new(value).style(value_style.bold()))
        .into()
}

fn status_bar(scenario: SketchScenario) -> Element {
    StatusBar::new()
        .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
        .left(Text::new(" DESIGN SKETCH ").style(Style::new().bg(accent()).fg(Color::Black).bold()))
        .center(Text::new(format!("{} | sketch", scenario.label())))
        .right(Text::new("PNG: tight / roomy / 118x34").style(muted()))
        .into()
}

fn status_label(status: ResponseStatus) -> &'static str {
    match status {
        ResponseStatus::Ok => "200 OK",
        ResponseStatus::Loading => "Loading",
        ResponseStatus::Error => "422 ERR",
    }
}

fn response_status_line(status: ResponseStatus) -> &'static str {
    match status {
        ResponseStatus::Ok => "HTTP/2 | 14 hdr",
        ResponseStatus::Loading => "await ResultMsg",
        ResponseStatus::Error => "422 | no retry",
    }
}

fn status_style(status: ResponseStatus) -> Style {
    match status {
        ResponseStatus::Ok => Style::new().fg(Color::Green),
        ResponseStatus::Loading => Style::new().fg(Color::Yellow),
        ResponseStatus::Error => Style::new().fg(Color::Red),
    }
}

fn app_bg() -> Style {
    Style::new().bg(Color::indexed(232)).fg(Color::indexed(252))
}

fn surface() -> Style {
    Style::new().bg(Color::indexed(235)).fg(Color::indexed(252))
}

fn input_style() -> Style {
    Style::new().bg(Color::indexed(234)).fg(Color::indexed(253))
}

fn editor_style() -> Style {
    Style::new().bg(Color::indexed(234)).fg(Color::indexed(253))
}

fn focus_surface() -> Style {
    Style::new().bg(Color::indexed(236)).fg(Color::White)
}

fn muted() -> Style {
    Style::new().fg(Color::indexed(245))
}

fn accent() -> Color {
    Color::rgb(45, 212, 191)
}

const REST_BODY: &str = r#"{
  "customer_id": "cus_live_19283",
  "include": ["customer", "payments", "line_items"],
  "filters": {
    "created_after": "2026-05-01T00:00:00Z",
    "status": ["open", "paid"]
  }
}"#;

const GRAPHQL_BODY: &str = r#"query CustomerOverview($id: ID!, $window: DateRange!) {
  customer(id: $id) {
    id
    name
    invoices(window: $window) {
      totalCount
      nodes { id status total dueDate }
    }
  }
}

variables:
{
  "id": "cus_enterprise_42",
  "window": { "from": "2026-04-01", "to": "2026-05-24" }
}"#;

const OVERFLOW_BODY: &str = r#"{
  "contacts": [
    {
      "name": "A Very Long Corporate Contact Name That Must Truncate Cleanly",
      "email": "alexandria.maximum-length-contact@subsidiary.parent-corporation.example",
      "roles": ["billing_admin", "security_reviewer", "legal_approver"]
    }
  ],
  "notify": true
}"#;

const REST_RESPONSE: &str = r#"{
  "data": [
    { "id": "inv_001", "status": "paid", "total": 12400, "currency": "USD" },
    { "id": "inv_002", "status": "open", "total": 9800, "currency": "USD" }
  ],
  "meta": {
    "count": 2,
    "trace_id": "req_ui_sketch_6f22"
  }
}"#;

const GRAPHQL_RESPONSE: &str = r#"{
  "state": "loading",
  "command": "send_graphql_request",
  "request_id": "cmd_7f3d",
  "message": "Waiting for ResultMsg(Ok|Err)"
}"#;

const ERROR_RESPONSE: &str = r#"{
  "error": {
    "code": "validation_failed",
    "message": "One or more contacts failed validation.",
    "fields": {
      "contacts[0].email": ["domain is not allowed for this workspace"],
      "contacts[0].roles": ["legal_approver requires enterprise plan"]
    }
  },
  "trace_id": "req_validation_9352"
}"#;
