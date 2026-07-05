use ratatui::widgets::Borders;

use crate::backend::ratatui_backend::common::{
    ClipBounds, calculate_visible_borders, style_paints_bg, to_ratatui_border_set, to_ratatui_style,
};
use crate::style::resolve::{resolve_accent_style, resolve_base_style, resolve_muted_style};
use crate::style::{BorderStyle, Rect, Style, Theme};
use crate::widgets::internal::{
    PositionedFragment, PositionedMessage, SequenceDiagramNode, autonumber_rect,
};
use crate::widgets::{
    ActorKind, FragmentKind, MessageStyle, SequenceDiagramVariant, SequenceItemPath,
};

pub(crate) fn render_sequence_diagram(
    f: &mut ratatui::Frame<'_>,
    node: &SequenceDiagramNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
    mouse_pos: Option<(u16, u16)>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let base_style = resolve_base_style(theme, node.style);
    let diagram_theme = &node.theme;
    let participant_style = resolve_accent_style(theme, diagram_theme.participant_style);
    let lifeline_style = resolve_muted_style(theme, diagram_theme.lifeline.style);
    let message_label_style = resolve_base_style(theme, diagram_theme.message_label_style);
    let note_style = resolve_muted_style(theme, diagram_theme.note_style);
    let activation_style = resolve_accent_style(theme, diagram_theme.activation.style);
    let autonumber_style = resolve_accent_style(theme, diagram_theme.autonumber.style);
    let hover_style = (!diagram_theme.hover_style.is_empty())
        .then(|| resolve_accent_style(theme, diagram_theme.hover_style));

    let bounds = ClipBounds::from_rrect(f.area());
    let outer_clip_rect = rect.intersection(&clip_rect);
    if !outer_clip_rect.is_empty() && (style_paints_bg(base_style) || node.border) {
        let outer_clip = ClipBounds::from_rect(outer_clip_rect);
        let mut paint = PaintCtx {
            f,
            clip: &outer_clip,
            bounds: &bounds,
        };
        if style_paints_bg(base_style) {
            fill_rect(&mut paint, rect, to_ratatui_style(base_style));
        }
        if node.border {
            let borders = calculate_visible_borders(rect, Some(clip_rect));
            draw_outer_border(&mut paint, rect, borders, node.border_style, base_style);
        }
    }

    let inner = node.content_rect(rect);
    if inner.w == 0 || inner.h == 0 || node.output.width == 0 || node.output.height == 0 {
        return;
    }
    let content_clip = inner.intersection(&clip_rect);
    if content_clip.is_empty() {
        return;
    }

    let hovered_path = mouse_pos
        .and_then(|(mx, my)| node.local_content_point(rect, mx as i16, my as i16))
        .and_then(|(local_x, local_y)| node.hit_test(local_x, local_y));
    let clip = ClipBounds::from_rect(content_clip);
    let mut paint = PaintCtx {
        f,
        clip: &clip,
        bounds: &bounds,
    };

    let lifeline_rstyle = to_ratatui_style(base_style.patch(lifeline_style));
    let lifeline_glyph = diagram_theme.lifeline.glyph;
    let message_glyphs = RenderMessageGlyphs {
        line: diagram_theme.message_glyphs.line,
        dashed_line: diagram_theme.message_glyphs.dashed_line,
        arrow_filled_right: diagram_theme.message_glyphs.arrow_filled_right,
        arrow_filled_left: diagram_theme.message_glyphs.arrow_filled_left,
        arrow_open_right: diagram_theme.message_glyphs.arrow_open_right,
        arrow_open_left: diagram_theme.message_glyphs.arrow_open_left,
        lost_terminator: diagram_theme.message_glyphs.lost_terminator,
        self_loop_top_right: diagram_theme.message_glyphs.self_loop_top_right,
        self_loop_bottom_right: diagram_theme.message_glyphs.self_loop_bottom_right,
    };
    let structural_glyphs = RenderStructuralGlyphs::from_theme(
        diagram_theme.message_glyphs.line,
        diagram_theme.lifeline.glyph,
    );
    for lifeline in &node.output.lifelines {
        let mut y = lifeline.y1;
        while y <= lifeline.y2 {
            draw_char(
                &mut paint,
                inner.x.saturating_add(lifeline.x),
                inner.y.saturating_add(y),
                lifeline_glyph,
                lifeline_rstyle,
            );
            y = y.saturating_add(1);
            if y == i16::MAX {
                break;
            }
        }
    }

    for fragment in &node.output.fragments {
        let style = resolve_fragment_style(
            theme,
            base_style,
            diagram_theme.fragment_style(fragment.kind),
            &hover_style,
            hovered_path.as_ref(),
            fragment,
        );
        draw_fragment(
            &mut paint,
            inner,
            fragment,
            style,
            diagram_theme.fragment_glyphs.border,
            diagram_theme.fragment_glyphs.branch_separator,
        );
    }

    for activation in &node.output.activations {
        let style = to_ratatui_style(base_style.patch(activation_style));
        draw_activation(
            &mut paint,
            offset_rect(inner, activation.rect),
            diagram_theme.activation.fill_glyph,
            diagram_theme.activation.fill_background,
            style,
        );
    }

    for (index, message) in node.output.messages.iter().enumerate() {
        let path = message_path_for_index(message.path.clone(), index);
        let line_style = resolve_message_line_style(
            theme,
            base_style,
            diagram_theme.message_style(message.style),
            &hover_style,
            hovered_path.as_ref(),
            &path,
            message,
        );
        draw_message(
            &mut paint,
            inner,
            message,
            node.variant,
            &message_glyphs,
            &structural_glyphs,
            to_ratatui_style(line_style),
        );

        let label_style = resolve_item_style(
            base_style.patch(message_label_style),
            &hover_style,
            hovered_path.as_ref(),
            &path,
            message.label_style,
        );
        draw_multiline_text(
            &mut paint,
            offset_rect(inner, message.label_rect),
            &message.display_lines,
            to_ratatui_style(label_style),
        );
    }

    for note in &node.output.notes {
        let style = resolve_item_style(
            base_style.patch(note_style),
            &hover_style,
            hovered_path.as_ref(),
            &note.path,
            note.style,
        );
        let style = to_ratatui_style(style);
        let rect = offset_rect(inner, note.rect);
        fill_rect(&mut paint, rect, style);
        draw_box(&mut paint, rect, diagram_theme.note_border, style);
        draw_multiline_text(
            &mut paint,
            offset_rect(inner, note.label_rect),
            &note.lines,
            style,
        );
    }

    for divider in &node.output.dividers {
        let mut style = base_style.patch(lifeline_style);
        style = patch_hover(style, &hover_style, hovered_path.as_ref(), &divider.path);
        let style = to_ratatui_style(style);
        let rect = offset_rect(inner, divider.rect);
        draw_horizontal(
            &mut paint,
            rect.x,
            rect.y,
            rect.w,
            message_glyphs.line,
            style,
        );
        draw_rect_text(
            &mut paint,
            offset_rect(inner, divider.label_rect),
            &divider.label,
            style,
        );
    }

    if let Some(numbers) = node.output.auto_numbers.as_ref() {
        for (path, number) in numbers {
            if let Some(local_rect) = autonumber_rect(
                path,
                *number,
                &node.output.messages,
                &diagram_theme.autonumber.format,
            ) {
                let text = diagram_theme
                    .autonumber
                    .format
                    .replace("{n}", &number.to_string());
                let rect = offset_rect(inner, local_rect);
                let mut style = base_style.patch(autonumber_style);
                style = patch_hover(style, &hover_style, hovered_path.as_ref(), path);
                draw_rect_text(&mut paint, rect, &text, to_ratatui_style(style));
            }
        }
    }

    for participant in &node.output.participants {
        let mut style = base_style.patch(participant_style);
        style = patch_hover(
            style,
            &hover_style,
            hovered_path.as_ref(),
            &participant.path,
        );
        let style = to_ratatui_style(style);
        draw_participant(
            &mut paint,
            ParticipantPaint {
                origin: inner,
                rect: participant.rect,
                label_rect: participant.label_rect,
                kind: participant.kind,
                variant: node.variant,
                label: &participant.display_label,
                border_style: diagram_theme.participant_border,
                structural_glyphs,
                style,
            },
        );
        if let Some(bottom_rect) = participant.bottom_rect {
            let delta = bottom_rect.y.saturating_sub(participant.rect.y);
            let bottom_label = Rect {
                y: participant.label_rect.y.saturating_add(delta),
                ..participant.label_rect
            };
            draw_participant(
                &mut paint,
                ParticipantPaint {
                    origin: inner,
                    rect: bottom_rect,
                    label_rect: bottom_label,
                    kind: participant.kind,
                    variant: node.variant,
                    label: &participant.display_label,
                    border_style: diagram_theme.participant_border,
                    structural_glyphs,
                    style,
                },
            );
        }
    }
}

trait PatchOpt {
    fn patch_opt(self, other: Option<Style>) -> Self;
}

impl PatchOpt for Style {
    fn patch_opt(self, other: Option<Style>) -> Self {
        other.map_or(self, |style| self.patch(style))
    }
}

struct PaintCtx<'a, 'b, 'c> {
    f: &'a mut ratatui::Frame<'b>,
    clip: &'c ClipBounds,
    bounds: &'c ClipBounds,
}

#[derive(Clone, Copy)]
struct RenderMessageGlyphs {
    line: char,
    dashed_line: char,
    arrow_filled_right: char,
    arrow_filled_left: char,
    arrow_open_right: char,
    arrow_open_left: char,
    lost_terminator: char,
    self_loop_top_right: char,
    self_loop_bottom_right: char,
}

#[derive(Clone, Copy)]
struct RenderStructuralGlyphs {
    minimal_rule: char,
    minimal_join: char,
    minimal_from_joint: char,
    minimal_to_joint: char,
}

impl RenderStructuralGlyphs {
    fn from_theme(line: char, lifeline: char) -> Self {
        let ascii = line.is_ascii() && lifeline.is_ascii();
        Self {
            minimal_rule: line,
            minimal_join: if ascii { '+' } else { '┬' },
            minimal_from_joint: if ascii { '+' } else { '├' },
            minimal_to_joint: if ascii { '+' } else { '┤' },
        }
    }
}

fn patch_hover(
    style: Style,
    hover_style: &Option<Style>,
    hovered: Option<&SequenceItemPath>,
    path: &SequenceItemPath,
) -> Style {
    if hovered == Some(path)
        && let Some(hover_style) = hover_style
    {
        return style.patch(*hover_style);
    }
    style
}

fn resolve_item_style(
    base: Style,
    hover_style: &Option<Style>,
    hovered: Option<&SequenceItemPath>,
    path: &SequenceItemPath,
    item_override: Option<Style>,
) -> Style {
    patch_hover(base, hover_style, hovered, path).patch_opt(item_override)
}

fn resolve_message_line_style(
    global_theme: &Theme,
    base_style: Style,
    message_style: Style,
    hover_style: &Option<Style>,
    hovered: Option<&SequenceItemPath>,
    path: &SequenceItemPath,
    message: &PositionedMessage,
) -> Style {
    let style = resolve_muted_style(global_theme, message_style);
    resolve_item_style(
        base_style.patch(style),
        hover_style,
        hovered,
        path,
        message.line_style,
    )
}

fn resolve_fragment_style(
    global_theme: &Theme,
    base_style: Style,
    fragment_style: Style,
    hover_style: &Option<Style>,
    hovered: Option<&SequenceItemPath>,
    fragment: &PositionedFragment,
) -> Style {
    let style = resolve_muted_style(global_theme, fragment_style);
    resolve_item_style(
        base_style.patch(style),
        hover_style,
        hovered,
        &fragment.path,
        fragment.style,
    )
}

fn offset_rect(origin: Rect, rect: Rect) -> Rect {
    Rect {
        x: origin.x.saturating_add(rect.x),
        y: origin.y.saturating_add(rect.y),
        w: rect.w,
        h: rect.h,
    }
}

fn draw_fragment(
    paint: &mut PaintCtx<'_, '_, '_>,
    origin: Rect,
    fragment: &PositionedFragment,
    style: Style,
    border_style: BorderStyle,
    branch_separator: char,
) {
    let style = to_ratatui_style(style);
    let rect = offset_rect(origin, fragment.rect);
    if matches!(fragment.kind, FragmentKind::Rect) {
        fill_rect(paint, rect, style);
    }
    draw_box(paint, rect, border_style, style);
    draw_rect_text(
        paint,
        offset_rect(origin, fragment.label_rect),
        &fragment.header_label,
        style,
    );
    for branch in &fragment.branches {
        let y = origin.y.saturating_add(branch.y);
        draw_horizontal(
            paint,
            rect.x.saturating_add(1),
            y,
            rect.w.saturating_sub(2),
            branch_separator,
            style,
        );
        draw_rect_text(
            paint,
            offset_rect(origin, branch.label_rect),
            &branch.label,
            style,
        );
    }
}

struct ParticipantPaint<'a> {
    origin: Rect,
    rect: Rect,
    label_rect: Rect,
    kind: ActorKind,
    variant: SequenceDiagramVariant,
    label: &'a str,
    border_style: crate::style::BorderStyle,
    structural_glyphs: RenderStructuralGlyphs,
    style: ratatui::style::Style,
}

fn draw_participant(paint: &mut PaintCtx<'_, '_, '_>, participant: ParticipantPaint<'_>) {
    let rect = offset_rect(participant.origin, participant.rect);
    match participant.variant {
        SequenceDiagramVariant::Boxed => match participant.kind {
            ActorKind::Participant => {
                fill_rect(paint, rect, participant.style);
                draw_box(paint, rect, participant.border_style, participant.style);
            }
            ActorKind::Actor => {
                let center = rect.x.saturating_add((rect.w / 2) as i16);
                draw_char(paint, center, rect.y, '○', participant.style);
                draw_char(
                    paint,
                    center,
                    rect.y.saturating_add(1),
                    '│',
                    participant.style,
                );
                draw_char(
                    paint,
                    center.saturating_sub(1),
                    rect.y.saturating_add(1),
                    '─',
                    participant.style,
                );
                draw_char(
                    paint,
                    center.saturating_add(1),
                    rect.y.saturating_add(1),
                    '─',
                    participant.style,
                );
                draw_char(
                    paint,
                    center.saturating_sub(1),
                    rect.y.saturating_add(2),
                    '╱',
                    participant.style,
                );
                draw_char(
                    paint,
                    center.saturating_add(1),
                    rect.y.saturating_add(2),
                    '╲',
                    participant.style,
                );
            }
        },
        SequenceDiagramVariant::Minimal => {
            let center = rect.x.saturating_add((rect.w / 2) as i16);
            let tee_y = rect.y.saturating_add(1);
            draw_horizontal(
                paint,
                rect.x,
                tee_y,
                rect.w,
                participant.structural_glyphs.minimal_rule,
                participant.style,
            );
            draw_char(
                paint,
                center,
                tee_y,
                participant.structural_glyphs.minimal_join,
                participant.style,
            );
        }
    }
    draw_rect_text(
        paint,
        offset_rect(participant.origin, participant.label_rect),
        participant.label,
        participant.style,
    );
}

fn draw_message(
    paint: &mut PaintCtx<'_, '_, '_>,
    origin: Rect,
    message: &PositionedMessage,
    variant: SequenceDiagramVariant,
    glyphs: &RenderMessageGlyphs,
    structural_glyphs: &RenderStructuralGlyphs,
    style: ratatui::style::Style,
) {
    let y = origin.y.saturating_add(message.y);
    let from = origin.x.saturating_add(message.from_x);
    let to = origin.x.saturating_add(message.to_x);
    if message.from_x == message.to_x {
        let right = from
            .saturating_add(message.line_rect.w as i16)
            .saturating_sub(1);
        draw_horizontal(
            paint,
            from,
            y,
            message.line_rect.w.saturating_sub(1),
            glyphs.line,
            style,
        );
        draw_char(paint, right, y, glyphs.self_loop_top_right, style);
        draw_horizontal(
            paint,
            from.saturating_add(1),
            y.saturating_add(1),
            message.line_rect.w.saturating_sub(2),
            glyphs.line,
            style,
        );
        draw_char(
            paint,
            right,
            y.saturating_add(1),
            glyphs.self_loop_bottom_right,
            style,
        );
        draw_arrow_head(
            paint,
            from,
            y.saturating_add(1),
            false,
            message.style,
            glyphs,
            style,
        );
        return;
    }

    if matches!(variant, SequenceDiagramVariant::Minimal) {
        draw_minimal_message(
            paint,
            MinimalMessagePaint {
                from,
                to,
                y,
                style_kind: message.style,
            },
            glyphs,
            structural_glyphs,
            style,
        );
        return;
    }

    let left = from.min(to);
    let right = from.max(to);
    let dashed = matches!(
        message.style,
        MessageStyle::SyncReply | MessageStyle::AsyncReply
    );
    let glyph = if dashed {
        glyphs.dashed_line
    } else {
        glyphs.line
    };
    for x in left..=right {
        if x != to {
            draw_char(paint, x, y, glyph, style);
        }
    }
    draw_arrow_head(paint, to, y, to > from, message.style, glyphs, style);
}

#[derive(Clone, Copy)]
struct MinimalMessagePaint {
    from: i16,
    to: i16,
    y: i16,
    style_kind: MessageStyle,
}

fn draw_minimal_message(
    paint: &mut PaintCtx<'_, '_, '_>,
    message: MinimalMessagePaint,
    glyphs: &RenderMessageGlyphs,
    structural_glyphs: &RenderStructuralGlyphs,
    style: ratatui::style::Style,
) {
    let from = message.from;
    let to = message.to;
    let y = message.y;
    let left = from.min(to);
    let right = from.max(to);
    let dashed = matches!(
        message.style_kind,
        MessageStyle::SyncReply | MessageStyle::AsyncReply
    );
    let glyph = if dashed {
        glyphs.dashed_line
    } else {
        glyphs.line
    };
    if to > from {
        draw_char(paint, from, y, structural_glyphs.minimal_from_joint, style);
        for x in from.saturating_add(1)..to {
            draw_char(paint, x, y, glyph, style);
        }
        draw_arrow_head(paint, to, y, true, message.style_kind, glyphs, style);
    } else {
        draw_arrow_head(paint, to, y, false, message.style_kind, glyphs, style);
        for x in left.saturating_add(1)..right {
            draw_char(paint, x, y, glyph, style);
        }
        draw_char(paint, from, y, structural_glyphs.minimal_to_joint, style);
    }
}

fn draw_activation(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    fill_glyph: char,
    fill_background: bool,
    style: ratatui::style::Style,
) {
    if fill_background {
        fill_rect(paint, rect, style);
    }
    let center = rect.x.saturating_add((rect.w / 2) as i16);
    for y in rect.y..rect.y.saturating_add(rect.h as i16) {
        draw_char(paint, center, y, fill_glyph, style);
    }
}

fn draw_arrow_head(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    right: bool,
    style_kind: MessageStyle,
    glyphs: &RenderMessageGlyphs,
    style: ratatui::style::Style,
) {
    let ch = match (right, style_kind) {
        (_, MessageStyle::Lost) => glyphs.lost_terminator,
        (true, MessageStyle::Async | MessageStyle::AsyncReply | MessageStyle::Open) => {
            glyphs.arrow_open_right
        }
        (false, MessageStyle::Async | MessageStyle::AsyncReply | MessageStyle::Open) => {
            glyphs.arrow_open_left
        }
        (true, _) => glyphs.arrow_filled_right,
        (false, _) => glyphs.arrow_filled_left,
    };
    draw_char(paint, x, y, ch, style);
}

fn message_path_for_index(path: SequenceItemPath, index: usize) -> SequenceItemPath {
    match path {
        SequenceItemPath::Message(_) => SequenceItemPath::Message(index),
        SequenceItemPath::SelfMessage(_) => SequenceItemPath::SelfMessage(index),
        other => other,
    }
}

fn draw_rect_text(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    text: &str,
    style: ratatui::style::Style,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    draw_text_clipped(paint, rect.x, rect.y, text, rect.w as usize, style);
}

fn draw_multiline_text(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    lines: &[std::sync::Arc<str>],
    style: ratatui::style::Style,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    for (offset, line) in lines.iter().take(rect.h as usize).enumerate() {
        draw_text_clipped(
            paint,
            rect.x,
            rect.y.saturating_add(offset as i16),
            line,
            rect.w as usize,
            style,
        );
    }
}

fn fill_rect(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    let bottom = rect.y.saturating_add(rect.h as i16);
    let right = rect.x.saturating_add(rect.w as i16);
    for y in rect.y..bottom {
        for x in rect.x..right {
            draw_char(paint, x, y, ' ', style);
        }
    }
}

fn draw_outer_border(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    borders: Borders,
    border_style: crate::style::BorderStyle,
    style: Style,
) {
    if rect.w < 2 || rect.h < 2 || borders.is_empty() {
        return;
    }
    let set = to_ratatui_border_set(border_style).unwrap_or(ratatui::symbols::border::PLAIN);
    let style = to_ratatui_style(style);
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    if borders.contains(Borders::TOP) {
        draw_horizontal_symbol(
            paint,
            left.saturating_add(1),
            top,
            right,
            set.horizontal_top,
            style,
        );
    }
    if borders.contains(Borders::BOTTOM) {
        draw_horizontal_symbol(
            paint,
            left.saturating_add(1),
            bottom,
            right,
            set.horizontal_bottom,
            style,
        );
    }
    if borders.contains(Borders::LEFT) {
        for y in top.saturating_add(1)..bottom {
            draw_symbol(paint, left, y, set.vertical_left, style);
        }
    }
    if borders.contains(Borders::RIGHT) {
        for y in top.saturating_add(1)..bottom {
            draw_symbol(paint, right, y, set.vertical_right, style);
        }
    }
    if borders.contains(Borders::TOP) && borders.contains(Borders::LEFT) {
        draw_symbol(paint, left, top, set.top_left, style);
    }
    if borders.contains(Borders::TOP) && borders.contains(Borders::RIGHT) {
        draw_symbol(paint, right, top, set.top_right, style);
    }
    if borders.contains(Borders::BOTTOM) && borders.contains(Borders::LEFT) {
        draw_symbol(paint, left, bottom, set.bottom_left, style);
    }
    if borders.contains(Borders::BOTTOM) && borders.contains(Borders::RIGHT) {
        draw_symbol(paint, right, bottom, set.bottom_right, style);
    }
}

fn draw_box(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    border_style: crate::style::BorderStyle,
    style: ratatui::style::Style,
) {
    if rect.w < 2 || rect.h < 2 {
        return;
    }
    let set = to_ratatui_border_set(border_style).unwrap_or(ratatui::symbols::border::PLAIN);
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    draw_symbol(paint, left, top, set.top_left, style);
    draw_symbol(paint, right, top, set.top_right, style);
    draw_symbol(paint, left, bottom, set.bottom_left, style);
    draw_symbol(paint, right, bottom, set.bottom_right, style);
    draw_horizontal_symbol(
        paint,
        left.saturating_add(1),
        top,
        right,
        set.horizontal_top,
        style,
    );
    draw_horizontal_symbol(
        paint,
        left.saturating_add(1),
        bottom,
        right,
        set.horizontal_bottom,
        style,
    );
    for y in top.saturating_add(1)..bottom {
        draw_symbol(paint, left, y, set.vertical_left, style);
        draw_symbol(paint, right, y, set.vertical_right, style);
    }
}

fn draw_horizontal(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    width: u16,
    ch: char,
    style: ratatui::style::Style,
) {
    for offset in 0..width {
        draw_char(paint, x.saturating_add(offset as i16), y, ch, style);
    }
}

fn draw_horizontal_symbol(
    paint: &mut PaintCtx<'_, '_, '_>,
    start: i16,
    y: i16,
    end_exclusive: i16,
    symbol: &str,
    style: ratatui::style::Style,
) {
    for x in start..end_exclusive {
        draw_symbol(paint, x, y, symbol, style);
    }
}

fn draw_text_clipped(
    paint: &mut PaintCtx<'_, '_, '_>,
    mut x: i16,
    y: i16,
    text: &str,
    available: usize,
    style: ratatui::style::Style,
) {
    for ch in text.chars().take(available) {
        draw_char(paint, x, y, ch, style);
        x = x.saturating_add(1);
    }
}

fn draw_char(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    ch: char,
    style: ratatui::style::Style,
) {
    let x = i32::from(x);
    let y = i32::from(y);
    if !paint.clip.contains(x, y) || !paint.bounds.contains(x, y) {
        return;
    }
    let Some(cell) = paint.f.buffer_mut().cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_char(ch).set_style(style);
}

fn draw_symbol(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    symbol: &str,
    style: ratatui::style::Style,
) {
    let x = i32::from(x);
    let y = i32::from(y);
    if !paint.clip.contains(x, y) || !paint.bounds.contains(x, y) {
        return;
    }
    let Some(cell) = paint.f.buffer_mut().cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_symbol(symbol).set_style(style);
}
