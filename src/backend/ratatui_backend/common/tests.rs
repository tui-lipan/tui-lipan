use ratatui::style::Color as RColor;

use super::{
    DEFAULT_SCROLLBAR_THUMB, IntegratedScrollbarAppearance, InteractiveStyleState,
    RatatuiTintCache, ScrollbarScrollState, apply_effect_style_clipped,
    apply_visual_effects_clipped, blend_paint_over_ratatui, dim_ratatui_color, finalize_style,
    gradient_sample_t, monochrome_ratatui_color, palette_quantize_ratatui_color,
    push_render_terminal_bg, render_integrated_hscrollbar, render_integrated_vscrollbar_half_block,
    render_vscrollbar_half_block, render_vscrollbar_with_metrics, resolve_interactive_style,
    resolve_interactive_style_raw, retro_crt_params, style_backdrop, tint_ratatui_color,
    truncate_end_with_ellipsis,
};
use crate::app::ContrastPolicy;
use crate::core::mask::CellMask;
use crate::style::{
    Color, ColorTransform, Paint, Rect, RetroPreset, RippleRadius, Style, VisualEffect,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect as RRect;
use std::sync::Arc;

use crate::utils::scrollbar::ScrollbarMetrics;

#[test]
fn truncates_with_ellipsis_when_too_wide() {
    let out = truncate_end_with_ellipsis("hello world", 5);
    assert_eq!(out.as_ref(), "hell…");
}

#[test]
fn returns_full_string_when_it_fits() {
    let out = truncate_end_with_ellipsis("hello", 5);
    assert_eq!(out.as_ref(), "hello");
}

#[test]
fn width_one_is_ellipsis() {
    let out = truncate_end_with_ellipsis("abc", 1);
    assert_eq!(out.as_ref(), "…");
}

#[test]
fn handles_wide_chars() {
    let out = truncate_end_with_ellipsis("你好世界", 3);
    assert_eq!(out.as_ref(), "你…");
    assert!(unicode_width::UnicodeWidthStr::width(out.as_ref()) <= 3);
}

#[test]
fn vaulttec_screen_bg_stays_darker_than_explicit_panel_bg() {
    let params = retro_crt_params(RetroPreset::VaultTec);

    let apply_bg = |mut color: RColor| {
        color = monochrome_ratatui_color(color, params.mono_strength);
        color = palette_quantize_ratatui_color(color, params.palette);
        if let Some((tint, alpha)) = params.tint {
            color = tint_ratatui_color(color, tint, alpha);
        }
        dim_ratatui_color(color, params.base_dim)
    };

    let screen_bg = apply_bg(params.screen_bg.expect("vaulttec screen bg"));
    let panel_bg = apply_bg(RColor::Rgb(28, 28, 28));

    assert_ne!(screen_bg, panel_bg);
}

#[test]
fn ratatui_tint_cache_matches_direct_tint() {
    let mut cache = RatatuiTintCache::new();
    let colors = [
        RColor::Reset,
        RColor::White,
        RColor::Black,
        RColor::Indexed(42),
        RColor::Rgb(20, 40, 80),
    ];

    for color in colors {
        assert_eq!(
            cache.tint(color, Color::Black, 0.6),
            tint_ratatui_color(color, Color::Black, 0.6)
        );
    }
    for color in colors {
        assert_eq!(
            cache.tint(color, Color::Black, 0.6),
            tint_ratatui_color(color, Color::Black, 0.6)
        );
    }
}

#[test]
fn interactive_style_focus_overrides_hover() {
    let style = resolve_interactive_style(
        Style::new().fg(Color::White).bg(Color::Black),
        Style::new().bg(Color::Blue),
        Style::new().bg(Color::Red),
        Style::new().bg(Color::Green),
        InteractiveStyleState {
            is_focused: true,
            is_hovered: true,
            is_disabled: false,
            policy: ContrastPolicy::Off,
        },
    );

    assert_eq!(style.bg, Some(Paint::Solid(Color::Blue)));
}

#[test]
fn interactive_style_focus_concrete_bg_allows_hover_transform() {
    let style = resolve_interactive_style_raw(
        Style::new().bg(Color::rgb(10, 10, 10)),
        Style::new().bg(Color::rgb(80, 80, 80)),
        Style::new()
            .bg(Color::Red)
            .transform_bg(ColorTransform::Dim(0.5)),
        Style::new().bg(Color::Green),
        true,
        true,
        false,
    );

    assert_eq!(style.bg, Some(Paint::Solid(Color::rgb(40, 40, 40))));
}

#[test]
fn interactive_style_disabled_remains_terminal() {
    let style = resolve_interactive_style_raw(
        Style::new().bg(Color::rgb(10, 10, 10)),
        Style::new().bg(Color::rgb(80, 80, 80)),
        Style::new()
            .bg(Color::Red)
            .transform_bg(ColorTransform::Dim(0.5)),
        Style::new().bg(Color::Green),
        true,
        true,
        true,
    );

    assert_eq!(style.bg, Some(Paint::Solid(Color::Green)));
}

#[test]
fn interactive_style_hover_transform_re_adjusts_fg_after_bg_change() {
    let base = Style::new()
        .fg(Color::rgb(120, 120, 120))
        .bg(Color::rgb(20, 20, 20));
    let hover = Style::new().transform_bg(ColorTransform::Lighten(0.9));

    let unfocused = resolve_interactive_style(
        base,
        Style::default(),
        hover,
        Style::default(),
        InteractiveStyleState {
            is_focused: false,
            is_hovered: false,
            is_disabled: false,
            policy: ContrastPolicy::Wcag,
        },
    );
    let hovered = resolve_interactive_style(
        base,
        Style::default(),
        hover,
        Style::default(),
        InteractiveStyleState {
            is_focused: false,
            is_hovered: true,
            is_disabled: false,
            policy: ContrastPolicy::Wcag,
        },
    );

    assert_ne!(hovered.bg, unfocused.bg);
    assert_ne!(hovered.fg, unfocused.fg);
}

#[test]
fn interactive_state_transforms_stack_on_previous_resolved_color() {
    // hover and focus carry only transforms; both should compose on the
    // previously resolved bg so that two stacked Dim(0.5) halves twice.
    let base = Style::new().bg(Color::rgb(100, 100, 100));
    let hover = Style::new().transform_bg(ColorTransform::Dim(0.5));
    let focus = Style::new().transform_bg(ColorTransform::Dim(0.5));

    let style = resolve_interactive_style(
        base,
        focus,
        hover,
        Style::default(),
        InteractiveStyleState {
            is_focused: true,
            is_hovered: true,
            is_disabled: false,
            policy: ContrastPolicy::Off,
        },
    );

    assert_eq!(style.bg, Some(Paint::Solid(Color::rgb(25, 25, 25))));
}

#[test]
fn finalize_style_resolves_opacity_with_backdrop_before_contrast() {
    let raw = Style::new()
        .fg(Color::Transparent)
        .transform_fg(ColorTransform::Opacity(0.5));

    let finalized = finalize_style(raw, Some(Color::rgb(10, 20, 30)), ContrastPolicy::Off);

    assert_eq!(
        finalized.fg,
        Some(Paint::Alpha {
            color: Color::rgb(10, 20, 30),
            alpha: 128,
        })
    );
    assert_eq!(finalized.fg_transform, None);
}

#[test]
fn contrast_policy_flattens_alpha_bg_against_terminal_bg() {
    // Light pigment with low alpha over a dark terminal renders as mostly
    // dark. The contrast policy must see the *rendered* (dark) bg and pick
    // a light fg — not see the white pigment and pick black.
    let _scope = push_render_terminal_bg(Some(RColor::Rgb(20, 20, 20)));

    let raw = Style::new()
        .fg(Color::White)
        .bg(Paint::rgba(255, 255, 255, 40));

    let finalized = finalize_style(raw, None, ContrastPolicy::BlackOrWhite);

    // Without the fix, the policy would see white-on-white and snap to
    // black, which is unreadable over the actual (dark) rendered bg.
    assert_eq!(finalized.fg, Some(Paint::Solid(Color::White)));
    // Bg paint preserved for the renderer to blend.
    assert_eq!(finalized.bg, Some(Paint::rgba(255, 255, 255, 40)));
}

#[test]
fn contrast_policy_prefers_style_backdrop_for_alpha_bg() {
    // A translucent blue selection over a dark content surface should be
    // evaluated against the rendered dark-blue mix, not the raw blue pigment or
    // unrelated terminal background.
    let _scope = push_render_terminal_bg(Some(RColor::Rgb(240, 240, 240)));

    let raw = Style::new()
        .fg(Color::White)
        .bg_alpha(Color::rgb(80, 160, 255), 0.35)
        .contrast_policy(ContrastPolicy::Apca);

    let finalized = finalize_style(raw, Some(Color::rgb(20, 20, 24)), ContrastPolicy::Wcag);

    assert_eq!(finalized.fg, Some(Paint::Solid(Color::White)));
    assert_eq!(finalized.bg, Some(Paint::rgba(80, 160, 255, 89)));
}

#[test]
fn tint_scrim_darkens_reset_bg_via_terminal_bg() {
    use crate::style::Style;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect as RRect;

    // Two adjacent cells representing a frame body cell (explicit bg) and
    // a decoration cell beneath it (fg only, bg = Reset because the
    // decoration sits outside the body fill).
    let terminal_bg = Some(RColor::Rgb(0x13, 0x14, 0x1a));
    let area = RRect::new(0, 0, 2, 1);
    let mut buf = Buffer::empty(area);
    buf.cell_mut((0, 0)).unwrap().bg = RColor::Rgb(0x13, 0x14, 0x1a);
    buf.cell_mut((1, 0)).unwrap().fg = RColor::Rgb(0x13, 0x14, 0x1a);
    buf.cell_mut((1, 0)).unwrap().bg = RColor::Reset;

    let backend = ratatui::backend::TestBackend::new(2, 1);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| {
        *f.buffer_mut() = buf.clone();
        apply_effect_style_clipped(
            f,
            crate::style::Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 1,
            },
            Style::new().tint_by(Color::Black, 0.6),
            None,
            terminal_bg,
        );

        // Body cell bg: blend(#13141a, Black, 0.6) = #08080a
        assert_eq!(
            f.buffer_mut().cell((0, 0)).unwrap().bg,
            RColor::Rgb(8, 8, 10)
        );
        // Decoration cell fg: same blend → #08080a
        assert_eq!(
            f.buffer_mut().cell((1, 0)).unwrap().fg,
            RColor::Rgb(8, 8, 10)
        );
        // Decoration cell bg: resolved against terminal_bg (#13141a),
        // not collapsed to #000000.
        assert_eq!(
            f.buffer_mut().cell((1, 0)).unwrap().bg,
            RColor::Rgb(8, 8, 10)
        );
    })
    .unwrap();
}

#[test]
fn tint_scrim_leaves_reset_bg_when_terminal_bg_unknown() {
    use crate::style::Style;
    use ratatui::layout::Rect as RRect;

    let area = RRect::new(0, 0, 1, 1);
    let backend = ratatui::backend::TestBackend::new(1, 1);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| {
        f.buffer_mut().cell_mut((0, 0)).unwrap().bg = RColor::Reset;
        apply_effect_style_clipped(
            f,
            crate::style::Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            Style::new().tint_by(Color::Black, 0.6),
            None,
            None,
        );
        // Without knowing the terminal bg, leave Reset alone rather than
        // synthesizing #000000.
        assert_eq!(f.buffer_mut().cell((0, 0)).unwrap().bg, RColor::Reset);
    })
    .unwrap();
    let _ = area;
}

#[test]
fn lighten_transform_resolves_reset_bg_via_terminal_bg() {
    let terminal_bg_color = RColor::Rgb(0x20, 0x22, 0x2a);
    let terminal_bg = Some(terminal_bg_color);
    let backend = TestBackend::new(1, 1);
    let mut term = Terminal::new(backend).unwrap();

    term.draw(|f| {
        f.buffer_mut().cell_mut((0, 0)).unwrap().bg = RColor::Reset;

        apply_effect_style_clipped(
            f,
            crate::style::Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            Style::new().transform_bg(ColorTransform::Lighten(0.35)),
            None,
            terminal_bg,
        );

        let bg = f.buffer_mut().cell((0, 0)).unwrap().bg;
        assert_ne!(bg, RColor::Reset);
        assert_ne!(bg, terminal_bg_color);
    })
    .unwrap();
}

#[test]
fn style_backdrop_flattens_alpha_bg_against_terminal_bg() {
    let _scope = push_render_terminal_bg(Some(RColor::Rgb(0, 0, 200)));
    let style = Style::new().bg(Paint::rgba(200, 0, 0, 128));

    assert_eq!(style_backdrop(style), Some(Color::Rgb(100, 0, 100)));
}

#[test]
fn blends_alpha_paint_over_ratatui_rgb_backdrop() {
    let paint = Paint::rgba(200, 0, 0, 128);

    let blended = blend_paint_over_ratatui(paint, RColor::Rgb(0, 0, 200));

    assert_eq!(blended, Some(RColor::Rgb(100, 0, 100)));
}

#[test]
fn scrollbar_custom_thumb_alpha_blends_over_track_bg() {
    let track_bg = RColor::Rgb(0x15, 0x15, 0x19);
    let thumb = Paint::rgba(0xff, 0xff, 0xff, 0x40);
    let backend = TestBackend::new(1, 3);
    let mut terminal = Terminal::new(backend).expect("terminal should init");

    terminal
        .draw(|f| {
            render_vscrollbar_with_metrics(
                f,
                Rect {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 3,
                },
                ScrollbarMetrics {
                    thumb_len: 1,
                    thumb_start: 1,
                    max_thumb_start: 2,
                    max_offset: 10,
                },
                '┃',
                Style::new().fg(thumb),
                Some(Style::new().bg(Color::Rgb(0x15, 0x15, 0x19))),
                None,
            );
        })
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let thumb_cell = &buffer[(0, 1)];
    let track_cell = &buffer[(0, 0)];

    assert_eq!(track_cell.symbol(), " ");
    assert_eq!(track_cell.bg, track_bg);
    assert_eq!(thumb_cell.symbol(), "┃");
    assert_eq!(thumb_cell.bg, track_bg);
    assert_eq!(
        Some(thumb_cell.fg),
        blend_paint_over_ratatui(thumb, track_bg)
    );
    assert_ne!(thumb_cell.fg, RColor::White);
}

#[test]
fn scrollbar_half_thumb_alpha_blends_over_track_bg() {
    let track_bg = RColor::Rgb(0x15, 0x15, 0x19);
    let thumb = Paint::rgba(0xff, 0xff, 0xff, 0x40);
    let backend = TestBackend::new(1, 1);
    let mut terminal = Terminal::new(backend).expect("terminal should init");

    terminal
        .draw(|f| {
            render_vscrollbar_half_block(
                f,
                Rect {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                ScrollbarMetrics {
                    thumb_len: 1,
                    thumb_start: 0,
                    max_thumb_start: 1,
                    max_offset: 10,
                },
                Style::new().fg(thumb),
                Some(Style::new().bg(Color::Rgb(0x15, 0x15, 0x19))),
                None,
            );
        })
        .expect("draw should succeed");

    let cell = &terminal.backend().buffer()[(0, 0)];

    assert_eq!(cell.symbol(), "▀");
    assert_eq!(cell.bg, track_bg);
    assert_eq!(Some(cell.fg), blend_paint_over_ratatui(thumb, track_bg));
    assert_ne!(cell.fg, RColor::White);
}

#[test]
fn integrated_vscrollbar_half_thumb_alpha_blends_over_track_style() {
    let track_bg = RColor::Rgb(0x15, 0x15, 0x19);
    let fallback_bg = RColor::Rgb(0x80, 0x00, 0x00);
    let thumb = Paint::rgba(0xff, 0xff, 0xff, 0x40);
    let backend = TestBackend::new(1, 1);
    let mut terminal = Terminal::new(backend).expect("terminal should init");

    terminal
        .draw(|f| {
            render_integrated_vscrollbar_half_block(
                f,
                Rect {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                ScrollbarMetrics {
                    thumb_len: 1,
                    thumb_start: 0,
                    max_thumb_start: 1,
                    max_offset: 10,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: DEFAULT_SCROLLBAR_THUMB,
                    border_char: "│",
                    base_style: Style::new().bg(Color::Rgb(0x80, 0x00, 0x00)),
                    thumb_style: Style::new().fg(thumb),
                    track_style: Some(Style::new().bg(Color::Rgb(0x15, 0x15, 0x19))),
                    clip_rect: None,
                    metrics_cache: None,
                },
            );
        })
        .expect("draw should succeed");

    let cell = &terminal.backend().buffer()[(0, 0)];

    assert_eq!(cell.symbol(), "▀");
    assert_eq!(cell.bg, track_bg);
    assert_eq!(Some(cell.fg), blend_paint_over_ratatui(thumb, track_bg));
    assert_ne!(Some(cell.fg), blend_paint_over_ratatui(thumb, fallback_bg));
}

#[test]
fn integrated_hscrollbar_thumb_alpha_blends_over_track_style() {
    let track_bg = RColor::Rgb(0x15, 0x15, 0x19);
    let fallback_bg = RColor::Rgb(0x80, 0x00, 0x00);
    let thumb = Paint::rgba(0xff, 0xff, 0xff, 0x40);
    let backend = TestBackend::new(3, 1);
    let mut terminal = Terminal::new(backend).expect("terminal should init");

    terminal
        .draw(|f| {
            render_integrated_hscrollbar(
                f,
                Rect {
                    x: 0,
                    y: 0,
                    w: 3,
                    h: 1,
                },
                ScrollbarScrollState {
                    offset: 10,
                    visible: 10,
                    total: 30,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: '■',
                    border_char: "─",
                    base_style: Style::new().bg(Color::Rgb(0x80, 0x00, 0x00)),
                    thumb_style: Style::new().fg(thumb),
                    track_style: Some(Style::new().bg(Color::Rgb(0x15, 0x15, 0x19))),
                    clip_rect: None,
                    metrics_cache: None,
                },
            );
        })
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let track_cell = &buffer[(0, 0)];
    let thumb_cell = &buffer[(1, 0)];

    assert_eq!(track_cell.symbol(), "─");
    assert_eq!(track_cell.bg, track_bg);
    assert_eq!(thumb_cell.symbol(), "■");
    assert_eq!(thumb_cell.bg, track_bg);
    assert_eq!(
        Some(thumb_cell.fg),
        blend_paint_over_ratatui(thumb, track_bg)
    );
    assert_ne!(
        Some(thumb_cell.fg),
        blend_paint_over_ratatui(thumb, fallback_bg)
    );
}

#[test]
fn transparent_alpha_paint_leaves_ratatui_channel_unset() {
    let paint = Paint::rgba(200, 0, 0, 0);

    assert_eq!(
        blend_paint_over_ratatui(paint, RColor::Rgb(0, 0, 200)),
        None
    );
}

#[test]
fn interactive_style_raw_keeps_contrast_unapplied() {
    let raw = resolve_interactive_style_raw(
        Style::new().fg(Color::rgb(0, 0, 0)).bg(Color::rgb(0, 0, 0)),
        Style::new(),
        Style::new(),
        Style::new(),
        false,
        false,
        false,
    );

    assert_eq!(raw.fg, Some(Paint::Solid(Color::rgb(0, 0, 0))));

    let finalized = finalize_style(raw, None, ContrastPolicy::Wcag);
    assert_ne!(finalized.fg, Some(Paint::Solid(Color::rgb(0, 0, 0))));
}

#[test]
fn gradient_sample_uses_full_smooth_mirrored_cycle_per_frequency() {
    let bounds = RRect::new(0, 0, 100, 1);

    let left = gradient_sample_t(
        0,
        0,
        0,
        bounds,
        1.0,
        0.0,
        crate::style::EffectAxis::Horizontal,
    );
    let center = gradient_sample_t(
        49,
        0,
        0,
        bounds,
        1.0,
        0.0,
        crate::style::EffectAxis::Horizontal,
    );
    let right = gradient_sample_t(
        99,
        0,
        0,
        bounds,
        1.0,
        0.0,
        crate::style::EffectAxis::Horizontal,
    );

    assert!(left < 0.001, "left edge should start near min: {left}");
    assert!(center > 0.999, "center should reach max: {center}");
    assert!(right < 0.001, "right edge should return near min: {right}");
}

#[test]
fn clipped_mask_tints_only_lit_cells() {
    let backend = TestBackend::new(3, 1);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let scope = Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 1,
    };
    let mut bits = vec![0u64; 1];
    bits[0] = 1;
    let mask = Arc::new(CellMask {
        origin: (0, 0),
        w: 3,
        h: 1,
        bits: bits.into(),
    });
    let clipped = VisualEffect::Clipped {
        bounds: None,
        mask: Some(mask),
        inner: Box::new(VisualEffect::dim(1.0)),
    };

    terminal
        .draw(|f| {
            for x in 0u16..3u16 {
                if let Some(c) = f.buffer_mut().cell_mut((x, 0)) {
                    c.fg = RColor::White;
                    c.bg = RColor::Black;
                }
            }
            apply_visual_effects_clipped(f, scope, &[clipped], 0, None, None);
        })
        .expect("draw");

    let buf = terminal.backend().buffer();
    assert_ne!(buf[(0, 0)].fg, buf[(1, 0)].fg);
    assert_eq!(buf[(1, 0)].fg, buf[(2, 0)].fg);
}

#[test]
fn centered_ripple_resolves_scope_center() {
    let backend = TestBackend::new(5, 1);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let scope = Rect {
        x: 0,
        y: 0,
        w: 5,
        h: 1,
    };
    let effect = VisualEffect::centered_ripple(0.0, 0.75, Color::rgb(255, 0, 0), 1.0);

    terminal
        .draw(|f| {
            for x in 0u16..5u16 {
                if let Some(c) = f.buffer_mut().cell_mut((x, 0)) {
                    c.fg = RColor::White;
                    c.bg = RColor::Black;
                }
            }
            apply_visual_effects_clipped(f, scope, &[effect], 0, None, None);
        })
        .expect("draw");

    let buf = terminal.backend().buffer();
    assert_ne!(buf[(2, 0)].fg, buf[(0, 0)].fg);
    assert_eq!(buf[(0, 0)].fg, RColor::White);
    assert_eq!(buf[(4, 0)].fg, RColor::White);
}

#[test]
fn looping_ripple_changes_output_by_phase() {
    let draw_fgs = |phase| {
        let backend = TestBackend::new(9, 1);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let scope = Rect {
            x: 0,
            y: 0,
            w: 9,
            h: 1,
        };
        let effect =
            VisualEffect::centered_looping_ripple(4.0, 10, 0.75, Color::rgb(255, 0, 0), 1.0);

        terminal
            .draw(|f| {
                for x in 0u16..9u16 {
                    if let Some(c) = f.buffer_mut().cell_mut((x, 0)) {
                        c.fg = RColor::White;
                        c.bg = RColor::Black;
                    }
                }
                apply_visual_effects_clipped(f, scope, &[effect], phase, None, None);
            })
            .expect("draw");

        (0u16..9u16)
            .map(|x| terminal.backend().buffer()[(x, 0)].fg)
            .collect::<Vec<_>>()
    };

    assert_ne!(draw_fgs(0), draw_fgs(5));
}

#[test]
fn once_ripple_before_start_is_noop() {
    let backend = TestBackend::new(5, 1);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let scope = Rect {
        x: 0,
        y: 0,
        w: 5,
        h: 1,
    };
    let effect = VisualEffect::Ripple {
        origin: crate::style::EffectOrigin::CENTER,
        radius: RippleRadius::Once {
            max_radius: 4.0,
            duration_ticks: 10,
            start_tick: 5,
        },
        ring_width: 0.75,
        tint: Color::rgb(255, 0, 0),
        strength: 1.0,
    };

    terminal
        .draw(|f| {
            for x in 0u16..5u16 {
                if let Some(c) = f.buffer_mut().cell_mut((x, 0)) {
                    c.fg = RColor::White;
                    c.bg = RColor::Black;
                }
            }
            apply_visual_effects_clipped(f, scope, &[effect], 4, None, None);
        })
        .expect("draw");

    let buf = terminal.backend().buffer();
    for x in 0u16..5u16 {
        assert_eq!(buf[(x, 0)].fg, RColor::White);
        assert_eq!(buf[(x, 0)].bg, RColor::Black);
    }
}

#[test]
fn clipped_bounds_only_tints_inside_rect() {
    let backend = TestBackend::new(4, 1);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let scope = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let clipped = VisualEffect::Clipped {
        bounds: Some(Rect {
            x: 1,
            y: 0,
            w: 2,
            h: 1,
        }),
        mask: None,
        inner: Box::new(VisualEffect::dim(1.0)),
    };

    terminal
        .draw(|f| {
            for x in 0u16..4u16 {
                if let Some(c) = f.buffer_mut().cell_mut((x, 0)) {
                    c.fg = RColor::White;
                    c.bg = RColor::Black;
                }
            }
            apply_visual_effects_clipped(f, scope, &[clipped], 0, None, None);
        })
        .expect("draw");

    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].fg, buf[(3, 0)].fg);
    assert_ne!(buf[(1, 0)].fg, buf[(0, 0)].fg);
    assert_eq!(buf[(1, 0)].fg, buf[(2, 0)].fg);
}
