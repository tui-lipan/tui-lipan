//! Consolidated markdown examples.
//!
//! Run with: cargo run --example markdown_hub --features markdown

#[cfg(feature = "markdown")]
use tui_lipan::prelude::*;

#[cfg(feature = "markdown")]
const PREVIEW_MD: &str = r#"# Markdown Links

`DocumentView` can expose markdown links through `on_click` events.

- Click [tui-lipan docs](https://docs.rs/tui-lipan)
- Click [project repository](https://github.com/tui-lipan/tui-lipan)
"#;

#[cfg(feature = "markdown")]
const BORDER_STYLES: &[BorderStyle] = &[
    BorderStyle::Plain,
    BorderStyle::Rounded,
    BorderStyle::Double,
    BorderStyle::Thick,
    BorderStyle::LightDoubleDashed,
    BorderStyle::HeavyDoubleDashed,
    BorderStyle::LightTripleDashed,
    BorderStyle::HeavyTripleDashed,
    BorderStyle::LightQuadrupleDashed,
    BorderStyle::HeavyQuadrupleDashed,
];

#[cfg(feature = "markdown")]
const BORDER_STYLE_NAMES: &[&str] = &[
    "plain", "rounded", "double", "thick", "ld-dash", "hd-dash", "lt-dash", "ht-dash", "lq-dash",
    "hq-dash",
];

#[cfg(feature = "markdown")]
struct MarkdownHub;

#[cfg(feature = "markdown")]
struct State {
    active_tab: usize,
    table_wrap: bool,
    width_fill: bool,
    table_padding: u16,
    column_separators: bool,
    outer_frame: bool,
    row_separators: TableRowSeparators,
    border_style_idx: usize,
    docs_visited: bool,
    last_event: String,
}

#[cfg(feature = "markdown")]
#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),
    HyperlinkActivated(HyperlinkEvent),
    MarkdownClicked(DocumentClickEvent),
    ResetVisited,
}

#[cfg(feature = "markdown")]
impl Component for MarkdownHub {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            active_tab: 0,
            table_wrap: true,
            width_fill: true,
            table_padding: 1,
            column_separators: true,
            outer_frame: true,
            row_separators: TableRowSeparators::Header,
            border_style_idx: 0,
            docs_visited: false,
            last_event: "Activate links below (callback-driven link handling).".to_string(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(event) => {
                ctx.state.active_tab = event.index.min(2);
            }
            Msg::HyperlinkActivated(event) => {
                ctx.state.docs_visited = true;
                if let Some(href) = event.href {
                    match tui_lipan::utils::open_url(href.as_ref()) {
                        Ok(()) => {
                            ctx.state.last_event =
                                format!("Opened hyperlink: {} -> {href}", event.label);
                        }
                        Err(err) => {
                            ctx.state.last_event = format!(
                                "Failed to open hyperlink {} -> {href}: {err}",
                                event.label
                            );
                        }
                    }
                } else {
                    ctx.state.last_event = format!("Hyperlink activated: {}", event.label);
                }
            }
            Msg::MarkdownClicked(event) => {
                if let Some(url) = event.link {
                    match tui_lipan::utils::open_url(url.as_ref()) {
                        Ok(()) => {
                            ctx.state.last_event = format!(
                                "Opened markdown link on line {}: {url}",
                                event.source_line + 1
                            );
                        }
                        Err(err) => {
                            ctx.state.last_event = format!(
                                "Failed to open markdown link on line {}: {url} ({err})",
                                event.source_line + 1
                            );
                        }
                    }
                } else {
                    ctx.state.last_event =
                        format!("Document clicked on line {}", event.source_line + 1);
                }
            }
            Msg::ResetVisited => {
                ctx.state.docs_visited = false;
                ctx.state.last_event = "Visited style reset.".to_string();
            }
        }

        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('w') if ctx.state.active_tab == 1 => {
                ctx.state.table_wrap = !ctx.state.table_wrap;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('m') if ctx.state.active_tab == 1 => {
                ctx.state.width_fill = !ctx.state.width_fill;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('p') if ctx.state.active_tab == 1 => {
                ctx.state.table_padding = if ctx.state.table_padding == 0 { 1 } else { 0 };
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('n') if ctx.state.active_tab == 1 => {
                ctx.state.column_separators = !ctx.state.column_separators;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('o') if ctx.state.active_tab == 1 => {
                ctx.state.outer_frame = !ctx.state.outer_frame;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('r') if ctx.state.active_tab == 1 => {
                ctx.state.row_separators = match ctx.state.row_separators {
                    TableRowSeparators::None => TableRowSeparators::Header,
                    TableRowSeparators::Header => TableRowSeparators::All,
                    TableRowSeparators::All => TableRowSeparators::None,
                };
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('b') if ctx.state.active_tab == 1 => {
                ctx.state.border_style_idx = (ctx.state.border_style_idx + 1) % BORDER_STYLES.len();
                return KeyUpdate::handled(Update::full());
            }
            _ => {}
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let title = match ctx.state.active_tab {
            0 => "Markdown Preview",
            1 => "Markdown Table Preview",
            _ => "Hyperlink + Markdown Link",
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Tabs::new()
                    .tab("Preview")
                    .tab("Tables")
                    .tab("Hyperlinks")
                    .active(ctx.state.active_tab)
                    .border(true)
                    .height(Length::Px(3))
                    .on_change(ctx.link().callback(Msg::TabChanged)),
            )
            .child(
                Frame::new()
                    .title(format!("Markdown Hub - {title}"))
                    .border(true)
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(match ctx.state.active_tab {
                        0 => self.preview_panel(),
                        1 => self.table_panel(ctx),
                        _ => self.hyperlink_panel(ctx),
                    }),
            )
            .into()
    }
}

#[cfg(feature = "markdown")]
impl MarkdownHub {
    fn preview_panel(&self) -> Element {
        DocumentView::new(MARKDOWN_PREVIEW)
            .markdown()
            .line_numbers(true)
            .wrap(false)
            .table_wrap(true)
            .table_width_mode(DocumentTableWidthMode::Fill)
            .table_outer_frame(true)
            .table_column_separators(true)
            .table_cell_padding(1)
            .h_scrollbar(true)
            .scrollbar(true)
            .into()
    }

    fn table_panel(&self, ctx: &Context<Self>) -> Element {
        let width_mode = if ctx.state.width_fill {
            DocumentTableWidthMode::Fill
        } else {
            DocumentTableWidthMode::Content
        };

        let status = format!(
            "width {} | wrap {} | pad {} | cols {} | outer {} | rows {} | border {}",
            if ctx.state.width_fill {
                "fill"
            } else {
                "content"
            },
            if ctx.state.table_wrap { "on" } else { "off" },
            ctx.state.table_padding,
            if ctx.state.column_separators {
                "on"
            } else {
                "off"
            },
            if ctx.state.outer_frame { "on" } else { "off" },
            match ctx.state.row_separators {
                TableRowSeparators::None => "none",
                TableRowSeparators::Header => "header",
                TableRowSeparators::All => "all",
            },
            BORDER_STYLE_NAMES[ctx.state.border_style_idx],
        );

        VStack::new()
            .child(
                StatusBar::new()
                    .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
                    .left(Text::new(status))
                    .right(Text::new(
                        "w wrap | m width | p pad | n cols | o outer | r rows | b border | q quit",
                    )),
            )
            .child(
                DocumentView::new(MARKDOWN_TABLE)
                    .markdown()
                    .line_numbers(true)
                    .wrap(false)
                    .table_wrap(ctx.state.table_wrap)
                    .table_width_mode(width_mode)
                    .table_outer_frame(ctx.state.outer_frame)
                    .table_column_separators(ctx.state.column_separators)
                    .table_row_separators(ctx.state.row_separators)
                    .table_border_variant(BORDER_STYLES[ctx.state.border_style_idx])
                    .table_cell_padding(ctx.state.table_padding)
                    .scrollbar(true)
                    .h_scrollbar(true),
            )
            .into()
    }

    fn hyperlink_panel(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .gap(1)
            .child(Text::new("Direct hyperlink widget:"))
            .child(
                HStack::new()
                    .gap(2)
                    .child(
                        Hyperlink::new("Open docs")
                            .href("https://docs.rs/tui-lipan")
                            .visited(ctx.state.docs_visited)
                            .visited_style(Style::new().fg(Color::Magenta).underline())
                            .on_activate(ctx.link().callback(Msg::HyperlinkActivated)),
                    )
                    .child(
                        Hyperlink::new("crates.io")
                            .href("https://crates.io/crates/tui-lipan")
                            .on_activate(tui_lipan::callbacks::open_hyperlink()),
                    )
                    .child(
                        Button::new("Reset visited")
                            .on_click(ctx.link().callback(|_: MouseEvent| Msg::ResetVisited)),
                    ),
            )
            .child(Text::new(format!(
                "Visited: {}",
                if ctx.state.docs_visited { "yes" } else { "no" }
            )))
            .child(Divider::horizontal())
            .child(Text::new("Markdown links inside DocumentView:"))
            .child(
                DocumentView::new(PREVIEW_MD)
                    .markdown()
                    .line_numbers(false)
                    .wrap(true)
                    .height(Length::Flex(1))
                    .on_click(ctx.link().callback(Msg::MarkdownClicked)),
            )
            .into()
    }
}

#[cfg(feature = "markdown")]
fn main() -> Result<()> {
    App::new().title("Markdown Hub").mount(MarkdownHub).run()
}

#[cfg(not(feature = "markdown"))]
fn main() {
    eprintln!("Run with: cargo run --example markdown_hub --features markdown");
}

#[cfg(feature = "markdown")]
const MARKDOWN_PREVIEW: &str = r#"# DocumentView Markdown Preview

Render markdown content as rich TUI blocks:

- headings without `#` markers
- **strong** / *emphasis* / ~~strikethrough~~
- inline `code`
- links like [tui-lipan](https://github.com)

> Blockquotes are rendered with a quote bar.
> They can span multiple lines.

## Table

| Name | Value | Trend |
|:-----|------:|:-----:|
| CPU  | 63%   | up    |
| RAM  | 41%   | flat  |
| IO   | 12%   | down  |

## Code

```rust
fn greet(name: &str) {
    println!("Hello, {name}!");
}
```

---

Scroll with mouse wheel or keyboard (j/k, arrows, PgUp/PgDn, Home/End).
"#;

#[cfg(feature = "markdown")]
const MARKDOWN_TABLE: &str = r#"# Markdown Table Preview

Toggle table rendering options live:

- `w`: table wrap
- `m`: width mode (content/fill)
- `p`: table cell padding (0/1)
- `n`: column separators
- `o`: outer frame
- `r`: row borders (between data rows)
- `b`: cycle border style

## Build Matrix

| Service | Region | Build | Queue | Duration | Success | Artifact | Owner |
|:--------|:-------|------:|------:|---------:|--------:|:---------|:------|
| api-gateway | eu-central-1 | 2481 | 14 | 00:04:32 | 99.7% | gateway-linux-amd64.tar.gz | team-platform |
| api-gateway | us-east-1 | 2482 | 22 | 00:04:11 | 99.8% | gateway-linux-arm64.tar.gz | team-platform |
| worker-payments | eu-west-1 | 1754 | 9 | 00:12:09 | 98.9% | payments-worker.tar.gz | team-billing |
| worker-notify | ap-southeast-1 | 991 | 3 | 00:02:52 | 99.9% | notify-worker.tar.gz | team-comms |
| analytics-batch | us-west-2 | 644 | 17 | 00:23:48 | 97.4% | analytics-batch.tar.gz | team-data |
| web-frontend | eu-central-1 | 3320 | 31 | 00:06:03 | 99.5% | web-assets-2026-02-26.zip | team-web |
| web-frontend | us-east-1 | 3321 | 8 | 00:05:44 | 99.6% | web-assets-2026-02-26.zip | team-web |
| auth-service | sa-east-1 | 1205 | 11 | 00:07:41 | 99.2% | auth-service-bundle.tar.gz | team-security |
| image-pipeline | eu-north-1 | 441 | 5 | 00:18:20 | 96.8% | image-pipeline.tar.gz | team-media |
| log-ingest | ca-central-1 | 872 | 6 | 00:03:47 | 99.3% | ingest-agent.tar.gz | team-observability |

## Notes

Drag across table cells to copy rectangular TSV selections.
"#;
