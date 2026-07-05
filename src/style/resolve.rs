//! Runtime style-slot resolution.

use crate::style::{Color, ColorTransform, Paint, Style, StyleSlot, Theme, ThemeRole};

/// Whether a state layer is stable state styling or a short-lived visual affordance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Durability {
    /// Short-lived state such as pointer hover whose effects may compose over durable state.
    Transient,
    /// Stable state such as focus or selection that keeps normal cascade precedence.
    Durable,
}

/// A state style layer with its cascade durability metadata.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StateLayer<'a> {
    pub style: &'a Style,
    pub durability: Durability,
}

/// Resolve state layers while allowing transient visual effects to compose over durable colors.
///
/// State layers are supplied in normal state-precedence order (for example hover, then focus).
/// Concrete fields continue to follow [`Style::patch`] precedence in that order. For transient
/// layers, compositor/effect fields are deferred until after concrete fields have resolved, so a
/// hover transform can still apply over a focused background without changing `Style::patch`.
pub(crate) fn resolve_state_cascade(base: Style, layers: &[StateLayer<'_>]) -> Style {
    let mut resolved = base;
    let mut transient_effects = Vec::new();

    for layer in layers {
        match layer.durability {
            Durability::Durable => {
                resolved = resolved.patch(*layer.style);
            }
            Durability::Transient => {
                resolved = resolved.patch(concrete_state_style(*layer.style));
                let effects = transient_effect_style(*layer.style);
                if !effects.is_empty() {
                    transient_effects.push(effects);
                }
            }
        }
    }

    for effects in transient_effects {
        resolved = resolved.patch(effects);
    }

    resolved
}

fn concrete_state_style(style: Style) -> Style {
    Style {
        fg: style.fg,
        bg: style.bg,
        fg_transform: None,
        bg_transform: None,
        contrast_policy: style.contrast_policy,
        bold: style.bold,
        dim: style.dim,
        italic: style.italic,
        underline: style.underline,
        reverse: style.reverse,
        strikethrough: style.strikethrough,
        underline_color: style.underline_color,
        dim_amount: None,
        tint: None,
    }
}

fn transient_effect_style(style: Style) -> Style {
    Style {
        fg: None,
        bg: None,
        fg_transform: style.fg_transform,
        bg_transform: style.bg_transform,
        contrast_policy: None,
        bold: None,
        dim: None,
        italic: None,
        underline: None,
        reverse: None,
        strikethrough: None,
        underline_color: None,
        dim_amount: style.dim_amount,
        tint: style.tint,
    }
}

/// Resolve a themed style slot against a semantic role of the active theme.
///
/// This is the single merge point for state styles that opt into `StyleSlot`:
/// `Inherit` returns the theme role, `Extend` patches the explicit style on top
/// of the role, and `Replace` ignores the theme role entirely.
pub fn resolve_slot(theme: &Theme, role: ThemeRole, slot: &StyleSlot) -> Style {
    match *slot {
        StyleSlot::Inherit => theme.role(role),
        StyleSlot::Extend(style) => theme.role(role).patch(style),
        StyleSlot::Replace(style) => style,
    }
}

/// Fill unset fields in `style` from `defaults`.
pub(crate) fn resolve_style_defaults(style: Style, defaults: Style) -> Style {
    Style {
        fg: style.fg.or(defaults.fg),
        bg: style.bg.or(defaults.bg),
        fg_transform: style.fg_transform.or(defaults.fg_transform),
        bg_transform: style.bg_transform.or(defaults.bg_transform),
        contrast_policy: style.contrast_policy.or(defaults.contrast_policy),
        bold: style.bold.or(defaults.bold),
        dim: style.dim.or(defaults.dim),
        italic: style.italic.or(defaults.italic),
        underline: style.underline.or(defaults.underline),
        reverse: style.reverse.or(defaults.reverse),
        strikethrough: style.strikethrough.or(defaults.strikethrough),
        underline_color: style.underline_color.or(defaults.underline_color),
        dim_amount: style.dim_amount.or(defaults.dim_amount),
        tint: style.tint.or(defaults.tint),
    }
}

pub(crate) fn resolve_hex_pending_edit_style(theme: &Theme, style: Style) -> Style {
    if !style.is_empty() {
        return style;
    }

    Style::new()
        .bg(theme
            .text_selection
            .bg
            .unwrap_or(theme.primary.fg.unwrap_or(Paint::Solid(Color::Blue))))
        .fg(theme
            .text_selection
            .fg
            .or(theme.primary.bg)
            .unwrap_or(Paint::Solid(Color::White)))
        .bold()
}

pub(crate) fn resolve_scrollbar_theme(
    theme: &Theme,
    thumb_style: Option<Style>,
    thumb_focus_style: Option<Style>,
    track_style: Option<Style>,
) -> (Option<Style>, Option<Style>, Option<Style>) {
    let thumb_style = thumb_style.or_else(|| {
        let mut style = Style::new().fg(theme.scrollbar.thumb);
        if matches!(theme.scrollbar.thumb, Color::Transparent) {
            style = style.transform_fg(ColorTransform::Lighten(0.16));
        }
        Some(style)
    });
    let thumb_focus_style = thumb_focus_style.or_else(|| {
        theme.scrollbar.thumb_focus.map(|focus_color| {
            let mut style = Style::new().fg(focus_color);
            if matches!(focus_color, Color::Transparent) {
                style = style.transform_fg(ColorTransform::Lighten(0.24));
            }
            style
        })
    });
    let track_style = track_style.or_else(|| {
        theme.scrollbar.track.map(|track_color| {
            let mut style = Style::new().fg(track_color);
            if matches!(track_color, Color::Transparent) {
                style = style.transform_fg(ColorTransform::Lighten(0.04));
            }
            style
        })
    });

    (thumb_style, thumb_focus_style, track_style)
}

fn fill_style_defaults(style: Style, defaults: Style, include_bg: bool) -> Style {
    Style {
        fg: style.fg.or(defaults.fg),
        bg: if include_bg {
            style.bg.or(defaults.bg)
        } else {
            style.bg
        },
        fg_transform: style.fg_transform.or(defaults.fg_transform),
        bg_transform: style.bg_transform.or(defaults.bg_transform),
        contrast_policy: style.contrast_policy.or(defaults.contrast_policy),
        bold: style.bold.or(defaults.bold),
        dim: style.dim.or(defaults.dim),
        italic: style.italic.or(defaults.italic),
        underline: style.underline.or(defaults.underline),
        reverse: style.reverse.or(defaults.reverse),
        strikethrough: style.strikethrough.or(defaults.strikethrough),
        underline_color: style.underline_color.or(defaults.underline_color),
        dim_amount: style.dim_amount.or(defaults.dim_amount),
        tint: style.tint.or(defaults.tint),
    }
}

/// Resolve a base themed style while preserving the legacy rule that base theme
/// backgrounds are not inherited unless the widget set one explicitly.
pub(crate) fn resolve_base_style(theme: &Theme, style: Style) -> Style {
    fill_style_defaults(style, theme.primary, false)
}

/// Resolve border chrome using the border role foreground before falling back to primary.
pub(crate) fn resolve_border_style(theme: &Theme, style: Style) -> Style {
    fill_style_defaults(style, theme.primary.patch(theme.border), false)
}

/// Resolve muted/disabled secondary content without inheriting theme backgrounds.
pub(crate) fn resolve_muted_style(theme: &Theme, style: Style) -> Style {
    fill_style_defaults(style, theme.primary.patch(theme.muted), false)
}

/// Resolve accent glyphs without inheriting theme backgrounds.
pub(crate) fn resolve_accent_style(theme: &Theme, style: Style) -> Style {
    let accent = if theme.accent.is_empty() {
        theme.primary
    } else {
        theme.accent
    };
    fill_style_defaults(style, theme.primary.patch(accent), false)
}

/// Resolve spinner/progress/slider glyph emphasis using the old forced accent semantics.
pub(crate) fn resolve_force_accent_style(theme: &Theme, style: Style) -> Style {
    let accent = Style {
        fg: theme.accent.fg.or(theme.primary.fg),
        bg: None,
        fg_transform: theme.accent.fg_transform.or(theme.primary.fg_transform),
        bg_transform: theme.accent.bg_transform.or(theme.primary.bg_transform),
        contrast_policy: theme
            .accent
            .contrast_policy
            .or(theme.primary.contrast_policy),
        bold: theme.primary.bold,
        dim: theme.primary.dim,
        italic: theme.primary.italic,
        underline: theme.primary.underline,
        reverse: theme.primary.reverse,
        strikethrough: theme.primary.strikethrough,
        underline_color: theme.primary.underline_color,
        dim_amount: theme.primary.dim_amount,
        tint: theme.accent.tint.or(theme.primary.tint),
    };
    fill_style_defaults(style, accent, false)
}

pub(crate) fn resolve_splitter_hover_style(theme: &Theme, style: Style) -> Style {
    let explicit_fg = style.fg.is_some();
    let mut resolved = resolve_base_style(theme, style);
    if !theme.hover.is_empty() && !explicit_fg {
        resolved.fg = Some(theme.splitter.hover.into());
    }
    resolved
}

pub(crate) fn resolve_splitter_active_style(theme: &Theme, style: Style) -> Style {
    let explicit_fg = style.fg.is_some();
    let mut resolved = resolve_base_style(theme, style);
    if !explicit_fg {
        resolved.fg = Some(theme.splitter.active.into());
    }
    resolved
}

impl StyleSlot {
    /// Returns true when this slot resolves to a non-empty style against `role`.
    pub fn resolves_non_empty(&self, theme: &Theme, role: ThemeRole) -> bool {
        !resolve_slot(theme, role, self).is_empty()
    }
}

/// Resolve a focused/unfocused selection pair while preserving the old
/// `unfocused_selection_style = None` behavior: when the unfocused slot is left
/// as `Inherit` but the focused selection slot is customized, unfocused rows
/// mirror the focused selection slot instead of jumping back to the theme's
/// selection role.
pub fn resolve_selection_slot(
    theme: &Theme,
    selection_slot: &StyleSlot,
    unfocused_selection_slot: &StyleSlot,
    is_focused: bool,
) -> Style {
    if is_focused
        || (matches!(unfocused_selection_slot, StyleSlot::Inherit)
            && !matches!(selection_slot, StyleSlot::Inherit))
    {
        resolve_slot(theme, ThemeRole::Selection, selection_slot)
    } else {
        resolve_slot(
            theme,
            ThemeRole::UnfocusedSelection,
            unfocused_selection_slot,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Paint};

    fn p(color: Color) -> Option<Paint> {
        Some(Paint::Solid(color))
    }

    fn themed_selection() -> Theme {
        Theme::default().selection(Style::new().fg(Color::White).bg(Color::Blue))
    }

    #[test]
    fn replace_blocks_theme_bg_leak() {
        let resolved = resolve_slot(
            &themed_selection(),
            ThemeRole::Selection,
            &StyleSlot::Replace(Style::new().fg(Color::Red)),
        );
        assert_eq!(resolved.fg, p(Color::Red));
        assert_eq!(resolved.bg, None);
    }

    #[test]
    fn extend_keeps_theme_fields() {
        let resolved = resolve_slot(
            &themed_selection(),
            ThemeRole::Selection,
            &StyleSlot::Extend(Style::new().fg(Color::Red)),
        );
        assert_eq!(resolved.fg, p(Color::Red));
        assert_eq!(resolved.bg, p(Color::Blue));
    }

    #[test]
    fn inherit_yields_theme_role() {
        let resolved = resolve_slot(
            &themed_selection(),
            ThemeRole::Selection,
            &StyleSlot::Inherit,
        );
        assert_eq!(resolved.fg, p(Color::White));
        assert_eq!(resolved.bg, p(Color::Blue));
    }

    #[test]
    fn unfocused_selection_inherit_follows_custom_selection_slot() {
        let theme = themed_selection();
        let selection = StyleSlot::Replace(Style::new().fg(Color::Red));
        let resolved = resolve_selection_slot(&theme, &selection, &StyleSlot::Inherit, false);

        assert_eq!(resolved.fg, p(Color::Red));
        assert_eq!(resolved.bg, None);
    }

    #[test]
    fn unfocused_selection_inherit_uses_theme_when_selection_inherits() {
        let resolved = resolve_selection_slot(
            &themed_selection(),
            &StyleSlot::Inherit,
            &StyleSlot::Inherit,
            false,
        );

        assert_eq!(resolved.fg, p(Color::White));
        assert_eq!(resolved.bg, p(Color::Blue));
    }

    #[test]
    fn hover_focus_matrix_respects_slot_semantics() {
        let theme = Theme::default()
            .hover(Style::new().fg(Color::White).bg(Color::Blue))
            .focus(Style::new().fg(Color::Green).bg(Color::Black));

        let hover_replace = resolve_slot(
            &theme,
            ThemeRole::Hover,
            &StyleSlot::Replace(Style::new().fg(Color::Red)),
        );
        assert_eq!(hover_replace.fg, p(Color::Red));
        assert_eq!(hover_replace.bg, None);

        let focus_extend = resolve_slot(
            &theme,
            ThemeRole::Focus,
            &StyleSlot::Extend(Style::new().fg(Color::Yellow)),
        );
        assert_eq!(focus_extend.fg, p(Color::Yellow));
        assert_eq!(focus_extend.bg, p(Color::Black));
    }

    #[test]
    fn state_cascade_durable_focus_bg_beats_hover_bg() {
        let base = Style::new().bg(Color::rgb(10, 10, 10));
        let hover = Style::new().bg(Color::Blue);
        let focus = Style::new().bg(Color::Green);

        let resolved = resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover,
                    durability: Durability::Transient,
                },
                StateLayer {
                    style: &focus,
                    durability: Durability::Durable,
                },
            ],
        );

        assert_eq!(resolved.bg, p(Color::Green));

        let resolved = resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover,
                    durability: Durability::Durable,
                },
                StateLayer {
                    style: &focus,
                    durability: Durability::Durable,
                },
            ],
        );

        assert_eq!(resolved.bg, p(Color::Green));
    }

    #[test]
    fn state_cascade_transient_hover_transform_applies_over_focus_bg() {
        let base = Style::new().bg(Color::rgb(10, 10, 10));
        let hover = Style::new().transform_bg(ColorTransform::Dim(0.5));
        let focus = Style::new().bg(Color::rgb(200, 180, 160));

        let resolved = resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover,
                    durability: Durability::Transient,
                },
                StateLayer {
                    style: &focus,
                    durability: Durability::Durable,
                },
            ],
        )
        .resolve_color_transforms();

        assert_eq!(resolved.bg, p(Color::rgb(100, 90, 80)));
    }

    #[test]
    fn state_cascade_modifiers_remain_durable_wins() {
        let base = Style::new();
        let hover = Style {
            bold: Some(false),
            underline: Some(false),
            ..Style::new()
        };
        let focus = Style::new().bold().underline();

        let resolved = resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover,
                    durability: Durability::Transient,
                },
                StateLayer {
                    style: &focus,
                    durability: Durability::Durable,
                },
            ],
        );

        assert_eq!(resolved.bold, Some(true));
        assert_eq!(resolved.underline, Some(true));
    }

    #[test]
    fn state_cascade_transient_hover_compositor_effects_survive_focus() {
        let base = Style::new();
        let hover = Style::new().dim_by(0.4).tint_by(Color::Red, 0.5);
        let focus = Style::new()
            .bg(Color::Blue)
            .dim_by(0.1)
            .tint_by(Color::Green, 0.2);

        let resolved = resolve_state_cascade(
            base,
            &[
                StateLayer {
                    style: &hover,
                    durability: Durability::Transient,
                },
                StateLayer {
                    style: &focus,
                    durability: Durability::Durable,
                },
            ],
        );

        assert_eq!(resolved.dim_amount, Some(0.4));
        assert_eq!(resolved.tint, Some((Color::Red, 0.5)));
    }
}
