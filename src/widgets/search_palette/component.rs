#![allow(private_interfaces)]

use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};
use unicode_width::UnicodeWidthStr;

use crate::callback::KeyHandler;
use crate::core::component::{Component, Context, KeyUpdate, TaskPolicy, Update, UpdateLevel};
use crate::core::element::Element;
use crate::core::event::KeyCode;
use crate::style::Length;
use crate::text::input::TextInput;
use crate::widgets::{Divider, Input, InputEvent, List, ListEvent, ListItem, Spacer, VStack};

use super::matching::{SearchResult, all_item_results, build_search_entries, match_items};
use super::render::{
    ListItemsOutput, RenderStyles, ScoreRender, SearchListItemsCtx, build_list_items,
};
use super::{SearchEvent, SearchItem, SearchPaletteProps};

/// Tracks whether the search query is owned by the palette's internal `TextInput`
/// (uncontrolled) or driven by an external `query` prop (controlled).
///
/// The two variants are mutually exclusive - only one carries live state at a
/// time, so no dead heap (undo history etc.) accumulates in controlled mode.
pub(crate) enum QuerySource {
    /// Default mode: the widget renders its own `Input` and owns the query.
    Uncontrolled(TextInput),
    /// Controlled mode: query is set by the caller via `SearchPalette::query()`.
    /// No `TextInput` or undo history is allocated.
    Controlled(Arc<str>),
}

impl QuerySource {
    fn query_str(&self) -> &str {
        match self {
            QuerySource::Uncontrolled(input) => input.text(),
            QuerySource::Controlled(q) => q.as_ref(),
        }
    }
}

pub(crate) struct SearchState {
    query_source: QuerySource,
    results: Vec<SearchResult>,
    results_query: Arc<str>,
    /// Index into `results` - always refers to a real matched item.
    selected: usize,
    query_id: u64,
    last_notified_selection: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub(crate) enum SearchPaletteMsg {
    QueryChanged(InputEvent),
    ResultsReady {
        query_id: u64,
        results: Vec<SearchResult>,
    },
    /// Fired by the List widget with the pre-resolved result index.
    Selected(usize),
    /// Fired by the List widget with the pre-resolved result index.
    Activated(usize),
    NavigateUp,
    NavigateDown,
    NavigateFirst,
    NavigateLast,
    NavigatePageUp,
    NavigatePageDown,
    ActivateSelected,
}

/// Maps navigation key codes to the corresponding `SearchPaletteMsg`.
///
/// Shared by both the uncontrolled-mode internal `Input` interceptor and the
/// controlled-mode `on_key` handler, eliminating the duplicated match block.
fn nav_key_to_msg(code: KeyCode) -> Option<SearchPaletteMsg> {
    match code {
        KeyCode::Up => Some(SearchPaletteMsg::NavigateUp),
        KeyCode::Down => Some(SearchPaletteMsg::NavigateDown),
        KeyCode::Enter => Some(SearchPaletteMsg::ActivateSelected),
        KeyCode::PageUp => Some(SearchPaletteMsg::NavigatePageUp),
        KeyCode::PageDown => Some(SearchPaletteMsg::NavigatePageDown),
        KeyCode::Home => Some(SearchPaletteMsg::NavigateFirst),
        KeyCode::End => Some(SearchPaletteMsg::NavigateLast),
        _ => None,
    }
}

fn search_input_key_interceptor(
    link: crate::callback::Link<SearchPaletteMsg>,
    user_interceptor: Option<KeyHandler>,
) -> KeyHandler {
    KeyHandler::new(move |key| {
        if key.mods == crate::core::event::KeyMods::default()
            && let Some(msg) = nav_key_to_msg(key.code)
        {
            link.send(msg);
            return true;
        }

        user_interceptor
            .as_ref()
            .map(|handler| handler.handle(key))
            .unwrap_or(false)
    })
}

pub(crate) struct SearchPaletteComponent<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T> SearchPaletteComponent<T> {
    pub(crate) fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

pub(super) fn element<T: Clone + PartialEq + 'static>(props: SearchPaletteProps<T>) -> Element {
    crate::child(|| SearchPaletteComponent::<T>::new(), props)
}

impl<T: Clone + PartialEq + 'static> Component for SearchPaletteComponent<T> {
    type Message = SearchPaletteMsg;
    type Properties = SearchPaletteProps<T>;
    type State = SearchState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        let query_source = if let Some(q) = &props.query {
            QuerySource::Controlled(q.clone())
        } else {
            let mut input = TextInput::default();
            if !props.initial_query.is_empty() {
                input.set_text(props.initial_query.to_string());
                input.set_cursor(props.initial_query.len());
            }
            QuerySource::Uncontrolled(input)
        };

        let results = initial_results(props, query_source.query_str());
        let results_query: Arc<str> = Arc::from(query_source.query_str().to_owned());
        let selected = resolve_initial_result_index(props.initial_selected_item_index, &results);

        Self::State {
            query_source,
            results,
            results_query,
            selected,
            query_id: 0,
            last_notified_selection: None,
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
        sync_current_selection(&ctx.props, &mut ctx.state);

        // When all items fit within the cap, create_state already computed the
        // complete result set synchronously - nothing to do.
        if ctx.props.items.len() <= sync_match_limit(&ctx.props) {
            return None;
        }

        // Either (a) there's a non-empty query that needs real fuzzy matching,
        // or (b) the item count exceeds the cap and we need the full zero-query
        // result set. In both cases, spawn async search.
        let query = Arc::from(ctx.state.query_source.query_str().to_owned());
        let query_id = ctx.state.query_id + 1;
        ctx.state.query_id = query_id;
        Some(spawn_search(
            ctx.link().clone(),
            query_id,
            query,
            &ctx.props.items,
            ctx.props.case_matching,
            ctx.props.normalization,
        ))
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let mut should_refresh = false;
        let mut reset_selection = false;

        match &ctx.props.query {
            Some(new_query) => {
                let query_changed = !matches!(
                    &ctx.state.query_source,
                    QuerySource::Controlled(current) if current == new_query
                );
                if query_changed {
                    ctx.state.query_source = QuerySource::Controlled(new_query.clone());
                    should_refresh = true;
                    reset_selection = true;
                }
            }
            None => {
                if let QuerySource::Controlled(current) = &ctx.state.query_source {
                    let mut input = TextInput::default();
                    input.set_text(current.to_string());
                    input.set_cursor(current.len());
                    ctx.state.query_source = QuerySource::Uncontrolled(input);
                    should_refresh = true;
                    reset_selection = true;
                }
            }
        }

        if old_props.items != ctx.props.items
            || old_props.case_matching != ctx.props.case_matching
            || old_props.normalization != ctx.props.normalization
        {
            should_refresh = true;
        }

        if should_refresh {
            return refresh_results(ctx, reset_selection);
        }

        if old_props.sync_selection != ctx.props.sync_selection {
            sync_current_selection(&ctx.props, &mut ctx.state);
            return Update::layout();
        }

        // Controlled palettes often drive `initial_selected_item_index` from external
        // state (e.g. a TextArea key interceptor). Without this, the list highlight
        // never moves when only that prop changes.
        if old_props.initial_selected_item_index != ctx.props.initial_selected_item_index {
            ctx.state.selected = resolve_initial_result_index(
                ctx.props.initial_selected_item_index,
                &ctx.state.results,
            );
            sync_current_selection(&ctx.props, &mut ctx.state);
            return Update::layout();
        }

        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let entries = if ctx.state.results_query.is_empty() || ctx.props.preserve_groups {
            ctx.props.entries.as_slice()
        } else {
            &[]
        };

        let ListItemsOutput {
            items: list_items,
            result_to_row,
            row_to_result,
        } = build_list_items(
            &ctx.props.items,
            entries,
            &ctx.state.results,
            SearchListItemsCtx {
                renderer: ctx.props.render_item.as_deref().map(|renderer| {
                    renderer as &dyn Fn(&SearchItem<T>, &super::SearchHighlight) -> Option<ListItem>
                }),
                status_renderer: ctx.props.item_status.as_deref().map(|renderer| {
                    renderer
                        as &dyn Fn(
                            &SearchItem<T>,
                            &super::SearchHighlight,
                        )
                            -> Option<crate::widgets::ListItemStatus>
                }),
                gutter_renderer: ctx.props.item_gutter.as_deref().map(|renderer| {
                    renderer
                        as &dyn Fn(
                            &SearchItem<T>,
                            &super::SearchHighlight,
                        )
                            -> Option<crate::widgets::ListItemGutter>
                }),
                styles: &RenderStyles {
                    item: ctx.props.item_style,
                    active_item: ctx.props.active_item_style,
                    description: ctx.props.description_style,
                    active_description: ctx.props.active_description_style,
                    focused_description: ctx.props.focused_description_style,
                    description_placement: ctx.props.description_placement,
                    description_separator: ctx.props.description_separator.clone(),
                    description_selection: ctx.props.description_selection,
                    description_overflow: ctx.props.description_overflow,
                    line_width: effective_search_line_width(ctx),
                    highlight: ctx.props.match_style,
                },
                score: ScoreRender {
                    show: ctx.props.show_scores,
                    gradient: ctx.props.score_gradient,
                    range: ctx.props.score_range,
                },
                selected_result_index: Some(ctx.state.selected),
                header_style: ctx.props.header_style,
            },
        );

        // Translate the result-space `selected` index to a visual row index for
        // the List widget. If there are no results the list is empty so 0 is fine.
        let visual_selected = result_to_row.get(ctx.state.selected).copied().unwrap_or(0);

        // Capture the mapping so the callbacks can reverse-translate row → result.
        let row_to_result_for_select = row_to_result.clone();
        let row_to_result_for_activate = row_to_result;

        let list_height = match ctx.props.height {
            Length::Auto => Length::Auto,
            Length::Px(px) if ctx.props.query.is_some() => Length::Px(px),
            _ => Length::Flex(1),
        };

        let list = List::new()
            .width(ctx.props.width)
            .height(list_height)
            .items(list_items)
            .selected(visual_selected)
            .border(ctx.props.list_config.border)
            .border_style(ctx.props.list_config.border_style)
            .padding(ctx.props.list_config.padding)
            .style(ctx.props.list_config.style)
            .selection_symbol(ctx.props.list_config.selection_symbol.clone())
            .selection_symbol_right(ctx.props.list_config.selection_symbol_right.clone())
            .selection_full_width(ctx.props.list_config.selection_full_width)
            .symbol_column(
                ctx.props
                    .list_symbol_column
                    .unwrap_or(ctx.props.list_config.symbol_column),
            )
            .gutter_gap(ctx.props.list_config.gutter_gap)
            .gutter_for_non_selectable(ctx.props.list_config.gutter_for_non_selectable)
            .active_style_slot(ctx.props.list_active_style)
            .item_horizontal_padding(ctx.props.list_config.item_horizontal_padding)
            .header_horizontal_padding(ctx.props.list_config.header_horizontal_padding)
            .focusable(ctx.props.list_focusable)
            .scrollbar(ctx.props.list_config.scrollbar)
            .scrollbar_config(ctx.props.list_config.scrollbar_config.clone());
        let mut list = list
            .selection_style_slot(ctx.props.list_config.selection_style)
            .unfocused_selection_style_slot(ctx.props.list_config.unfocused_selection_style)
            .hover_style_slot(ctx.props.list_hover_style)
            .item_hover_style_slot(
                ctx.props
                    .list_config
                    .item_hover_style
                    .unwrap_or(crate::style::StyleSlot::Inherit),
            )
            .activate_on_click(false)
            .on_select({
                let link = ctx.link().clone();
                crate::callback::Callback::new(move |event: ListEvent| {
                    // Translate the visual row index to a result index.
                    // If the row is a header/spacer, ignore the event.
                    if let Some(Some(result_idx)) =
                        row_to_result_for_select.get(event.index).copied()
                    {
                        link.send(SearchPaletteMsg::Selected(result_idx));
                    }
                })
            })
            .on_activate({
                let link = ctx.link().clone();
                crate::callback::Callback::new(move |event: ListEvent| {
                    if let Some(Some(result_idx)) =
                        row_to_result_for_activate.get(event.index).copied()
                    {
                        link.send(SearchPaletteMsg::Activated(result_idx));
                    }
                })
            });

        if let Some(style) = ctx.props.list_config.selection_symbol_style {
            list = list.selection_symbol_style(style);
        }
        if let Some(style) = ctx.props.list_config.unfocused_selection_symbol_style {
            list = list.unfocused_selection_symbol_style(style);
        }
        if let Some(ref symbol) = ctx.props.list_unselected_symbol {
            list = list.unselected_symbol(Some(symbol.clone()));
        }
        if let Some(ref symbol) = ctx.props.list_active_symbol {
            list = list.active_symbol(Some(symbol.clone()));
        }
        if let Some(style) = ctx.props.list_active_symbol_style {
            list = list.active_symbol_style(style);
        }

        if let Some(text) = ctx.props.empty_text.clone() {
            list = list
                .empty_text(text)
                .empty_text_style(ctx.props.list_config.empty_text_style);
        }

        let mut stack = VStack::new()
            .width(ctx.props.width)
            .height(ctx.props.height);

        // Uncontrolled mode: render the Input + divider above the results list.
        // In controlled mode the caller provides and renders the input elsewhere -
        // no Input widget is constructed, no TextInput state is allocated.
        if let QuerySource::Uncontrolled(text_input) = &ctx.state.query_source {
            let input_key_interceptor = search_input_key_interceptor(
                ctx.link().clone(),
                ctx.props.input_key_interceptor.clone(),
            );

            let default_suffix = format!("{}/{}", ctx.state.results.len(), ctx.props.items.len());
            let prefix = ctx.props.input_prefix.as_deref().unwrap_or(" ");
            let suffix = ctx.props.input_suffix.as_deref().unwrap_or(&default_suffix);

            let mut input = Input::new(text_input.text().to_owned())
                .cursor(text_input.cursor())
                .anchor(text_input.anchor())
                .caret_shape(ctx.props.input_caret_shape)
                .placeholder(ctx.props.placeholder.as_ref())
                .prefix(prefix)
                .prefix_style(ctx.props.input_prefix_style)
                .focus_prefix_style(ctx.props.input_focus_prefix_style)
                .suffix(suffix)
                .suffix_style(ctx.props.input_suffix_style)
                .focus_suffix_style(ctx.props.input_focus_suffix_style)
                .border(ctx.props.input_border)
                .border_style(ctx.props.input_border_style)
                .padding(ctx.props.input_padding)
                .style(ctx.props.input_style)
                .hover_style_slot(ctx.props.input_hover_style)
                .focus_style_slot(ctx.props.input_focus_style)
                .focus_content_style(ctx.props.input_focus_content_style)
                .placeholder_style(ctx.props.input_placeholder_style)
                .focus_placeholder_style(ctx.props.input_focus_placeholder_style)
                .key_interceptor(input_key_interceptor)
                .on_change(ctx.link().callback(SearchPaletteMsg::QueryChanged));

            if let Some(color) = ctx.props.input_caret_color {
                input = input.caret_color(color);
            }

            stack = stack.child(input);
            if ctx.props.input_divider {
                let divider = Divider::horizontal()
                    .style(ctx.props.input_divider_style)
                    .join_frame(ctx.props.input_divider_join_frame);
                stack = stack.child(divider);
            } else {
                // When the divider is disabled, keep a 1-line vertical gap so the
                // palette layout stays consistent with the default appearance.
                stack = stack.child(Spacer::new().height(Length::Px(1)));
            }
        }

        let mut element: Element = stack.child(list).into();
        if let Some(max_width) = ctx.props.max_width {
            element = element.max_width(max_width);
        }
        if let Some(max_height) = ctx.props.max_height {
            element = element.max_height(max_height);
        }
        element
    }

    fn on_key(&mut self, key: crate::core::event::KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        // In uncontrolled mode the Input widget intercepts navigation keys
        // before text editing. In controlled mode there is no Input widget, so
        // we handle navigation here (fires when the results List or the palette
        // container has keyboard focus).
        if matches!(ctx.state.query_source, QuerySource::Uncontrolled(_)) {
            return KeyUpdate::unhandled(Update::none());
        }
        if key.mods != crate::core::event::KeyMods::default() {
            return KeyUpdate::unhandled(Update::none());
        }
        let Some(msg) = nav_key_to_msg(key.code) else {
            return KeyUpdate::unhandled(Update::none());
        };
        ctx.link().send(msg);
        KeyUpdate::handled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            SearchPaletteMsg::QueryChanged(event) => {
                // Only reachable in uncontrolled mode: the Input widget that
                // emits this message is never rendered when query_source is Controlled.
                let QuerySource::Uncontrolled(ref mut text_input) = ctx.state.query_source else {
                    return Update::none();
                };

                text_input.set_text(event.value.as_ref().to_string());
                text_input.set_cursor(event.cursor);
                text_input.set_anchor(event.anchor);

                let query: Arc<str> = Arc::from(text_input.text().to_owned());
                if let Some(cb) = &ctx.props.on_query_change {
                    cb.emit(query.clone());
                }

                refresh_results(ctx, true)
            }
            SearchPaletteMsg::ResultsReady {
                query_id,
                mut results,
            } => {
                if query_id != ctx.state.query_id {
                    return Update::none();
                }
                let prev_len = ctx.state.results.len();
                let query_empty = ctx.state.query_source.query_str().is_empty();
                let expanded_sync_results =
                    query_empty && prev_len < results.len() && !results.is_empty();
                if ctx.props.preserve_groups && !ctx.props.entries.is_empty() && !query_empty {
                    results.sort_by_key(|r| r.item_index);
                }
                ctx.state.results = results;
                ctx.state.results_query = Arc::from(ctx.state.query_source.query_str().to_owned());
                if expanded_sync_results || ctx.state.selected >= ctx.state.results.len() {
                    ctx.state.selected = resolve_initial_result_index(
                        ctx.props.initial_selected_item_index,
                        &ctx.state.results,
                    );
                }
                sync_current_selection(&ctx.props, &mut ctx.state);
                Update::layout()
            }
            SearchPaletteMsg::Selected(result_idx) => {
                ctx.state.selected = result_idx;
                emit_search_event(&ctx.props, &ctx.state.results, result_idx, true);
                remember_current_selection(&mut ctx.state);
                Update::layout()
            }
            SearchPaletteMsg::Activated(result_idx) => {
                emit_search_event(&ctx.props, &ctx.state.results, result_idx, false);
                Update::none()
            }
            SearchPaletteMsg::NavigateUp => {
                navigate_up(&ctx.props, &mut ctx.state);
                Update::layout()
            }
            SearchPaletteMsg::NavigateDown => {
                navigate_down(&ctx.props, &mut ctx.state);
                Update::layout()
            }
            SearchPaletteMsg::NavigateFirst => {
                if !ctx.state.results.is_empty() {
                    ctx.state.selected = 0;
                    emit_search_event(&ctx.props, &ctx.state.results, 0, true);
                    remember_current_selection(&mut ctx.state);
                }
                Update::layout()
            }
            SearchPaletteMsg::NavigateLast => {
                let len = ctx.state.results.len();
                if len > 0 {
                    ctx.state.selected = len - 1;
                    emit_search_event(&ctx.props, &ctx.state.results, len - 1, true);
                    remember_current_selection(&mut ctx.state);
                }
                Update::layout()
            }
            SearchPaletteMsg::NavigatePageUp => {
                let len = ctx.state.results.len();
                if len > 0 {
                    ctx.state.selected = ctx.state.selected.saturating_sub(10);
                    emit_search_event(&ctx.props, &ctx.state.results, ctx.state.selected, true);
                    remember_current_selection(&mut ctx.state);
                }
                Update::layout()
            }
            SearchPaletteMsg::NavigatePageDown => {
                let len = ctx.state.results.len();
                if len > 0 {
                    ctx.state.selected = (ctx.state.selected + 10).min(len - 1);
                    emit_search_event(&ctx.props, &ctx.state.results, ctx.state.selected, true);
                    remember_current_selection(&mut ctx.state);
                }
                Update::layout()
            }
            SearchPaletteMsg::ActivateSelected => {
                emit_search_event(&ctx.props, &ctx.state.results, ctx.state.selected, false);
                Update::none()
            }
        }
    }
}

fn resolve_initial_result_index(
    initial_item_index: Option<usize>,
    results: &[SearchResult],
) -> usize {
    if results.is_empty() {
        return 0;
    }
    if let Some(idx) = initial_item_index
        && let Some(r) = results.iter().position(|res| res.item_index == idx)
    {
        return r;
    }
    0
}

fn initial_results<T>(props: &SearchPaletteProps<T>, query: &str) -> Vec<SearchResult> {
    if query.trim().is_empty() {
        if props.items.len() <= sync_match_limit(props) {
            return all_item_results(props.items.len());
        }
        let cap = props.items.len().min(sync_match_limit(props));
        return all_item_results(cap);
    }

    if props.items.len() <= sync_match_limit(props) {
        let entries = build_search_entries(&props.items);
        return match_items(&entries, query, props.case_matching, props.normalization);
    }

    Vec::new()
}

fn refresh_results<T: Clone + PartialEq + 'static>(
    ctx: &mut Context<SearchPaletteComponent<T>>,
    reset_selection: bool,
) -> Update {
    let query: Arc<str> = Arc::from(ctx.state.query_source.query_str().to_owned());

    if query.is_empty() {
        ctx.state.results = initial_results(&ctx.props, query.as_ref());
        ctx.state.results_query = query.clone();
        if ctx.props.items.len() <= sync_match_limit(&ctx.props) {
            if reset_selection {
                ctx.state.selected = resolve_initial_result_index(
                    ctx.props.initial_selected_item_index,
                    &ctx.state.results,
                );
            }
            sync_current_selection(&ctx.props, &mut ctx.state);
            return Update::layout();
        }
        if reset_selection {
            ctx.state.selected = resolve_initial_result_index(
                ctx.props.initial_selected_item_index,
                &ctx.state.results,
            );
        }
    } else if ctx.props.items.len() <= sync_match_limit(&ctx.props) {
        let mut results = initial_results(&ctx.props, query.as_ref());
        if ctx.props.preserve_groups && !ctx.props.entries.is_empty() {
            results.sort_by_key(|r| r.item_index);
        }
        ctx.state.results = results;
        ctx.state.results_query = query.clone();
        if reset_selection {
            ctx.state.selected = resolve_initial_result_index(
                ctx.props.initial_selected_item_index,
                &ctx.state.results,
            );
        }
        sync_current_selection(&ctx.props, &mut ctx.state);
        return Update::layout();
    }

    let query_id = ctx.state.query_id + 1;
    ctx.state.query_id = query_id;
    layout_with_command(spawn_search(
        ctx.link().clone(),
        query_id,
        query,
        &ctx.props.items,
        ctx.props.case_matching,
        ctx.props.normalization,
    ))
}

fn layout_with_command(command: crate::core::component::Command) -> Update {
    Update {
        dirty: true,
        level: UpdateLevel::Layout,
        command: Some(command),
    }
}

fn effective_search_line_width<T: Clone + PartialEq + 'static>(
    ctx: &Context<SearchPaletteComponent<T>>,
) -> Option<u16> {
    let mut width = ctx.viewport().w;
    if width == 0 {
        return None;
    }

    if ctx.props.list_config.border {
        width = width.saturating_sub(2);
    }
    width = width.saturating_sub(ctx.props.list_config.padding.horizontal());

    if ctx.props.list_config.scrollbar
        && matches!(
            ctx.props.list_config.scrollbar_config.variant,
            crate::style::ScrollbarVariant::Standalone
        )
    {
        width = width.saturating_sub(1);
    }

    let symbol_width = ctx
        .props
        .list_config
        .selection_symbol
        .as_deref()
        .map(UnicodeWidthStr::width)
        .unwrap_or(0)
        .max(
            ctx.props
                .list_active_symbol
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0),
        )
        .max(
            ctx.props
                .list_unselected_symbol
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0),
        ) as u16;

    width = width.saturating_sub(symbol_width);
    width = width.saturating_sub(ctx.props.list_config.item_horizontal_padding.horizontal());

    if width == 0 { None } else { Some(width) }
}

fn sync_match_limit<T>(props: &SearchPaletteProps<T>) -> usize {
    props.sync_match_limit.max(1)
}

fn current_selection_signature(state: &SearchState) -> Option<(usize, usize)> {
    state
        .results
        .get(state.selected)
        .map(|result| (state.selected, result.item_index))
}

fn remember_current_selection(state: &mut SearchState) {
    state.last_notified_selection = current_selection_signature(state);
}

fn sync_current_selection<T: Clone>(props: &SearchPaletteProps<T>, state: &mut SearchState) {
    let current = current_selection_signature(state);

    if !props.sync_selection {
        state.last_notified_selection = current;
        return;
    }

    if current.is_none() {
        state.last_notified_selection = None;
        return;
    }

    if state.last_notified_selection == current {
        return;
    }

    emit_search_event(props, &state.results, state.selected, true);
    state.last_notified_selection = current;
}

fn emit_search_event<T: Clone>(
    props: &SearchPaletteProps<T>,
    results: &[SearchResult],
    match_index: usize,
    is_select: bool,
) {
    let Some(result) = results.get(match_index) else {
        return;
    };
    let Some(item) = props.items.get(result.item_index) else {
        return;
    };

    let event = SearchEvent {
        match_index,
        item_index: result.item_index,
        item: item.clone(),
    };

    if is_select {
        if let Some(cb) = &props.on_select {
            cb.emit(event);
        }
    } else if let Some(cb) = &props.on_activate {
        cb.emit(event);
    }
}

fn navigate_up<T: Clone>(props: &SearchPaletteProps<T>, state: &mut SearchState) {
    let len = state.results.len();
    if len == 0 {
        return;
    }
    let next = if state.selected == 0 {
        if props.navigation_wrap { len - 1 } else { 0 }
    } else {
        state.selected - 1
    };
    if next != state.selected {
        state.selected = next;
        emit_search_event(props, &state.results, state.selected, true);
        remember_current_selection(state);
    }
}

fn navigate_down<T: Clone>(props: &SearchPaletteProps<T>, state: &mut SearchState) {
    let len = state.results.len();
    if len == 0 {
        return;
    }
    let next = if state.selected + 1 >= len {
        if props.navigation_wrap { 0 } else { len - 1 }
    } else {
        state.selected + 1
    };
    if next != state.selected {
        state.selected = next;
        emit_search_event(props, &state.results, state.selected, true);
        remember_current_selection(state);
    }
}

fn spawn_search<T>(
    link: crate::callback::Link<SearchPaletteMsg>,
    query_id: u64,
    query: Arc<str>,
    items: &[SearchItem<T>],
    case_matching: CaseMatching,
    normalization: Normalization,
) -> crate::core::component::Command {
    let item_count = items.len();
    let entries = if query.trim().is_empty() {
        None
    } else {
        Some(build_search_entries(items))
    };
    link.command_keyed("search", TaskPolicy::LatestOnly, move |link| {
        if link.is_cancelled() {
            return;
        }
        let results = entries.as_ref().map_or_else(
            || all_item_results(item_count),
            |entries| match_items(entries, &query, case_matching, normalization),
        );
        let _ = link.send_if_not_cancelled(SearchPaletteMsg::ResultsReady { query_id, results });
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::Arc;

    use super::{
        QuerySource, SearchPaletteMsg, SearchState, initial_results, navigate_down, navigate_up,
        resolve_initial_result_index, search_input_key_interceptor, sync_current_selection,
    };
    use crate::callback::{Callback, Dispatcher, KeyHandler, Link, ScopeId};
    use crate::core::component::{Component, Context, Update, UpdateLevel};
    use crate::core::element::Element;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::runtime::RuntimeCore;
    use crate::style::{Length, Rect, Theme};
    use crate::widgets::{ListConfig, SearchEvent, SearchItem, SearchPalette};

    struct PaletteRoot {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for PaletteRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            SearchPalette::<usize>::new()
                .items((0..8).map(|i| SearchItem::new(format!("item-{i}"), i)))
                .sync_match_limit(8)
                .height(Length::Px(4))
                .into()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    #[test]
    fn initial_results_match_synchronously_within_limit() {
        let palette = SearchPalette::<usize>::new()
            .items((0..3).map(|i| SearchItem::new(format!("item-{i}"), i)))
            .sync_match_limit(3);

        let results = initial_results(&palette.props, "item-2");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].item_index, 2);
    }

    #[test]
    fn initial_results_defer_non_empty_query_above_limit() {
        let palette = SearchPalette::<usize>::new()
            .items((0..3).map(|i| SearchItem::new(format!("item-{i}"), i)))
            .sync_match_limit(2);

        let results = initial_results(&palette.props, "item-2");

        assert!(results.is_empty());
    }

    #[test]
    fn navigation_update_is_scoped_layout_not_full_root_render() {
        let view_count = Rc::new(Cell::new(0));
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let mut runtime = RuntimeCore::new_test(
            PaletteRoot {
                view_count: Rc::clone(&view_count),
            },
            (),
            Rect::default(),
            Theme::default(),
            crate::app::context::SurfaceMode::Fullscreen,
            Rc::new(Cell::new(true)),
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);
        assert_eq!(view_count.get(), 1);

        let palette_scope = ScopeId(2);
        let level = runtime
            .update_from_boxed(palette_scope, Box::new(SearchPaletteMsg::NavigateDown))
            .expect("palette navigation should update");

        assert_eq!(level, UpdateLevel::Layout);
        assert!(runtime.refresh_cached_scopes(&[palette_scope], bounds));
        assert!(runtime.reconcile_cached_element(bounds, None, None, None));
        assert_eq!(view_count.get(), 1);
    }

    #[test]
    fn zero_sync_limit_clamps_to_one() {
        let palette = SearchPalette::<usize>::new()
            .items((0..2).map(|i| SearchItem::new(format!("item-{i}"), i)))
            .sync_match_limit(0);

        let results = initial_results(&palette.props, "");

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn list_symbol_column_convenience_overrides_list_config_default() {
        let palette = SearchPalette::<usize>::new().list_symbol_column(false);

        assert_eq!(palette.props.list_symbol_column, Some(false));
    }

    #[test]
    fn list_config_keeps_new_leading_column_fields() {
        let config = ListConfig::new()
            .symbol_column(false)
            .gutter_gap(2)
            .gutter_for_non_selectable(true);
        let palette = SearchPalette::<usize>::new().list_config(config);

        assert!(!palette.props.list_config.symbol_column);
        assert_eq!(palette.props.list_config.gutter_gap, 2);
        assert!(palette.props.list_config.gutter_for_non_selectable);
    }

    #[test]
    fn sync_selection_emits_current_item_only_once() {
        let selected = Rc::new(RefCell::new(Vec::new()));
        let selected_for_cb = Rc::clone(&selected);

        let palette = SearchPalette::<usize>::new()
            .items((0..3).map(|i| SearchItem::new(format!("item-{i}"), i)))
            .sync_selection(true)
            .on_select(Callback::new(move |event: SearchEvent<usize>| {
                selected_for_cb
                    .borrow_mut()
                    .push((event.match_index, event.item_index));
            }));

        let mut state = SearchState {
            query_source: QuerySource::Controlled(Arc::from("")),
            results: initial_results(&palette.props, ""),
            results_query: Arc::from(""),
            selected: 0,
            query_id: 0,
            last_notified_selection: None,
        };

        sync_current_selection(&palette.props, &mut state);
        sync_current_selection(&palette.props, &mut state);

        assert_eq!(&*selected.borrow(), &[(0, 0)]);
    }

    #[test]
    fn navigation_wrap_can_be_disabled() {
        let selected = Rc::new(RefCell::new(Vec::new()));
        let selected_for_cb = Rc::clone(&selected);

        let palette = SearchPalette::<usize>::new()
            .items((0..3).map(|i| SearchItem::new(format!("item-{i}"), i)))
            .initial_selected_item_index(Some(2))
            .navigation_wrap(false)
            .on_select(Callback::new(move |event: SearchEvent<usize>| {
                selected_for_cb
                    .borrow_mut()
                    .push((event.match_index, event.item_index));
            }));

        let mut state = SearchState {
            query_source: QuerySource::Controlled(Arc::from("")),
            results: initial_results(&palette.props, ""),
            results_query: Arc::from(""),
            selected: resolve_initial_result_index(
                palette.props.initial_selected_item_index,
                &initial_results(&palette.props, ""),
            ),
            query_id: 0,
            last_notified_selection: None,
        };

        navigate_down(&palette.props, &mut state);
        assert_eq!(state.selected, 2);
        assert!(selected.borrow().is_empty());

        navigate_up(&palette.props, &mut state);
        assert_eq!(state.selected, 1);
        assert_eq!(&*selected.borrow(), &[(1, 1)]);
    }

    #[test]
    fn internal_input_interceptor_prioritizes_palette_navigation() {
        let messages = Rc::new(RefCell::new(Vec::new()));
        let messages_for_dispatch = Rc::clone(&messages);
        let dispatcher = Dispatcher::new(move |_scope, msg| {
            messages_for_dispatch
                .borrow_mut()
                .push(*msg.downcast::<SearchPaletteMsg>().expect("search message"));
        });
        let link = Link::new(ScopeId(1), dispatcher);

        let user_seen = Rc::new(RefCell::new(false));
        let user_seen_for_handler = Rc::clone(&user_seen);
        let user_interceptor = KeyHandler::new(move |_key| {
            *user_seen_for_handler.borrow_mut() = true;
            true
        });

        let handler = search_input_key_interceptor(link, Some(user_interceptor));

        assert!(handler.handle(key(KeyCode::Down)));
        assert!(matches!(
            messages.borrow().first(),
            Some(SearchPaletteMsg::NavigateDown)
        ));
        assert!(!*user_seen.borrow());

        assert!(handler.handle(key(KeyCode::Char(' '))));
        assert_eq!(messages.borrow().len(), 1);
        assert!(*user_seen.borrow());
    }
}
