#![allow(clippy::needless_range_loop)]

use crate::layout::axis::{Axis, align_x, requested_main_axis};
use crate::layout::measure::min_size_constrained;
use crate::style::{Align, Justify, Length, Rect};

use super::{GridItem, GridProps};

pub(crate) struct GridResolved {
    pub placements: Vec<(u16, u16)>,
    pub col_tracks: Vec<Length>,
    pub row_tracks: Vec<Length>,
    pub col_mins: Vec<u16>,
    pub row_mins: Vec<u16>,
}

fn normalize_tracks(cols: &[Length], rows: &[Length]) -> (Vec<Length>, Vec<Length>) {
    let c = if cols.is_empty() {
        vec![Length::Auto]
    } else {
        cols.to_vec()
    };
    let r = if rows.is_empty() {
        vec![Length::Auto]
    } else {
        rows.to_vec()
    };
    (c, r)
}

fn clamp_span(span: u16, max: usize) -> usize {
    let s = span.max(1) as usize;
    s.min(max.max(1))
}

fn fits(occ: &[Vec<bool>], r: usize, c: usize, rs: usize, cs: usize, num_cols: usize) -> bool {
    if c + cs > num_cols {
        return false;
    }
    if r + rs > occ.len() {
        return false;
    }
    for rr in r..r + rs {
        for cc in c..c + cs {
            if occ[rr][cc] {
                return false;
            }
        }
    }
    true
}

fn mark_occ(occ: &mut [Vec<bool>], r: usize, c: usize, rs: usize, cs: usize) {
    for rr in r..r + rs {
        for cc in c..c + cs {
            occ[rr][cc] = true;
        }
    }
}

pub(crate) fn resolve_placements(
    items: &[GridItem],
    col_tracks: &[Length],
    mut row_tracks: Vec<Length>,
) -> (Vec<(u16, u16)>, Vec<Length>) {
    let num_cols = col_tracks.len().max(1);
    let num_rows_init = row_tracks.len().max(1);
    let mut occ: Vec<Vec<bool>> = (0..num_rows_init).map(|_| vec![false; num_cols]).collect();
    let mut out: Vec<(u16, u16)> = Vec::with_capacity(items.len());

    for item in items {
        let rs = clamp_span(item.span.0, 4096);
        let cs = clamp_span(item.span.1, num_cols);

        if let Some((r0, c0)) = item.placement {
            let r = r0 as usize;
            let c = c0 as usize;
            while r + rs > occ.len() {
                occ.push(vec![false; num_cols]);
                row_tracks.push(Length::Auto);
            }
            if fits(&occ, r, c, rs, cs, num_cols) {
                mark_occ(&mut occ, r, c, rs, cs);
                out.push((r0, c0));
                continue;
            }
        }

        loop {
            let mut found = None;
            for r in 0..occ.len() {
                for c in 0..num_cols {
                    if fits(&occ, r, c, rs, cs, num_cols) {
                        found = Some((r, c));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            if let Some((r, c)) = found {
                mark_occ(&mut occ, r, c, rs, cs);
                out.push((r as u16, c as u16));
                break;
            }
            occ.push(vec![false; num_cols]);
            row_tracks.push(Length::Auto);
        }
    }

    (out, row_tracks)
}

fn percent_of(available: u16, p: u16) -> u16 {
    if available == u16::MAX {
        return 0;
    }
    ((available as u32).saturating_mul(p.min(100) as u32) / 100).min(u16::MAX as u32) as u16
}

fn intrinsic_main_budget(tracks: &[Length], mins: &[u16], gap: u16) -> u16 {
    let n = tracks.len().max(1);
    let gap_total = gap.saturating_mul(n.saturating_sub(1) as u16);
    let mut s = gap_total as u32;
    for i in 0..n {
        let t = tracks.get(i).copied().unwrap_or(Length::Auto);
        let m = *mins.get(i).unwrap_or(&0);
        let line = match t {
            Length::Px(px) => px.max(m),
            Length::Percent(_) => m,
            Length::Auto | Length::Flex(_) => m,
        };
        s = s.saturating_add(line as u32);
    }
    s.min(u16::MAX as u32).max(1) as u16
}

fn resolve_line_sizes(
    tracks: &[Length],
    mins: &[u16],
    gap: u16,
    available: u16,
    distribute_slack: bool,
) -> Vec<u16> {
    let n = tracks.len().max(1);
    let ml = mins.len().max(n);
    let mut m = vec![0u16; n];
    for i in 0..n {
        m[i] = *mins.get(i).unwrap_or(&0);
    }
    let gap_total = gap.saturating_mul(n.saturating_sub(1) as u16);
    let inner = available.saturating_sub(gap_total);
    let mut sizes = vec![0u16; n];
    let mut flex: Vec<(usize, u16)> = Vec::new();
    let mut used: u32 = 0;

    for i in 0..n {
        let t = tracks.get(i).copied().unwrap_or(Length::Auto);
        match t {
            Length::Px(px) => {
                sizes[i] = px.max(m[i]);
                used = used.saturating_add(sizes[i] as u32);
            }
            Length::Percent(p) => {
                sizes[i] = percent_of(inner, p).max(m[i]);
                used = used.saturating_add(sizes[i] as u32);
            }
            Length::Auto => {
                sizes[i] = m[i];
                used = used.saturating_add(sizes[i] as u32);
            }
            Length::Flex(w) => {
                sizes[i] = m[i];
                flex.push((i, w.max(1)));
                used = used.saturating_add(sizes[i] as u32);
            }
        }
    }

    let inner_u32 = inner as u32;
    if distribute_slack && used < inner_u32 {
        let extra = inner_u32 - used;
        let tw: u32 = flex.iter().map(|(_, w)| *w as u32).sum();
        if tw > 0 {
            let mut rem = extra;
            for (ix, &(i, w)) in flex.iter().enumerate() {
                let add = if ix + 1 == flex.len() {
                    rem
                } else {
                    ((extra as u64) * (w as u64) / (tw as u64)) as u32
                };
                sizes[i] = sizes[i].saturating_add(add as u16);
                rem = rem.saturating_sub(add);
            }
        }
    } else if used > inner_u32 {
        let over = (used - inner_u32) as u16;
        let mut left = over;
        for i in 0..n {
            if left == 0 {
                break;
            }
            let t = tracks.get(i).copied().unwrap_or(Length::Auto);
            if matches!(t, Length::Auto | Length::Flex(_)) {
                let take = left.min(sizes[i]);
                sizes[i] = sizes[i].saturating_sub(take);
                left -= take;
            }
        }
    }

    let _ = ml;
    sizes
}

fn span_main_axis(sizes: &[u16], gap: u16, start: usize, count: usize) -> u16 {
    let end = start.saturating_add(count).min(sizes.len());
    let mut t = 0u16;
    for i in start..end {
        t = t.saturating_add(sizes[i]);
        if i + 1 < end {
            t = t.saturating_add(gap);
        }
    }
    t
}

fn track_start(sizes: &[u16], gap: u16, idx: usize) -> i16 {
    let mut p = 0i16;
    for i in 0..idx.min(sizes.len()) {
        p = p.saturating_add(sizes[i] as i16);
        if i + 1 < sizes.len() {
            p = p.saturating_add(gap as i16);
        }
    }
    p
}

fn distribute_span_deficit(
    sizes: &mut [u16],
    tracks: &[Length],
    start: usize,
    len: usize,
    deficit: u16,
) {
    if deficit == 0 || len == 0 {
        return;
    }
    let mut auto_cols: Vec<usize> = Vec::new();
    let mut flex_cols: Vec<(usize, u16)> = Vec::new();
    for i in start..(start + len).min(sizes.len()) {
        match tracks.get(i).copied().unwrap_or(Length::Auto) {
            Length::Auto => auto_cols.push(i),
            Length::Flex(w) => flex_cols.push((i, w.max(1))),
            _ => {}
        }
    }
    let mut left = deficit;
    if !auto_cols.is_empty() {
        let per = left / auto_cols.len() as u16;
        let mut rem = left % auto_cols.len() as u16;
        for i in auto_cols {
            let add = per.saturating_add(if rem > 0 {
                rem -= 1;
                1
            } else {
                0
            });
            sizes[i] = sizes[i].saturating_add(add);
            left = left.saturating_sub(add);
        }
    }
    if left > 0 && !flex_cols.is_empty() {
        let tw: u32 = flex_cols.iter().map(|(_, w)| *w as u32).sum();
        if tw > 0 {
            let mut rem = left as u32;
            for (ix, (i, w)) in flex_cols.iter().enumerate() {
                let add = if ix + 1 == flex_cols.len() {
                    rem
                } else {
                    ((left as u64) * (*w as u64) / (tw as u64)) as u32
                };
                sizes[*i] = sizes[*i].saturating_add(add as u16);
                rem = rem.saturating_sub(add);
            }
        }
    }
}

pub(crate) fn compute_intrinsic_mins(
    props: &GridProps,
    items: &[GridItem],
    placements: &[(u16, u16)],
    col_tracks: &[Length],
    row_tracks: &[Length],
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (Vec<u16>, Vec<u16>) {
    let num_cols = col_tracks.len();
    let num_rows = row_tracks.len();
    let mut col_mins = vec![0u16; num_cols];
    let mut row_mins = vec![0u16; num_rows];

    let inner_w = max_w.map(|w| {
        w.saturating_sub(props.padding.horizontal())
            .saturating_sub(if props.border { 2 } else { 0 })
    });
    let inner_h = max_h.map(|h| {
        h.saturating_sub(props.padding.vertical())
            .saturating_sub(if props.border { 2 } else { 0 })
    });
    let avail_w = inner_w.unwrap_or(u16::MAX);
    let avail_h = inner_h.unwrap_or(u16::MAX);

    for t_idx in 0..num_cols {
        match col_tracks[t_idx] {
            Length::Px(px) => col_mins[t_idx] = col_mins[t_idx].max(px),
            Length::Percent(p) => {
                col_mins[t_idx] = col_mins[t_idx].max(percent_of(avail_w, p));
            }
            _ => {}
        }
    }
    for t_idx in 0..num_rows {
        match row_tracks[t_idx] {
            Length::Px(px) => row_mins[t_idx] = row_mins[t_idx].max(px),
            Length::Percent(p) => {
                row_mins[t_idx] = row_mins[t_idx].max(percent_of(avail_h, p));
            }
            _ => {}
        }
    }

    for (item, &(r, c)) in items.iter().zip(placements.iter()) {
        let rs = item.span.0.max(1) as usize;
        let cs = item.span.1.max(1) as usize;
        let r = r as usize;
        let c = c as usize;
        if r >= num_rows || c >= num_cols {
            continue;
        }

        let (mw, mh) = min_size_constrained(&item.element, inner_w, inner_h);

        if rs == 1 && cs == 1 {
            if matches!(
                col_tracks.get(c).copied().unwrap_or(Length::Auto),
                Length::Auto
            ) {
                col_mins[c] = col_mins[c].max(mw);
            }
            if matches!(
                row_tracks.get(r).copied().unwrap_or(Length::Auto),
                Length::Px(_) | Length::Percent(_)
            ) {
                row_mins[r] = row_mins[r].max(mh);
            }
            continue;
        }

        if cs > 1 {
            let interior_x = props.gap_x.saturating_mul(cs.saturating_sub(1) as u16);
            let mut sum_w = interior_x;
            for cc in c..(c + cs).min(num_cols) {
                sum_w = sum_w.saturating_add(col_mins[cc]);
            }
            if mw > sum_w {
                distribute_span_deficit(&mut col_mins, col_tracks, c, cs, mw.saturating_sub(sum_w));
            }
        }
    }

    let col_budget =
        inner_w.unwrap_or_else(|| intrinsic_main_budget(col_tracks, &col_mins, props.gap_x));
    let col_slack = max_w.is_some();
    let col_probe = resolve_line_sizes(col_tracks, &col_mins, props.gap_x, col_budget, col_slack);

    for (item, &(r, c)) in items.iter().zip(placements.iter()) {
        let rs = item.span.0.max(1) as usize;
        let cs = item.span.1.max(1) as usize;
        let r = r as usize;
        let c = c as usize;
        if r >= num_rows || c >= num_cols {
            continue;
        }
        if rs != 1 {
            continue;
        }
        if !matches!(
            row_tracks.get(r).copied().unwrap_or(Length::Auto),
            Length::Auto
        ) {
            continue;
        }
        let cs_eff = cs.min(num_cols.saturating_sub(c));
        let cw = span_main_axis(&col_probe, props.gap_x, c, cs_eff);
        let (_, mh) = min_size_constrained(&item.element, Some(cw), inner_h);
        row_mins[r] = row_mins[r].max(mh);
    }

    for (item, &(r, c)) in items.iter().zip(placements.iter()) {
        let rs = item.span.0.max(1) as usize;
        let cs = item.span.1.max(1) as usize;
        let c = c as usize;
        let r = r as usize;
        if r >= num_rows || c >= num_cols || rs <= 1 {
            continue;
        }

        let cs_eff = cs.min(num_cols.saturating_sub(c));
        let cw = span_main_axis(&col_probe, props.gap_x, c, cs_eff);
        let (_, mh) = min_size_constrained(&item.element, Some(cw), inner_h);
        let interior_y = props.gap_y.saturating_mul(rs.saturating_sub(1) as u16);
        let mut sum_h = interior_y;
        for rr in r..(r + rs).min(num_rows) {
            sum_h = sum_h.saturating_add(row_mins[rr]);
        }
        if mh > sum_h {
            distribute_span_deficit(&mut row_mins, row_tracks, r, rs, mh.saturating_sub(sum_h));
        }
    }

    (col_mins, row_mins)
}

pub(crate) fn resolve_grid(
    props: &GridProps,
    items: &[GridItem],
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> GridResolved {
    let (col_tracks, row_tracks) = normalize_tracks(&props.columns, &props.rows);
    let (placements, row_tracks) = resolve_placements(items, &col_tracks, row_tracks);
    let (col_mins, row_mins) = compute_intrinsic_mins(
        props,
        items,
        &placements,
        &col_tracks,
        &row_tracks,
        max_w,
        max_h,
    );
    GridResolved {
        placements,
        col_tracks,
        row_tracks,
        col_mins,
        row_mins,
    }
}

pub(crate) fn measure_grid(
    props: &GridProps,
    items: &[GridItem],
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    if items.is_empty() {
        return chrome_min(props);
    }

    let g = resolve_grid(props, items, max_w, max_h);
    let inner_w_avail = max_w.map(|w| {
        w.saturating_sub(props.padding.horizontal())
            .saturating_sub(if props.border { 2 } else { 0 })
    });
    let inner_h_avail = max_h.map(|h| {
        h.saturating_sub(props.padding.vertical())
            .saturating_sub(if props.border { 2 } else { 0 })
    });

    let ip_w = inner_w_avail
        .unwrap_or_else(|| intrinsic_main_budget(&g.col_tracks, &g.col_mins, props.gap_x));
    let ip_h = inner_h_avail
        .unwrap_or_else(|| intrinsic_main_budget(&g.row_tracks, &g.row_mins, props.gap_y));

    let col_sizes = resolve_line_sizes(
        &g.col_tracks,
        &g.col_mins,
        props.gap_x,
        ip_w,
        max_w.is_some(),
    );
    let row_sizes = resolve_line_sizes(
        &g.row_tracks,
        &g.row_mins,
        props.gap_y,
        ip_h,
        max_h.is_some(),
    );

    let ncols = col_sizes.len().max(1);
    let nrows = row_sizes.len().max(1);
    let mut tw: u32 = props.gap_x as u32 * ncols.saturating_sub(1) as u32;
    for &c in &col_sizes {
        tw = tw.saturating_add(c as u32);
    }
    let mut th: u32 = props.gap_y as u32 * nrows.saturating_sub(1) as u32;
    for &r in &row_sizes {
        th = th.saturating_add(r as u32);
    }

    let mut w = (tw as u16).saturating_add(props.padding.horizontal());
    let mut h = (th as u16).saturating_add(props.padding.vertical());
    if props.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    if let Some(mw) = max_w {
        w = w.min(mw);
    }
    if let Some(mh) = max_h {
        h = h.min(mh);
    }

    (w, h)
}

fn chrome_min(props: &GridProps) -> (u16, u16) {
    let mut w = props.padding.horizontal();
    let mut h = props.padding.vertical();
    if props.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }
    (w, h)
}

fn justify_in_cell(bounds: Rect, child_h: u16, justify: Justify) -> i16 {
    match justify {
        Justify::Start | Justify::SpaceBetween | Justify::SpaceAround | Justify::SpaceEvenly => {
            bounds.y
        }
        Justify::Center => bounds
            .y
            .saturating_add((bounds.h.saturating_sub(child_h) / 2) as i16),
        Justify::End => bounds
            .y
            .saturating_add(bounds.h.saturating_sub(child_h) as i16),
    }
}

pub(crate) fn layout_grid(props: &GridProps, items: &[GridItem], bounds: Rect) -> Vec<Rect> {
    if items.is_empty() {
        return Vec::new();
    }

    let inner = bounds.inner(props.border, props.padding);
    let g = resolve_grid(props, items, Some(inner.w), Some(inner.h));
    let col_sizes = resolve_line_sizes(&g.col_tracks, &g.col_mins, props.gap_x, inner.w, true);
    let row_sizes = resolve_line_sizes(&g.row_tracks, &g.row_mins, props.gap_y, inner.h, true);

    let num_cols = col_sizes.len();
    let num_rows = row_sizes.len();

    let mut out = Vec::with_capacity(items.len());

    for (i, item) in items.iter().enumerate() {
        let (r, c) = g.placements[i];
        let rs = item.span.0.max(1) as usize;
        let cs = item.span.1.max(1) as usize;
        let r = r as usize;
        let c = c as usize;

        if r >= num_rows || c >= num_cols {
            out.push(Rect {
                x: inner.x,
                y: inner.y,
                w: 0,
                h: 0,
            });
            continue;
        }

        let cs_eff = cs.min(num_cols.saturating_sub(c));
        let rs_eff = rs.min(num_rows.saturating_sub(r));

        let x0 = inner
            .x
            .saturating_add(track_start(&col_sizes, props.gap_x, c));
        let y0 = inner
            .y
            .saturating_add(track_start(&row_sizes, props.gap_y, r));
        let cw = span_main_axis(&col_sizes, props.gap_x, c, cs_eff);
        let ch = span_main_axis(&row_sizes, props.gap_y, r, rs_eff);
        let cell = Rect {
            x: x0,
            y: y0,
            w: cw,
            h: ch,
        };

        let (mw, mh) = min_size_constrained(&item.element, Some(cw), Some(ch));
        let req_w = requested_main_axis(&item.element, Axis::Horizontal, None);
        let req_h = requested_main_axis(&item.element, Axis::Vertical, None);
        let fills_w = matches!(req_w, Length::Flex(_) | Length::Percent(_));
        let fills_h = matches!(req_h, Length::Flex(_) | Length::Percent(_));
        let child_w = if props.align == Align::Stretch || fills_w {
            cw
        } else {
            mw.min(cw)
        };
        let child_h = if fills_h { ch } else { mh.min(ch) };

        let cx = align_x(cell, child_w, props.align);
        let cy = justify_in_cell(cell, child_h, props.justify);

        out.push(Rect {
            x: cx,
            y: cy,
            w: child_w,
            h: child_h,
        });
    }

    out
}
