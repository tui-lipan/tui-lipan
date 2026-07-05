use std::cell::Cell as StdCell;
use std::thread_local;

use ratatui::layout::Position;
use ratatui::style::Color as RColor;

use crate::app::ContrastPolicy;
use crate::style::resolve::{Durability, StateLayer, resolve_state_cascade};
use crate::style::{Color, Paint, Style};
use crate::utils::color_contrast::{
    readable_style, readable_style_apca, readable_style_black_or_white, readable_text_color,
    readable_text_color_apca, readable_text_color_black_or_white,
};

use super::colors::from_ratatui_color;

pub(crate) const DEFAULT_SCROLLBAR_THUMB: char = '█';

thread_local! {
    static RENDER_TERMINAL_BG: StdCell<Option<RColor>> = const { StdCell::new(None) };
}

pub(crate) struct TerminalBgScope(Option<RColor>);

impl Drop for TerminalBgScope {
    fn drop(&mut self) {
        RENDER_TERMINAL_BG.with(|slot| slot.set(self.0));
    }
}

pub(crate) fn push_render_terminal_bg(terminal_bg: Option<RColor>) -> TerminalBgScope {
    let prev = RENDER_TERMINAL_BG.with(|slot| {
        let prev = slot.get();
        slot.set(terminal_bg);
        prev
    });
    TerminalBgScope(prev)
}

pub(crate) fn current_render_terminal_bg() -> Option<RColor> {
    RENDER_TERMINAL_BG.with(|slot| slot.get())
}

thread_local! {
    static RENDER_SCREEN_BG: StdCell<Option<ratatui::style::Style>> = const { StdCell::new(None) };
}

/// RAII guard restoring the previous screen-background fill on drop.
pub(crate) struct ScreenBgScope(Option<ratatui::style::Style>);

impl Drop for ScreenBgScope {
    fn drop(&mut self) {
        RENDER_SCREEN_BG.with(|slot| slot.set(self.0));
    }
}

/// Install the resolved root viewport background fill for the current draw.
///
/// `None` leaves the terminal background untouched (the default). When set, the
/// render pass fills every otherwise-untouched cell with this style so the UI
/// reads as a designed surface instead of floating on the host terminal color.
pub(crate) fn push_render_screen_background(style: Option<ratatui::style::Style>) -> ScreenBgScope {
    let prev = RENDER_SCREEN_BG.with(|slot| {
        let prev = slot.get();
        slot.set(style);
        prev
    });
    ScreenBgScope(prev)
}

/// The resolved root viewport background fill for the current draw, if any.
pub(crate) fn current_render_screen_background() -> Option<ratatui::style::Style> {
    RENDER_SCREEN_BG.with(|slot| slot.get())
}

/// Terminal background as a [`Color`], or [`Color::Reset`] when unknown.
///
/// Used as the backdrop for flattening alpha paints before contrast resolution
/// so the contrast policy sees the rendered (post-blend) color rather than the
/// raw pigment.
fn render_terminal_bg_color() -> Color {
    current_render_terminal_bg()
        .map(from_ratatui_color)
        .unwrap_or(Color::Reset)
}

pub(crate) fn remember_cursor_position(
    sink: Option<&StdCell<Option<Position>>>,
    position: Position,
) {
    if let Some(sink) = sink {
        sink.set(Some(position));
    }
}

pub(crate) struct InteractiveStyleState {
    pub is_focused: bool,
    pub is_hovered: bool,
    pub is_disabled: bool,
    pub policy: ContrastPolicy,
}

/// Get scrollbar metrics, using the render context cache if available.
#[inline]
pub(crate) fn resolve_interactive_style(
    base: Style,
    focus_style: Style,
    hover_style: Style,
    disabled_style: Style,
    state: InteractiveStyleState,
) -> Style {
    finalize_style(
        resolve_interactive_style_raw(
            base,
            focus_style,
            hover_style,
            disabled_style,
            state.is_focused,
            state.is_hovered,
            state.is_disabled,
        ),
        None,
        state.policy,
    )
}

/// Resolve the raw interactive style from base + state-dependent patches.
///
/// Priority: disabled > (hover + focus).
/// When disabled, only `disabled_style` is applied so disabled remains terminal.
/// When not disabled, hover is transient and focus is durable: concrete fields
/// follow hover then focus precedence, while hover effects compose afterward.
pub(crate) fn resolve_interactive_style_raw(
    base: Style,
    focus_style: Style,
    hover_style: Style,
    disabled_style: Style,
    is_focused: bool,
    is_hovered: bool,
    is_disabled: bool,
) -> Style {
    if is_disabled {
        return resolve_state_cascade(
            base,
            &[StateLayer {
                style: &disabled_style,
                durability: Durability::Durable,
            }],
        );
    }

    match (is_hovered, is_focused) {
        (true, true) => resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover_style,
                    durability: Durability::Transient,
                },
                StateLayer {
                    style: &focus_style,
                    durability: Durability::Durable,
                },
            ],
        ),
        (true, false) => resolve_state_cascade(
            base,
            &[StateLayer {
                style: &hover_style,
                durability: Durability::Transient,
            }],
        ),
        (false, true) => resolve_state_cascade(
            base,
            &[StateLayer {
                style: &focus_style,
                durability: Durability::Durable,
            }],
        ),
        (false, false) => base,
    }
}

/// Flatten `style.fg` / `style.bg` against the terminal backdrop so contrast
/// calculations operate on the rendered (post-blend) colors, not raw pigments.
///
/// Returns `(preferred_fg, resolved_bg)` ready to feed into the WCAG/APCA
/// pickers. The original `style.bg` is left untouched so the renderer still
/// performs the actual alpha blending downstream.
fn resolved_contrast_inputs(fg: Paint, bg: Paint) -> (Color, Color) {
    let terminal_bg = render_terminal_bg_color();
    let bg_resolved = bg.flatten_over(terminal_bg);
    let fg_resolved = fg.flatten_over(bg_resolved);
    (fg_resolved, bg_resolved)
}

fn readable_style_frame_memo(mut style: Style) -> Style {
    use crate::backend::ratatui_backend::glyph_paint_cache::active_paint_memo;
    if let Some(rc) = active_paint_memo() {
        style = style.resolve_color_transforms();
        style.contrast_policy = None;
        if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
            let (fg_resolved, bg_resolved) = resolved_contrast_inputs(fg, bg);
            let key = (Some(fg_resolved), bg_resolved);
            let resolved = {
                let mut m = rc.borrow_mut();
                *m.readable_wcag_fg
                    .entry(key)
                    .or_insert_with(|| readable_text_color(Some(fg_resolved), bg_resolved))
            };
            style.fg = Some(Paint::Solid(resolved));
        }
        style
    } else {
        readable_style(style)
    }
}

fn readable_style_black_or_white_frame_memo(mut style: Style) -> Style {
    use crate::backend::ratatui_backend::glyph_paint_cache::active_paint_memo;
    if let Some(rc) = active_paint_memo() {
        style = style.resolve_color_transforms();
        style.contrast_policy = None;
        if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
            let (fg_resolved, bg_resolved) = resolved_contrast_inputs(fg, bg);
            let key = (Some(fg_resolved), bg_resolved);
            let resolved = {
                let mut m = rc.borrow_mut();
                *m.readable_bw_fg.entry(key).or_insert_with(|| {
                    readable_text_color_black_or_white(Some(fg_resolved), bg_resolved)
                })
            };
            style.fg = Some(Paint::Solid(resolved));
        }
        style
    } else {
        readable_style_black_or_white(style)
    }
}

fn readable_style_apca_frame_memo(mut style: Style) -> Style {
    use crate::backend::ratatui_backend::glyph_paint_cache::active_paint_memo;
    if let Some(rc) = active_paint_memo() {
        style = style.resolve_color_transforms();
        style.contrast_policy = None;
        if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
            let (fg_resolved, bg_resolved) = resolved_contrast_inputs(fg, bg);
            let key = (Some(fg_resolved), bg_resolved);
            let resolved = {
                let mut m = rc.borrow_mut();
                *m.readable_apca_fg
                    .entry(key)
                    .or_insert_with(|| readable_text_color_apca(Some(fg_resolved), bg_resolved))
            };
            style.fg = Some(Paint::Solid(resolved));
        }
        style
    } else {
        readable_style_apca(style)
    }
}

pub(crate) fn finalize_style(raw: Style, backdrop: Option<Color>, policy: ContrastPolicy) -> Style {
    let mut style = Style {
        contrast_policy: None,
        ..raw
    }
    .resolve_color_transforms();

    if matches!(style.fg, Some(Paint::Solid(Color::Transparent)))
        && let Some(resolved_backdrop) = backdrop
    {
        let backdrop_paint = Paint::Solid(resolved_backdrop);
        style.fg = if let Some(transform) = style.fg_transform {
            Some(transform.apply_paint_with_backdrop(backdrop_paint, style.bg))
        } else {
            Some(backdrop_paint)
        };
        style.fg_transform = None;
    }

    let policy = raw.contrast_policy.unwrap_or(policy);
    if matches!(policy, ContrastPolicy::Off) {
        return style;
    }

    // Flatten alpha paints against the supplied style backdrop before contrast
    // resolution. Fall back to the terminal backdrop when no containing style
    // background is known. The policy must evaluate against the colors that
    // will actually be rendered, not the raw pigments. The original `style.bg`
    // is preserved on the way out so the renderer still blends correctly.
    let original_bg = style.bg;
    let terminal_bg = render_terminal_bg_color();
    let style_backdrop = backdrop.unwrap_or(terminal_bg);
    let bg_resolved = style.bg.map(|bg| bg.flatten_over(style_backdrop));
    let fg_resolved = style.fg.map(|fg| {
        let backdrop_for_fg = bg_resolved.unwrap_or(style_backdrop);
        fg.flatten_over(backdrop_for_fg)
    });
    let contrast_input = Style {
        fg: fg_resolved.map(Paint::Solid),
        bg: bg_resolved.map(Paint::Solid),
        ..style
    };

    let mut resolved = match policy {
        ContrastPolicy::Off => unreachable!(),
        ContrastPolicy::Wcag => readable_style_frame_memo(contrast_input),
        ContrastPolicy::BlackOrWhite => readable_style_black_or_white_frame_memo(contrast_input),
        ContrastPolicy::Apca => readable_style_apca_frame_memo(contrast_input),
    };
    resolved.bg = original_bg;
    resolved
}

pub(crate) fn style_backdrop(style: Style) -> Option<Color> {
    style
        .bg
        .filter(|bg| !bg.is_transparent_paint() && !bg.is_backdrop_sentinel())
        .map(|bg| bg.flatten_over(render_terminal_bg_color()))
}
