use crate::style::{Color, ColorTransform, Style, Theme, ThemeRole};

pub(crate) fn with_theme_role(theme: &Theme, role: ThemeRole, style: Style) -> Style {
    theme.role(role).patch(style)
}

pub(crate) fn with_theme_primary(theme: &Theme, style: Style) -> Style {
    let primary = theme.primary;
    Style {
        fg: style.fg.or(primary.fg),
        bg: style.bg,
        fg_transform: style.fg_transform.or(primary.fg_transform),
        bg_transform: style.bg_transform.or(primary.bg_transform),
        contrast_policy: style.contrast_policy.or(primary.contrast_policy),
        bold: style.bold.or(primary.bold),
        dim: style.dim.or(primary.dim),
        italic: style.italic.or(primary.italic),
        underline: style.underline.or(primary.underline),
        reverse: style.reverse.or(primary.reverse),
        strikethrough: style.strikethrough.or(primary.strikethrough),
        underline_color: style.underline_color.or(primary.underline_color),
        dim_amount: style.dim_amount.or(primary.dim_amount),
        tint: style.tint.or(primary.tint),
    }
}

pub(crate) fn with_theme_muted(theme: &Theme, style: Style) -> Style {
    let muted = theme.muted;
    Style {
        fg: style.fg.or(muted.fg).or(theme.primary.fg),
        bg: style.bg,
        fg_transform: style
            .fg_transform
            .or(muted.fg_transform)
            .or(theme.primary.fg_transform),
        bg_transform: style
            .bg_transform
            .or(muted.bg_transform)
            .or(theme.primary.bg_transform),
        contrast_policy: style
            .contrast_policy
            .or(muted.contrast_policy)
            .or(theme.primary.contrast_policy),
        bold: style.bold.or(muted.bold).or(theme.primary.bold),
        dim: style.dim.or(muted.dim).or(theme.primary.dim),
        italic: style.italic.or(muted.italic).or(theme.primary.italic),
        underline: style
            .underline
            .or(muted.underline)
            .or(theme.primary.underline),
        reverse: style.reverse.or(muted.reverse).or(theme.primary.reverse),
        strikethrough: style
            .strikethrough
            .or(muted.strikethrough)
            .or(theme.primary.strikethrough),
        underline_color: style
            .underline_color
            .or(muted.underline_color)
            .or(theme.primary.underline_color),
        dim_amount: style
            .dim_amount
            .or(muted.dim_amount)
            .or(theme.primary.dim_amount),
        tint: style.tint.or(muted.tint).or(theme.primary.tint),
    }
}

pub(crate) fn with_theme_accent(theme: &Theme, style: Style) -> Style {
    let accent = if theme.accent.is_empty() {
        theme.primary
    } else {
        theme.accent
    };

    Style {
        fg: style.fg.or(accent.fg).or(theme.primary.fg),
        bg: style.bg,
        fg_transform: style
            .fg_transform
            .or(accent.fg_transform)
            .or(theme.primary.fg_transform),
        bg_transform: style
            .bg_transform
            .or(accent.bg_transform)
            .or(theme.primary.bg_transform),
        contrast_policy: style
            .contrast_policy
            .or(accent.contrast_policy)
            .or(theme.primary.contrast_policy),
        bold: style.bold.or(accent.bold).or(theme.primary.bold),
        dim: style.dim.or(accent.dim).or(theme.primary.dim),
        italic: style.italic.or(accent.italic).or(theme.primary.italic),
        underline: style
            .underline
            .or(accent.underline)
            .or(theme.primary.underline),
        reverse: style.reverse.or(accent.reverse).or(theme.primary.reverse),
        strikethrough: style
            .strikethrough
            .or(accent.strikethrough)
            .or(theme.primary.strikethrough),
        underline_color: style
            .underline_color
            .or(accent.underline_color)
            .or(theme.primary.underline_color),
        dim_amount: style
            .dim_amount
            .or(accent.dim_amount)
            .or(theme.primary.dim_amount),
        tint: style.tint.or(accent.tint).or(theme.primary.tint),
    }
}

pub(crate) fn with_theme_input_focus(theme: &Theme, style: Style) -> Style {
    if theme.focus_decoration {
        theme.input.focus.patch(style)
    } else {
        style
    }
}

pub(crate) fn with_theme_error(theme: &Theme, style: Style) -> Style {
    if style.is_empty() {
        theme.role(ThemeRole::Error)
    } else {
        style
    }
}

pub(crate) fn with_theme_optional_primary(theme: &Theme, style: Option<Style>) -> Option<Style> {
    style.map(|style| with_theme_primary(theme, style))
}

pub(crate) fn scrollbar_styles(
    theme: &Theme,
    thumb_style: Option<Style>,
    thumb_focus_style: Option<Style>,
    track_style: Option<Style>,
) -> (Option<Style>, Option<Style>, Option<Style>) {
    let thumb_style =
        thumb_style.or_else(|| Some(scrollbar_palette_style(theme.scrollbar.thumb, 0.16)));
    let thumb_focus_style = thumb_focus_style.or_else(|| {
        if !theme.focus_decoration {
            return None;
        }
        theme
            .scrollbar
            .thumb_focus
            .map(|color| scrollbar_palette_style(color, 0.24))
    });
    let track_style = track_style.or_else(|| {
        theme
            .scrollbar
            .track
            .map(|color| scrollbar_palette_style(color, 0.04))
    });

    (thumb_style, thumb_focus_style, track_style)
}

fn scrollbar_palette_style(color: Color, lighten: f32) -> Style {
    let mut style = Style::new().fg(color);
    if matches!(color, Color::Transparent) {
        style = style.transform_fg(ColorTransform::Lighten(lighten));
    }
    style
}
