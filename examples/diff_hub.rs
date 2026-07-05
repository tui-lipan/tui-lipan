use std::sync::Arc;

use tui_lipan::DiffPalette;
use tui_lipan::prelude::*;

const BEFORE: &str = r#"fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}

fn goodbye(name: &str) -> String {
    format!("Goodbye, {}", name)
}

fn main() {
    let msg = greet("World");
    println!("{}", msg);

    let bye = goodbye("World");
    println!("{}", bye);

    println!("End of program.");
}
"#;

const AFTER: &str = r#"fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn main() {
    let user = "Lipan";
    let msg = greet(user);
    println!("{msg}");

    let items = vec!["Rust", "TUI", "Lipan"];
    for item in items {
        println!("Loading {item}...");
    }

    println!("Application ready.");
}
"#;

const CLICKABLE_BEFORE: &str = r#"//! Large diff for clickable context-separator demos.

use std::collections::HashMap;

const MAX_RETRIES: u32 = 3;
const TIMEOUT_MS: u64 = 5_000;

struct Config {
    host: String,
    port: u16,
}

impl Config {
    fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }
}

fn validate_host(host: &str) -> bool {
    !host.is_empty() && host.len() < 256
}

fn validate_port(port: u16) -> bool {
    port > 0
}

fn build_url(config: &Config) -> String {
    format!("http://{}:{}", config.host, config.port)
}

fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}

fn log_info(message: &str) {
    println!("[info] {}", message);
}

fn log_warn(message: &str) {
    println!("[warn] {}", message);
}

fn cache_key(namespace: &str, id: &str) -> String {
    format!("{namespace}:{id}")
}

fn record_latency(operation: &str, millis: u64) {
    println!("[metric] {operation} took {millis}ms");
}

fn record_counter(name: &str, value: u64) {
    println!("[counter] {name}={value}");
}

fn flush_metrics() {
    println!("[metric] flush complete");
}

fn health_check() -> bool {
    true
}

fn connect(config: &Config) -> bool {
    validate_host(&config.host) && validate_port(config.port)
}

fn main() {
    let config = Config::new("localhost", 8080);
    let msg = greet("World");
    log_info(&msg);
    if connect(&config) {
        log_info(&build_url(&config));
    }
    record_latency("startup", 12);
    record_counter("requests", 0);
    flush_metrics();
    log_warn("shutdown");
}
"#;

const CLICKABLE_AFTER: &str = r#"//! Large diff for clickable context-separator demos.

use std::collections::HashMap;

const MAX_RETRIES: u32 = 5;
const TIMEOUT_MS: u64 = 10_000;

struct Config {
    host: String,
    port: u16,
    tls: bool,
}

impl Config {
    fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            tls: false,
        }
    }
}

fn validate_host(host: &str) -> bool {
    !host.is_empty() && host.len() < 256
}

fn validate_port(port: u16) -> bool {
    port > 0 && port < 65535
}

fn build_url(config: &Config) -> String {
    let scheme = if config.tls { "https" } else { "http" };
    format!("{scheme}://{}:{}", config.host, config.port)
}

fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn log_info(message: &str) {
    println!("[info] {}", message);
}

fn log_warn(message: &str) {
    println!("[warn] {}", message);
}

fn cache_key(namespace: &str, id: &str) -> String {
    format!("{namespace}:{id}")
}

fn record_latency(operation: &str, millis: u64) {
    println!("[metric] {operation} took {millis}ms");
}

fn record_counter(name: &str, value: u64) {
    println!("[counter] {name}={value}");
}

fn flush_metrics() {
    println!("[metric] flush complete");
}

fn health_check() -> bool {
    health_check_core() && health_check_disk()
}

fn health_check_core() -> bool {
    true
}

fn health_check_disk() -> bool {
    true
}

fn retry<F: FnMut() -> bool>(mut attempt: F) -> bool {
    for _ in 0..MAX_RETRIES {
        if attempt() {
            return true;
        }
    }
    false
}

fn connect(config: &Config) -> bool {
    retry(|| validate_host(&config.host) && validate_port(config.port))
}

fn main() {
    let config = Config::new("lipan.local", 4430);
    let msg = greet("Lipan");
    log_info(&msg);
    if connect(&config) {
        log_info(&build_url(&config));
    }
    record_latency("startup", 4);
    record_counter("requests", 1);
    flush_metrics();
    log_warn("ready");
}
"#;

const PATCH: &str = r#"Index: /home/user/Work/Projects/opencode-tui/src/bin/mockup.rs
===================================================================
--- /home/user/Work/Projects/opencode-tui/src/bin/mockup.rs
+++ /home/user/Work/Projects/opencode-tui/src/bin/mockup.rs
@@ -92,8 +92,16 @@
     DialogPromptStash,
     DialogTimeline,
     DialogMessageDetails,
     DialogPromptInput,
+    DialogExportOptions,
+    DialogMessageActions,
+    DialogSkillPicker,
+    DialogSubagent,
+    DialogProviderMethodSelect,
+    DialogProviderAuthApi,
+    DialogProviderAuthCode,
+    DialogProviderAuthAuto,
 }

 impl Screen {
     fn all() -> &'static [Screen] {"#;

struct DiffHub;

#[derive(Default)]
struct State {
    active_tab: usize,
    text_split_scroll: Option<usize>,
    doc_split_scroll: Option<usize>,
    expanded_ranges: Vec<DiffContextExpansion>,
    context_expand_lines: usize,
}

#[derive(Clone)]
enum Msg {
    TabChanged(TabsEvent),
    TextSplitScrolled(DiffScrollEvent),
    DocSplitScrolled(DiffScrollEvent),
    ContextSeparatorClicked(DiffContextSeparatorEvent),
    ResetContexts,
}

impl Component for DiffHub {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            context_expand_lines: 5,
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(event) => {
                ctx.state.active_tab = event.index.min(6);
            }
            Msg::TextSplitScrolled(event) => {
                if matches!(event.pane, DiffPane::Left | DiffPane::Right) {
                    ctx.state.text_split_scroll = Some(event.scroll.offset);
                }
            }
            Msg::DocSplitScrolled(event) => {
                if matches!(event.pane, DiffPane::Left | DiffPane::Right) {
                    ctx.state.doc_split_scroll = Some(event.scroll.offset);
                }
            }
            Msg::ContextSeparatorClicked(event) => {
                let current = ctx
                    .state
                    .expanded_ranges
                    .iter()
                    .find(|expansion| expansion.range == event.range);
                let next = event.next_expansion(current);
                if let Some(index) = ctx
                    .state
                    .expanded_ranges
                    .iter()
                    .position(|expansion| expansion.range == event.range)
                {
                    ctx.state.expanded_ranges[index] = next;
                } else {
                    ctx.state.expanded_ranges.push(next);
                }
            }
            Msg::ResetContexts => {
                ctx.state.expanded_ranges.clear();
            }
        }

        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let config = DiffDataConfig::default();
        let diff_data = Arc::new(DiffData::with_config(BEFORE, AFTER, config));

        let base_area = TextArea::new("")
            .line_numbers(true)
            .min_line_number_width(3)
            .wrap(false)
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Standalone))
            .h_scrollbar(true);

        let base_doc = DocumentView::new("")
            .line_numbers(true)
            .wrap(false)
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Standalone))
            .h_scrollbar(true);

        let unified_style = DiffPalette {
            added: Style::new()
                .bg(Color::rgb(18, 52, 34))
                .fg(Color::LightGreen),
            removed: Style::new().bg(Color::rgb(52, 24, 24)).fg(Color::LightRed),
            added_marker: Style::new().fg(Color::Green),
            removed_marker: Style::new().fg(Color::Red),
            ..DiffPalette::default()
        };

        let (title, view): (&str, Element) = match ctx.state.active_tab {
            0 => {
                let mut widget = DiffView::with_content(BEFORE, AFTER)
                    .with_shared_diff(diff_data.clone())
                    .mode(DiffViewMode::Split)
                    .word_diff(true)
                    .gutter_inset(1)
                    .border(false)
                    .panels_border(false)
                    .highlight_full_width(true)
                    .single_scrollbar(true)
                    .on_scroll(ctx.link().callback(Msg::TextSplitScrolled));

                if let Some(offset) = ctx.state.text_split_scroll {
                    widget = widget.scroll_offset(offset);
                }

                #[cfg(feature = "syntax-syntect")]
                {
                    widget = widget.with_syntax("rust", "base16-ocean.dark");
                }

                ("Split (TextArea backend, scroll synced)", widget.into())
            }
            1 => {
                let mut widget = DiffView::with_content(BEFORE, AFTER)
                    .mode(DiffViewMode::Split)
                    .word_diff(true)
                    .border(false)
                    .panels_border(true)
                    .gutter_inset(1)
                    .join_frame(false)
                    .single_scrollbar(true)
                    .highlight_full_width(true)
                    .context_lines(3)
                    .context_separator_text("{arrow} {count} {line_word} omitted {direction}")
                    .document_view(base_doc.clone())
                    .on_scroll(ctx.link().callback(Msg::DocSplitScrolled));

                if let Some(offset) = ctx.state.doc_split_scroll {
                    widget = widget.scroll_offset(offset);
                }

                #[cfg(feature = "syntax-syntect")]
                {
                    widget = widget.with_syntax("rust", "base16-ocean.dark");
                }

                (
                    "Split (DocumentView backend, context_lines=3)",
                    widget.into(),
                )
            }
            2 => {
                let widget = DiffView::with_content(BEFORE, AFTER)
                    .with_shared_diff(diff_data)
                    .mode(DiffViewMode::Unified)
                    .editable(true)
                    .diff_style(unified_style)
                    .show_prefixes(true)
                    .word_diff(true)
                    .highlight_full_width(true)
                    .text_area(base_area);

                #[cfg(feature = "syntax-syntect")]
                let widget = widget.with_syntax("rust", "base16-ocean.dark");

                ("Unified (editable TextArea backend)", widget.into())
            }
            3 => {
                let widget = DiffView::with_content(BEFORE, AFTER)
                    .mode(DiffViewMode::Unified)
                    .word_diff(true)
                    .highlight_full_width(true)
                    .context_lines(3)
                    .context_separator_text("{arrow} {count} {line_word} omitted {direction}")
                    .document_view(base_doc);

                #[cfg(feature = "syntax-syntect")]
                let widget = widget.with_syntax("rust", "base16-ocean.dark");

                (
                    "Unified (DocumentView backend, context_lines=3)",
                    widget.into(),
                )
            }
            4 => {
                let widget = DiffView::with_content(CLICKABLE_BEFORE, CLICKABLE_AFTER)
                    .mode(DiffViewMode::Unified)
                    .word_diff(true)
                    .highlight_full_width(true)
                    .context_lines(1)
                    .context_expand_lines(ctx.state.context_expand_lines)
                    .context_separator_text(
                        "{arrow} Click to expand {count} hidden {line_word} {direction} {arrow}",
                    )
                    .context_separator_hover_style(
                        Style::new().bg(Color::rgb(35, 45, 60)).underline(),
                    )
                    .expanded_context_expansions(ctx.state.expanded_ranges.clone())
                    .on_context_separator_click(ctx.link().callback(Msg::ContextSeparatorClicked));

                #[cfg(feature = "syntax-syntect")]
                let widget = widget.with_syntax("rust", "base16-ocean.dark");

                let controls = HStack::new()
                    .height(Length::Auto)
                    .align(Align::Center)
                    .gap(2)
                    .child(Text::new(
                        "Interactive Demo: Click separators to reveal 5 lines at a time (scroll stays anchored).",
                    ))
                    .child(
                        Button::new("Reset Contexts")
                            .on_click(ctx.link().callback(|_| Msg::ResetContexts)),
                    );

                let content = VStack::new()
                    .gap(1)
                    .child(controls)
                    .child(widget.height(Length::Flex(1)));

                ("Clickable Context Separators (Interactive)", content.into())
            }
            5 => (
                "Patch input - Unified mode",
                DiffView::new()
                    .patch(PATCH)
                    .mode(DiffViewMode::Unified)
                    .into(),
            ),
            _ => (
                "Patch input - Split mode",
                DiffView::new()
                    .patch(PATCH)
                    .mode(DiffViewMode::Split)
                    .into(),
            ),
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Tabs::new()
                    .tab("Split/TextArea")
                    .tab("Split/DocumentView")
                    .tab("Unified Editable")
                    .tab("Unified Context")
                    .tab("Clickable Context")
                    .tab("Patch Unified")
                    .tab("Patch Split")
                    .active(ctx.state.active_tab)
                    .border(true)
                    .height(Length::Px(3))
                    .on_change(ctx.link().callback(Msg::TabChanged)),
            )
            .child(
                Frame::new()
                    .title(format!("Diff Hub - {title}"))
                    .status("Switch variants with tabs")
                    .border(true)
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(view),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Diff Hub")
        .mount(DiffHub)
        .run()
}
