use super::DocumentLineNumberMode;
use super::node::DocumentViewNode;
use super::node::{DocumentFlattenCtx, build_visual_text_index, flatten_blocks};
#[cfg(feature = "diff-view")]
use super::node::{insert_source_padding_visual_lines, source_visual_heights};
use crate::style::Length;
#[cfg(feature = "diff-view")]
use crate::widgets::diff_view::{
    compute_split_wrap_padding, compute_split_wrap_padding_from_heights, peer_pass1_source_heights,
    peer_simulated_content_width, record_pass1_source_heights, split_wrap_layout_pass,
};

#[derive(Clone)]
pub(crate) struct DocumentVisualPlan {
    pub content_w: u16,
    pub visual_lines: Vec<super::node::DocumentVisualLine>,
    pub max_line_width: u16,
}

fn flatten_visual_lines(
    dv_node: &DocumentViewNode,
    doc: &super::format::FormattedDocument,
    content_w: u16,
) -> (Vec<super::node::DocumentVisualLine>, u16) {
    let (visual_lines, max_line_width) = flatten_blocks(
        doc,
        content_w,
        DocumentFlattenCtx {
            wrap: dv_node.wrap,
            table_wrap: dv_node.table_wrap,
            table_width_mode: dv_node.table_width_mode,
            table_outer_frame: dv_node.table_outer_frame,
            table_column_separators: dv_node.table_column_separators,
            table_row_separators: dv_node.table_row_separators,
            table_cell_padding: dv_node.table_cell_padding,
            table_border_variant: dv_node.table_border_variant,
            doc_styles: &dv_node.doc_styles,
            #[cfg(feature = "syntax-syntect")]
            code_strategy: dv_node.code_syntax_strategy.as_deref(),
        },
    );

    #[cfg(feature = "diff-view")]
    let mut visual_lines = visual_lines;

    #[cfg(feature = "diff-view")]
    if dv_node.wrap
        && content_w > 0
        && let Some(peer_lines) = dv_node.peer_source_lines.as_deref()
    {
        let own_source_heights = source_visual_heights(&visual_lines);
        let sync_side = (&dv_node.split_wrap_sync, dv_node.split_wrap_side);

        if let (Some(sync), Some(side)) = sync_side {
            let pass = split_wrap_layout_pass(sync);
            if pass == 1 {
                record_pass1_source_heights(sync, side, &own_source_heights);
            } else if pass == 2 {
                let padding = if let Some(peer_h) = peer_pass1_source_heights(sync, side) {
                    compute_split_wrap_padding_from_heights(&own_source_heights, peer_h.as_ref())
                } else {
                    let peer_sim_w =
                        peer_simulated_content_width(sync, side, content_w).unwrap_or(content_w);
                    compute_split_wrap_padding(&own_source_heights, peer_lines, peer_sim_w)
                };
                insert_source_padding_visual_lines(&mut visual_lines, &padding);
            } else {
                let peer_sim_w =
                    peer_simulated_content_width(sync, side, content_w).unwrap_or(content_w);
                let padding =
                    compute_split_wrap_padding(&own_source_heights, peer_lines, peer_sim_w);
                insert_source_padding_visual_lines(&mut visual_lines, &padding);
            }
        } else {
            let padding = compute_split_wrap_padding(&own_source_heights, peer_lines, content_w);
            insert_source_padding_visual_lines(&mut visual_lines, &padding);
        }
    }

    (visual_lines, max_line_width)
}

pub(crate) fn build_document_visual_plan(
    dv_node: &DocumentViewNode,
    doc: &super::format::FormattedDocument,
    inner_w: u16,
    scrollbar_cols: u16,
) -> DocumentVisualPlan {
    let source_line_count = dv_node.value.split('\n').count().max(1);
    let mut gutter = super::layout::resolved_gutter_base_width(
        source_line_count,
        dv_node.line_numbers,
        dv_node.min_line_number_width,
        dv_node.line_number_separator,
        dv_node.line_number_content_gap,
        dv_node.gutter_col_width,
    );
    let mut total_gutter = super::layout::gutter_total_width(gutter, dv_node.gutter_gap);
    let mut content_w =
        super::layout::content_width_from_inner(inner_w, total_gutter, scrollbar_cols);

    let (mut visual_lines, mut max_line_width) = flatten_visual_lines(dv_node, doc, content_w);

    if dv_node.line_numbers && matches!(dv_node.line_number_mode, DocumentLineNumberMode::Visual) {
        let desired_gutter = super::layout::resolved_gutter_base_width(
            visual_lines.len().max(1),
            true,
            dv_node.min_line_number_width,
            dv_node.line_number_separator,
            dv_node.line_number_content_gap,
            0,
        );
        if desired_gutter != gutter {
            gutter = desired_gutter;
            total_gutter = super::layout::gutter_total_width(gutter, dv_node.gutter_gap);
            content_w =
                super::layout::content_width_from_inner(inner_w, total_gutter, scrollbar_cols);
            let (reflattened, reflattened_max_w) = flatten_visual_lines(dv_node, doc, content_w);
            visual_lines = reflattened;
            max_line_width = reflattened_max_w;
        }
    }

    DocumentVisualPlan {
        content_w,
        visual_lines,
        max_line_width,
    }
}

/// Measure-only metrics: `(content_w, visual_line_count, max_line_width)`.
///
/// For documents whose blocks are all `Lines` (diffs, plain text) and that do
/// not use visual line-number renumbering, this counts visual lines without
/// materializing/cloning per-line spans - the clone+alloc that dominates the
/// measure path while resizing split-wrap diffs. Any other document falls back
/// to [`build_document_visual_plan`] so its behavior is byte-for-byte unchanged.
///
/// For the fast path the result equals
/// `(plan.content_w, plan.visual_lines.len(), plan.max_line_width)` from
/// [`build_document_visual_plan`]; the split-wrap pass side effects
/// (recording pass-1 source heights) are preserved.
pub(crate) fn build_document_visual_metrics(
    dv_node: &DocumentViewNode,
    doc: &super::format::FormattedDocument,
    inner_w: u16,
    scrollbar_cols: u16,
) -> (u16, usize, u16) {
    // Visual line-number renumbering can grow the gutter based on the wrapped
    // line count, which feeds back into content width and forces a re-flatten.
    // The fast path does not model that loop, so defer to the full plan when it
    // could apply. (Diff panes set `line_numbers(false)`, so they take the fast
    // path.)
    let visual_renumber =
        dv_node.line_numbers && matches!(dv_node.line_number_mode, DocumentLineNumberMode::Visual);

    if !visual_renumber {
        let source_line_count = dv_node.value.split('\n').count().max(1);
        let gutter = super::layout::resolved_gutter_base_width(
            source_line_count,
            dv_node.line_numbers,
            dv_node.min_line_number_width,
            dv_node.line_number_separator,
            dv_node.line_number_content_gap,
            dv_node.gutter_col_width,
        );
        let total_gutter = super::layout::gutter_total_width(gutter, dv_node.gutter_gap);
        let content_w =
            super::layout::content_width_from_inner(inner_w, total_gutter, scrollbar_cols);

        if let Some((source_heights, max_w)) =
            super::node::lines_only_source_heights(doc, content_w, dv_node.wrap)
        {
            let count = visual_line_count_with_split_wrap(dv_node, content_w, &source_heights);
            return (content_w, count, max_w);
        }
    }

    let plan = build_document_visual_plan(dv_node, doc, inner_w, scrollbar_cols);
    (plan.content_w, plan.visual_lines.len(), plan.max_line_width)
}

/// Total visual-line count after split-wrap row-alignment padding, mirroring the
/// `#[cfg(feature = "diff-view")]` block in [`flatten_visual_lines`] but operating
/// on per-source heights instead of materialized visual lines. Also performs the
/// same pass-1 height recording side effect the dual-pass relies on.
fn visual_line_count_with_split_wrap(
    dv_node: &DocumentViewNode,
    content_w: u16,
    source_heights: &[u16],
) -> usize {
    let own_total: usize = source_heights.iter().map(|h| *h as usize).sum();

    #[cfg(feature = "diff-view")]
    {
        if dv_node.wrap
            && content_w > 0
            && let Some(peer_lines) = dv_node.peer_source_lines.as_deref()
        {
            let pad_total = match (&dv_node.split_wrap_sync, dv_node.split_wrap_side) {
                (Some(sync), Some(side)) => {
                    let pass = split_wrap_layout_pass(sync);
                    if pass == 1 {
                        record_pass1_source_heights(sync, side, source_heights);
                        0
                    } else if pass == 2 {
                        let padding = if let Some(peer_h) = peer_pass1_source_heights(sync, side) {
                            compute_split_wrap_padding_from_heights(source_heights, peer_h.as_ref())
                        } else {
                            let peer_sim_w = peer_simulated_content_width(sync, side, content_w)
                                .unwrap_or(content_w);
                            compute_split_wrap_padding(source_heights, peer_lines, peer_sim_w)
                        };
                        padding.iter().map(|p| *p as usize).sum()
                    } else {
                        let peer_sim_w = peer_simulated_content_width(sync, side, content_w)
                            .unwrap_or(content_w);
                        compute_split_wrap_padding(source_heights, peer_lines, peer_sim_w)
                            .iter()
                            .map(|p| *p as usize)
                            .sum()
                    }
                }
                _ => compute_split_wrap_padding(source_heights, peer_lines, content_w)
                    .iter()
                    .map(|p| *p as usize)
                    .sum(),
            };
            return own_total.saturating_add(pad_total);
        }
    }

    let _ = (dv_node, content_w);
    own_total
}

pub(crate) fn auto_height_for_visual_plan(
    dv_node: &DocumentViewNode,
    visual_line_count: usize,
    max_line_width: u16,
    content_w: u16,
) -> u16 {
    let h_scrollbar_over_border = super::layout::h_scrollbar_over_border(
        dv_node.h_scrollbar,
        dv_node.h_scrollbar_variant,
        dv_node.border,
    );
    let h_scrollbar_visible = super::layout::h_scrollbar_visible(
        dv_node.h_scrollbar,
        dv_node.wrap,
        max_line_width as usize,
        content_w,
    );

    super::layout::visual_height_with_chrome(
        visual_line_count,
        dv_node.padding,
        dv_node.border,
        h_scrollbar_visible,
        h_scrollbar_over_border,
    )
}

pub(crate) fn viewport_height_for_visual_plan(
    dv_node: &DocumentViewNode,
    rect: crate::style::Rect,
    max_line_width: u16,
    content_w: u16,
) -> u16 {
    let h_scrollbar_over_border = super::layout::h_scrollbar_over_border(
        dv_node.h_scrollbar,
        dv_node.h_scrollbar_variant,
        dv_node.border,
    );
    let h_scrollbar_visible = super::layout::h_scrollbar_visible(
        dv_node.h_scrollbar,
        dv_node.wrap,
        max_line_width as usize,
        content_w,
    );

    let mut viewport_h = rect.inner(dv_node.border, dv_node.padding).h;
    if h_scrollbar_visible && !h_scrollbar_over_border {
        viewport_h = viewport_h.saturating_sub(1);
    }
    viewport_h
}

pub(crate) fn apply_visual_plan_to_node(dv_node: &mut DocumentViewNode, plan: DocumentVisualPlan) {
    dv_node.visual_cache.lines = plan.visual_lines;
    dv_node.visual_cache.source_line_map = dv_node
        .visual_cache
        .lines
        .iter()
        .map(|line| line.source_line)
        .collect();
    dv_node.visual_cache.max_line_width = plan.max_line_width;
    let (line_texts, line_starts, line_lengths, flat_text) =
        build_visual_text_index(&dv_node.visual_cache.lines, plan.content_w);
    dv_node.visual_cache.line_texts = line_texts;
    dv_node.visual_cache.line_starts = line_starts;
    dv_node.visual_cache.line_lengths = line_lengths;
    dv_node.visual_cache.flat_text = flat_text;
    dv_node.total_visual_lines = dv_node.visual_cache.lines.len();
    dv_node.max_line_width = plan.max_line_width;
}

pub(crate) fn should_use_visual_auto_height(dv_height: Length) -> bool {
    matches!(dv_height, Length::Auto)
}
