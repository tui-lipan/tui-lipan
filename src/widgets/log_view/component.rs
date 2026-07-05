use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};

use super::matching::{LogSearchResult, match_logs};
use super::{LogViewEvent, LogViewProps};
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::style::{Span, Style, StyleSlot};
use crate::widgets::{List, ListEvent, ListItem};

pub struct LogViewState {
    results: Vec<LogSearchResult>,
    query_id: u64,
    last_query: String,
    last_mode: super::LogFilterMode,
    last_case_sensitive: bool,
    // Cache for rendered items - using Arc to avoid clone on every render
    cached_items: Arc<[ListItem]>,
    cached_entries_id: usize,
    cached_show_level: bool,
}

impl Default for LogViewState {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            query_id: 0,
            last_query: String::new(),
            last_mode: super::LogFilterMode::Fuzzy,
            last_case_sensitive: true,
            cached_items: Arc::new([]),
            cached_entries_id: 0,
            cached_show_level: true,
        }
    }
}

#[derive(Clone, Debug)]
pub enum LogViewMsg {
    ResultsReady {
        query_id: u64,
        results: Vec<LogSearchResult>,
    },
    Selected(ListEvent),
    Activated(ListEvent),
}

pub struct LogViewComponent;

impl LogViewComponent {
    pub fn new() -> Self {
        Self
    }
}

impl Component for LogViewComponent {
    type Message = LogViewMsg;
    type Properties = LogViewProps;
    type State = LogViewState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Self::State::default()
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
        let query = ctx.props.filter.as_deref().unwrap_or("").to_string();
        ctx.state.last_query = query;
        ctx.state.last_mode = ctx.props.filter_mode;
        ctx.state.last_case_sensitive = ctx.props.case_sensitive;
        ctx.state.query_id += 1;
        Some(spawn_filter(ctx.state.query_id, ctx))
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            LogViewMsg::ResultsReady { query_id, results } => {
                if query_id != ctx.state.query_id {
                    return Update::none();
                }
                ctx.state.results = results;
                // Rebuild cache since results changed
                rebuild_cache(ctx);
                Update::full()
            }
            LogViewMsg::Selected(event) => {
                if let Some(on_select) = &ctx.props.on_select
                    && let Some(result) = ctx.state.results.get(event.index)
                    && let Some(entry) = ctx.props.entries.get(result.source_index)
                {
                    on_select.emit(LogViewEvent {
                        visible_index: event.index,
                        source_index: result.source_index,
                        entry: entry.clone(),
                    });
                }
                Update::none()
            }
            LogViewMsg::Activated(event) => {
                if let Some(on_activate) = &ctx.props.on_activate
                    && let Some(result) = ctx.state.results.get(event.index)
                    && let Some(entry) = ctx.props.entries.get(result.source_index)
                {
                    on_activate.emit(LogViewEvent {
                        visible_index: event.index,
                        source_index: result.source_index,
                        entry: entry.clone(),
                    });
                }
                Update::none()
            }
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let query = ctx.props.filter.as_deref().unwrap_or("").to_string();
        let entries_changed = !Arc::ptr_eq(&old_props.entries, &ctx.props.entries);
        let filter_changed = query != ctx.state.last_query
            || ctx.props.filter_mode != ctx.state.last_mode
            || ctx.props.case_sensitive != ctx.state.last_case_sensitive;
        if filter_changed {
            // Filter parameters changed - run async so large entry sets don't
            // block the UI thread while the user is typing.
            ctx.state.last_query = query;
            ctx.state.last_mode = ctx.props.filter_mode;
            ctx.state.last_case_sensitive = ctx.props.case_sensitive;
            ctx.state.query_id += 1;
            return Update::with_command(spawn_filter(ctx.state.query_id, ctx));
        }
        if entries_changed {
            // Only entries changed (append) - refilter inline to avoid a
            // two-render flicker from the async round-trip.
            refilter_inline(ctx);
            rebuild_cache(ctx);
            return Update::full();
        }
        // Check if we need to rebuild cache due to show_level change
        let entries_id = ctx.props.entries.as_ptr() as usize;
        if ctx.state.cached_entries_id != entries_id
            || ctx.state.cached_show_level != ctx.props.show_level
        {
            rebuild_cache(ctx);
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let following_live = ctx.props.auto_follow && !ctx.props.paused;
        let selected = if following_live {
            ctx.state.cached_items.len().saturating_sub(1)
        } else {
            ctx.props
                .selected
                .min(ctx.state.cached_items.len().saturating_sub(1))
        };
        let selection_style = if following_live {
            StyleSlot::Replace(Style::default())
        } else {
            ctx.props.selection_style
        };
        let unfocused_selection_style = if following_live {
            StyleSlot::Replace(Style::default())
        } else {
            ctx.props.unfocused_selection_style
        };

        let mut list = List::new()
            .items_arc(ctx.state.cached_items.clone())
            .selected(selected)
            .force_scroll_to_selected(following_live)
            .style(ctx.props.style)
            .hover_style_slot(ctx.props.hover_style)
            .item_hover_style_slot(ctx.props.item_hover_style)
            .border(ctx.props.border)
            .border_style(ctx.props.border_style)
            .padding(ctx.props.padding)
            .scrollbar(ctx.props.scrollbar)
            .scrollbar_config(ctx.props.scrollbar_config.clone())
            .show_scroll_indicators(ctx.props.show_scroll_indicators)
            .scroll_indicator_style(ctx.props.scroll_indicator_style)
            .width(ctx.props.width)
            .height(ctx.props.height)
            .activate_on_click(ctx.props.activate_on_click)
            .on_select(ctx.link().callback(LogViewMsg::Selected))
            .on_activate(ctx.link().callback(LogViewMsg::Activated));
        list = list
            .selection_style_slot(selection_style)
            .unfocused_selection_style_slot(unfocused_selection_style);

        if let Some(empty_text) = ctx.props.empty_text.clone() {
            list = list
                .empty_text(empty_text)
                .empty_text_style(ctx.props.empty_text_style);
        }

        list.into()
    }
}

/// Rebuild the cached ListItems from current results and entries.
/// This is called when results, entries, or show_level change.
fn rebuild_cache(ctx: &mut Context<LogViewComponent>) {
    let mut items = Vec::with_capacity(ctx.state.results.len());

    for result in &ctx.state.results {
        if let Some(entry) = ctx.props.entries.get(result.source_index) {
            let mut spans = Vec::new();
            if ctx.props.show_level {
                spans.push(
                    Span::new(format!("[{}]", entry.level.label()))
                        .style(ctx.props.level_style(entry.level)),
                );
                spans.push(Span::new(" "));
            }

            if result.hits.is_empty() {
                spans.push(Span::new(entry.message.clone()));
            } else {
                spans.extend(highlight_spans(
                    &entry.message,
                    &result.hits,
                    ctx.props.style,
                    ctx.props
                        .selection_style
                        .explicit_style()
                        .unwrap_or_default(),
                ));
            }

            items.push(ListItem::from_spans(spans));
        }
    }

    ctx.state.cached_items = items.into();
    ctx.state.cached_entries_id = ctx.props.entries.as_ptr() as usize;
    ctx.state.cached_show_level = ctx.props.show_level;
}

/// Run the current filter synchronously and update `results` in place.
/// Used when only entries changed (no filter parameter change) to avoid
/// the two-render flicker from the async `spawn_filter` round-trip.
fn refilter_inline(ctx: &mut Context<LogViewComponent>) {
    let query = ctx.props.filter.as_deref().unwrap_or("");
    let case_matching = if ctx.props.case_sensitive {
        CaseMatching::Respect
    } else {
        CaseMatching::Ignore
    };
    ctx.state.results = match_logs(
        &ctx.props.entries,
        query,
        ctx.props.filter_mode,
        case_matching,
        Normalization::Smart,
    );
    ctx.state.query_id += 1;
}

fn spawn_filter(query_id: u64, ctx: &Context<LogViewComponent>) -> crate::core::component::Command {
    let query = ctx.props.filter.as_deref().unwrap_or("").to_string();
    let entries = ctx.props.entries.clone();
    let mode = ctx.props.filter_mode;
    let case_matching = if ctx.props.case_sensitive {
        CaseMatching::Respect
    } else {
        CaseMatching::Ignore
    };
    let link = ctx.link().clone();

    link.command(move |link| {
        let results = match_logs(&entries, &query, mode, case_matching, Normalization::Smart);
        link.send(LogViewMsg::ResultsReady { query_id, results });
    })
}

/// Optimized highlight spans using two-pointer approach.
/// Since hits are sorted, we can iterate through them once instead of binary searching per character.
fn highlight_spans(
    text: &str,
    hits: &[u32],
    base_style: crate::style::Style,
    selection_style: crate::style::Style,
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_highlight = false;
    let mut hit_idx = 0;

    for (char_idx, ch) in text.chars().enumerate() {
        // Check if current position is a hit using two-pointer technique
        // Advance hit_idx while the current hit is before our position
        while hit_idx < hits.len() && (hits[hit_idx] as usize) < char_idx {
            hit_idx += 1;
        }
        let is_hit = hit_idx < hits.len() && (hits[hit_idx] as usize) == char_idx;

        if is_hit != current_highlight && !current.is_empty() {
            let style = if current_highlight {
                base_style.patch(selection_style)
            } else {
                base_style
            };
            spans.push(Span::new(std::mem::take(&mut current)).style(style));
        }
        current_highlight = is_hit;
        current.push(ch);
    }

    if !current.is_empty() {
        let style = if current_highlight {
            base_style.patch(selection_style)
        } else {
            base_style
        };
        spans.push(Span::new(current).style(style));
    }

    spans
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::app::context::SurfaceMode;
    use crate::core::component::Context;
    use crate::core::node::NodeKind;
    use crate::runtime::RuntimeCore;
    use crate::style::{Rect, Theme};
    use crate::widgets::{LogEntry, LogView};

    use crate::style::Style;

    struct LiveFollowLogViewProbe;

    impl Component for LiveFollowLogViewProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            LogView::new()
                .entries([LogEntry::info("first"), LogEntry::info("second")])
                .auto_follow(true)
                .paused(false)
                .on_select(crate::callback::Callback::new(|_| {}))
                .into()
        }
    }

    #[test]
    fn highlight_spans_multibyte() {
        let text = "€a"; // '€' is 3 bytes
        let hits = vec![1]; // 'a' is at char index 1
        let base_style = Style::default();
        let selection_style = Style::new().reverse();

        let spans = highlight_spans(text, &hits, base_style, selection_style);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content.as_ref(), "€");
        assert_eq!(spans[0].style, base_style);
        assert_eq!(spans[1].content.as_ref(), "a");
        assert_eq!(spans[1].style, base_style.patch(selection_style));
    }

    #[test]
    fn highlight_spans_ascii() {
        let text = "abc";
        let hits = vec![1]; // 'b'
        let base_style = Style::default();
        let selection_style = Style::new().reverse();

        let spans = highlight_spans(text, &hits, base_style, selection_style);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content.as_ref(), "a");
        assert_eq!(spans[1].content.as_ref(), "b");
        assert_eq!(spans[1].style, base_style.patch(selection_style));
        assert_eq!(spans[2].content.as_ref(), "c");
    }

    #[test]
    fn live_follow_list_stays_click_selectable() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let mouse_capture = Rc::new(Cell::new(false));
        let mut runtime = RuntimeCore::new_test(
            LiveFollowLogViewProbe,
            (),
            bounds,
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);

        let list = runtime
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::List(list) => Some(list),
                _ => None,
            })
            .expect("log view should reconcile to a list node");

        assert!(list.on_select.is_some());
        assert!(list.on_activate.is_some());
    }
}
