use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::{RenderContext, build_join_index, render};
use crate::animation::{Easing, TransitionConfig};
use crate::app::ContrastPolicy;
use crate::app::context::SurfaceMode;
use crate::backend::ratatui_backend::common::finalize_style;
use crate::core::component::{Component, Context, Update};
use crate::core::node::{NodeId, NodeKind};
use crate::runtime::RuntimeCore;
#[cfg(feature = "devtools")]
use crate::style::BorderStyle;
use crate::style::{
    BorderEdges, Color, ColorTransform, Edge, EffectAxis, EffectPalette, Length, Paint, Rect,
    ScrollbarConfig, ScrollbarVariant, Style, Theme, VisualEffect,
};
use crate::utils::color_contrast::contrast_ratio;
use crate::widgets::{
    Animated, BorderLabels, BorderMergeMode, Button, Canvas, DecorationGlyph, DecorationPlacement,
    Divider, EdgeDecoration, EffectScope, Frame, FrameLabel, HStack, List, ListItem, Modal, Spacer,
    Splitter, SplitterHandleMode, TabVariant, Text, Toast, VStack, ZStack,
};

struct HeaderFrameComponent;

struct EffectScopeRenderComponent;

struct EffectScopeColorTransformFgOnlyComponent;

struct EffectScopeMonochromeComponent;

struct EffectScopePaletteQuantizeComponent;

struct EffectScopeRainbowWaveComponent;

struct EffectScopeResetSkipComponent;

struct EffectScopeNestedRootPortalComponent;

struct EffectScopeWrappedComponentRootPortalComponent;

struct RootPortalModalOnlyComponent;

struct EffectScopeScanlinesComponent;

struct NestedEffectScopeCompositionComponent;

struct DevToolsTopmostAppBackdropComponent;

#[cfg(feature = "devtools")]
struct DevToolsBorderUnderlayComponent;

struct TransparentModalOverlayComponent;

struct ExplicitOverlayBackgroundPaintComponent;

struct ExplicitOverlayForegroundOnlySpacesComponent;

struct ToastTransitionUnderlayComponent;

struct ToastSurfaceBandsComponent;

struct ToastTransitionDefaultUnderlayComponent;

struct TransparentModalBorderOverColoredBackgroundComponent;

struct DefaultModalBackdropClearsForegroundComponent;

struct TransparentModalBorderPreservesUnderlyingForegroundComponent;

struct TransparentFrameDecorationPreservesUnderlyingBackgroundComponent;

struct AnimatedOpacityFadeComponent;

struct AnimatedOpacityZeroZStackComponent;

struct AnimatedOpacityZeroFgOnlyZStackComponent;

struct AnimatedOpacityHalfUnderlayComponent;

struct AnimatedColorTargetComponent {
    active: Rc<Cell<bool>>,
}

struct AnimatedPositionOffsetComponent {
    opacity: f32,
}

struct CompactFramePaintLeakComponent;
struct CompactFrameStatusRightComponent;

struct GroupedBorderLabelsComponent;

struct TabsWithHeaderLabelComponent;

struct HeaderLabelPaddingStyleComponent;

struct OffscreenModalOverlayComponent;

struct FrameHoverUnderModalComponent;

struct ButtonAlphaHoverComponent;

fn render_runtime_with_hover<C: Component>(
    runtime: &RuntimeCore<C>,
    viewport: Rect,
    hovered: Option<NodeId>,
    mouse_pos: Option<(u16, u16)>,
) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let scrollbar_metrics_cache = RefCell::new(Default::default());
    let overlay_bg_snapshot = RefCell::new(Vec::new());
    let join_index = build_join_index(&runtime.tree);
    let cursor_position = Cell::new(None);
    let dnd_snapshot_cells = RefCell::new(None);
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered,
        mouse_pos,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &scrollbar_metrics_cache,
        overlay_bg_snapshot: &overlay_bg_snapshot,
        join_index: &join_index,
        cursor_position: &cursor_position,
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &dnd_snapshot_cells,
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");
    terminal.backend().buffer().clone()
}

#[test]
fn style_contrast_policy_overrides_widget_policy() {
    let style = Style::new()
        .fg(Color::rgb(0, 0, 0))
        .bg(Color::rgb(0, 0, 0))
        .contrast_policy(ContrastPolicy::Wcag);

    let adjusted = finalize_style(style, None, ContrastPolicy::Off);

    assert!(contrast_ratio(adjusted.fg.unwrap().color(), adjusted.bg.unwrap().color()) >= 4.5);
    assert_eq!(adjusted.contrast_policy, None);
}

#[test]
fn style_contrast_policy_judges_an_alpha_bg_against_the_supplied_backdrop() {
    // An alpha background is flattened before the policy weighs it, so the pairing it judges is
    // the one that will be rendered *given this backdrop* - not the raw pigment.
    let style = Style::new()
        .fg(Color::rgb(0, 0, 0))
        .bg_alpha(Color::rgb(0, 0, 0), 0.5);

    let over_black = finalize_style(style, Some(Color::rgb(0, 0, 0)), ContrastPolicy::Wcag);
    let over_white = finalize_style(style, Some(Color::rgb(255, 255, 255)), ContrastPolicy::Wcag);

    // Same style, different backdrop, different correction: black-on-black must be lifted, while
    // black over a half-white blend is already readable and is left alone.
    assert_ne!(
        over_black.fg, over_white.fg,
        "the backdrop must change the verdict",
    );
    assert_eq!(
        over_white.fg,
        Some(crate::style::Paint::Solid(Color::rgb(0, 0, 0))),
    );
    // The alpha paint survives resolution so the renderer still composites it.
    assert_eq!(over_black.bg, style.bg);
}

#[test]
fn style_contrast_policy_cannot_see_content_below_a_floating_overlay() {
    // Pins the limitation `Style::contrast_policy` documents: with no backdrop to name, it falls
    // back to the terminal background. Text that is readable against *that* still renders against
    // whatever cells the overlay happens to cover, which this pass never sees. Content-aware
    // correction is `EffectScope::contrast_policy`, which runs per cell after compositing.
    let style = Style::new()
        .fg(Color::rgb(171, 178, 191))
        .bg_alpha(Color::rgb(50, 54, 61), 0.82);

    let assumed = finalize_style(style, None, ContrastPolicy::Wcag);
    let over_white = finalize_style(style, Some(Color::rgb(255, 255, 255)), ContrastPolicy::Wcag);

    assert_ne!(
        assumed.fg, over_white.fg,
        "naming the real backdrop is what lets the policy correct this pairing",
    );
}

#[test]
fn style_contrast_policy_off_disables_widget_auto_contrast() {
    let style = Style::new()
        .fg(Color::rgb(0, 0, 0))
        .bg(Color::rgb(0, 0, 0))
        .contrast_policy(ContrastPolicy::Off);

    let adjusted = finalize_style(style, None, ContrastPolicy::Wcag);

    assert_eq!(
        adjusted.fg,
        Some(crate::style::Paint::Solid(Color::rgb(0, 0, 0)))
    );
    assert_eq!(adjusted.contrast_policy, None);
}

impl Component for HeaderFrameComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .height(Length::Px(3))
            .header_content(Text::new("Search"))
            .child(Text::new("Body"))
            .into()
    }
}

impl Component for GroupedBorderLabelsComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(24))
            .height(Length::Px(5))
            .header(
                BorderLabels::new()
                    .left(FrameLabel::new("left"))
                    .center("center")
                    .right("right"),
            )
            .footer(
                BorderLabels::new()
                    .left("mode")
                    .center("workspace")
                    .right("time"),
            )
            .child(Text::new("body"))
            .into()
    }
}

impl Component for TabsWithHeaderLabelComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(42))
            .height(Length::Px(3))
            .header_left("[2]")
            .header_padding(1)
            .tab_titles(["Files", "Worktrees", "Submodule"])
            .tab_variant(TabVariant::Minimal)
            .active_tab(1)
            .child(Text::new("body"))
            .into()
    }
}

impl Component for HeaderLabelPaddingStyleComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(12))
            .height(Length::Px(3))
            .style(Style::new().fg(Color::Red))
            .header(
                BorderLabels::new()
                    .left(FrameLabel::new("X").style(Style::new().fg(Color::Blue)))
                    .padding(1),
            )
            .child(Text::new("body"))
            .into()
    }
}

#[test]
fn grouped_border_labels_render_in_all_positions() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        GroupedBorderLabelsComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);
    let row = |y: u16| {
        (0..viewport.w)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect::<String>()
    };

    assert!(row(0).contains("left"));
    assert!(row(0).contains("center"));
    assert!(row(0).contains("right"));
    assert!(row(4).contains("mode"));
    assert!(row(4).contains("workspace"));
    assert!(row(4).contains("time"));
}

#[test]
fn grouped_label_style_resolution_preserves_each_layer() {
    let base = Style::new().fg(Color::Red);
    let group_style = Style::new().fg(Color::Green).bold();
    let group_focused = Style::new().underline();
    let label_style = Style::new().fg(Color::Blue);
    let label_focused = Style::new().fg(Color::Yellow);
    let group = BorderLabels::new()
        .style(group_style)
        .focused_style(group_focused);
    let label = FrameLabel::new("label")
        .style(label_style)
        .focused_style(label_focused);

    let unfocused = crate::backend::ratatui_backend::renderers::frame::render::resolve_label_style(
        base, &group, &label, false,
    );
    assert_eq!(unfocused.fg, label_style.fg);
    assert_eq!(unfocused.bold, group_style.bold);
    assert_eq!(unfocused.underline, None);

    let focused = crate::backend::ratatui_backend::renderers::frame::render::resolve_label_style(
        base, &group, &label, true,
    );
    assert_eq!(focused.fg, label_focused.fg);
    assert_eq!(focused.bold, group_style.bold);
    assert_eq!(focused.underline, group_focused.underline);
}

#[test]
fn tabs_remain_after_a_grouped_header_prefix() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 42,
        h: 3,
    };
    let mut runtime = RuntimeCore::new_test(
        TabsWithHeaderLabelComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);
    let row = (0..viewport.w)
        .map(|x| buffer[(x, 0)].symbol().to_owned())
        .collect::<String>();

    assert!(
        row.contains("─[2]─Files - Worktrees - Submodule"),
        "rendered header: {row:?}"
    );
}

#[test]
fn header_label_padding_uses_border_style() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 3,
    };
    let mut runtime = RuntimeCore::new_test(
        HeaderLabelPaddingStyleComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);

    assert_eq!(buffer[(1, 0)].fg, ratatui::style::Color::Red);
    assert_eq!(buffer[(2, 0)].fg, ratatui::style::Color::Blue);
    assert_eq!(buffer[(3, 0)].fg, ratatui::style::Color::Red);
}

impl Component for EffectScopeRenderComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(
                EffectScope::new()
                    .transform_fg(ColorTransform::Dim(0.5))
                    .child(Text::new("A").style(Style::new().fg(Color::rgb(100, 120, 140)))),
            )
            .child(
                EffectScope::new()
                    .contrast_policy(ContrastPolicy::Wcag)
                    .child(
                        Text::new("B").style(
                            Style::new()
                                .fg(Color::rgb(20, 20, 20))
                                .bg(Color::rgb(0, 0, 0)),
                        ),
                    ),
            )
            .into()
    }
}

impl Component for EffectScopeColorTransformFgOnlyComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::ColorTransform {
                fg: Some(ColorTransform::Dim(0.5)),
                bg: None,
            })
            .child(
                Text::new("C").style(
                    Style::new()
                        .fg(Color::rgb(100, 120, 140))
                        .bg(Color::rgb(10, 20, 30)),
                ),
            )
            .into()
    }
}

impl Component for EffectScopeMonochromeComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::Monochrome { strength: 1.0 })
            .child(
                Text::new("M").style(
                    Style::new()
                        .fg(Color::rgb(20, 200, 40))
                        .bg(Color::rgb(200, 20, 40)),
                ),
            )
            .into()
    }
}

impl Component for EffectScopePaletteQuantizeComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::PaletteQuantize {
                palette: EffectPalette::Custom(vec![Color::rgb(0, 0, 0), Color::rgb(255, 0, 0)]),
            })
            .child(
                Text::new("Q").style(
                    Style::new()
                        .fg(Color::rgb(20, 200, 20))
                        .bg(Color::rgb(250, 30, 30)),
                ),
            )
            .into()
    }
}

impl Component for EffectScopeRainbowWaveComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::RainbowWave {
                blend: 1.0,
                frequency: 1.0,
                speed: 1.0,
                axis: EffectAxis::Horizontal,
            })
            .child(Text::new("R").style(Style::new().fg(Color::rgb(80, 80, 80))))
            .into()
    }
}

impl Component for EffectScopeResetSkipComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::Monochrome { strength: 1.0 })
            .effect(VisualEffect::PaletteQuantize {
                palette: EffectPalette::Amber,
            })
            .effect(VisualEffect::tint(Color::rgb(255, 0, 0), 0.8))
            .effect(VisualEffect::RainbowWave {
                blend: 1.0,
                frequency: 1.0,
                speed: 1.0,
                axis: EffectAxis::Horizontal,
            })
            .child(Text::new("S").style(Style::new().fg(Color::Reset).bg(Color::Reset)))
            .into()
    }
}

impl Component for EffectScopeScanlinesComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::Scanlines {
                strength: 0.5,
                spacing: 2,
            })
            .child(
                VStack::new()
                    .child(Text::new("A").style(Style::new().fg(Color::rgb(100, 120, 140))))
                    .child(Text::new("B").style(Style::new().fg(Color::rgb(100, 120, 140))))
                    .child(Text::new("C").style(Style::new().fg(Color::rgb(100, 120, 140)))),
            )
            .into()
    }
}

impl Component for EffectScopeNestedRootPortalComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .transform_fg(ColorTransform::Dim(0.5))
            .child(
                VStack::new()
                    .child(Text::new("base").style(Style::new().fg(Color::rgb(100, 120, 140))))
                    .child(
                        Modal::new()
                            .width(Length::Px(3))
                            .height(Length::Px(1))
                            .padding(0)
                            .border(false)
                            .frame_style(Style::new().bg(Color::Transparent))
                            .child(
                                Text::new("M").style(Style::new().fg(Color::rgb(100, 120, 140))),
                            ),
                    ),
            )
            .into()
    }
}

impl Component for EffectScopeWrappedComponentRootPortalComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ZStack::new()
            .child(Text::new("BBBBBBBBB").style(Style::new().fg(Color::rgb(100, 120, 140))))
            .child(
                EffectScope::new()
                    .transform_fg(ColorTransform::Dim(0.5))
                    .child(crate::child::<RootPortalModalOnlyComponent, _>(
                        || RootPortalModalOnlyComponent,
                        (),
                    )),
            )
            .into()
    }
}

impl Component for RootPortalModalOnlyComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Modal::new()
            .width(Length::Px(3))
            .height(Length::Px(1))
            .padding(0)
            .border(false)
            .frame_style(Style::new().bg(Color::Transparent))
            .child(Text::new("M").style(Style::new().fg(Color::rgb(100, 120, 140))))
            .into()
    }
}

impl Component for NestedEffectScopeCompositionComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .effect(VisualEffect::tint(Color::rgb(255, 0, 0), 0.5))
            .child(
                EffectScope::new()
                    .effect(VisualEffect::Monochrome { strength: 1.0 })
                    .child(Text::new("N").style(Style::new().fg(Color::rgb(0, 200, 0)))),
            )
            .into()
    }
}

impl Component for DevToolsTopmostAppBackdropComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .dim_by(0.75)
            .child(
                VStack::new()
                    .child(Text::new("app").style(Style::new().fg(Color::Blue)))
                    .child(
                        Modal::new()
                            .width(Length::Px(8))
                            .height(Length::Px(3))
                            .padding(0)
                            .border(false)
                            .backdrop_style(
                                Style::new().bg(Color::Black).fg(Color::Red).dim_by(0.75),
                            )
                            .child(Text::new("MODAL")),
                    ),
            )
            .into()
    }
}

#[cfg(feature = "devtools")]
impl Component for DevToolsBorderUnderlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .border(true)
            .border_style(BorderStyle::Plain)
            .child(Text::new("app"))
            .into()
    }
}

impl Component for AnimatedOpacityFadeComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Animated::new(Text::new("XX").style(Style::new().fg(Color::White).bg(Color::indexed(236))))
            .opacity(0.0)
            .into()
    }
}

impl Component for AnimatedOpacityZeroZStackComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ZStack::new()
            .child(Text::new("UNDER").style(Style::new().fg(Color::LightCyan)))
            .child(
                Animated::new(
                    Text::new("XXXXX").style(Style::new().fg(Color::White).bg(Color::indexed(236))),
                )
                .opacity(0.0),
            )
            .into()
    }
}

impl Component for AnimatedOpacityZeroFgOnlyZStackComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ZStack::new()
            .child(Text::new("UNDER").style(Style::new().fg(Color::LightCyan)))
            .child(
                Animated::new(Text::new("XXXXX").style(Style::new().fg(Color::White)))
                    .opacity(0.0)
                    .opacity_fg_only(true),
            )
            .into()
    }
}

impl Component for AnimatedOpacityHalfUnderlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .style(Style::new().bg(Color::rgb(20, 40, 60)))
            .child(
                Animated::new(
                    Text::new("XX").style(
                        Style::new()
                            .fg(Color::rgb(200, 200, 200))
                            .bg(Color::rgb(100, 100, 100)),
                    ),
                )
                .opacity(0.5),
            )
            .into()
    }
}

impl Component for AnimatedColorTargetComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Animated::new(Text::new("Z").style(Style::new().fg(Color::White).bg(Color::Black)))
            .fg(if self.active.get() {
                Color::rgb(110, 120, 130)
            } else {
                Color::rgb(10, 20, 30)
            })
            .bg(if self.active.get() {
                Color::rgb(130, 140, 150)
            } else {
                Color::rgb(30, 40, 50)
            })
            .transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .into()
    }
}

impl Component for AnimatedPositionOffsetComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Spacer::new().height(Length::Px(1)))
            .child(
                HStack::new()
                    .child(Spacer::new().width(Length::Px(3)))
                    .child(
                        Animated::new(
                            Text::new("X").style(Style::new().fg(Color::White).bg(Color::Black)),
                        )
                        .opacity(self.opacity),
                    ),
            )
            .into()
    }
}

impl Component for CompactFramePaintLeakComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .compact(true)
            .border(true)
            .height(Length::Px(2))
            .style(Style::new().bg(Color::DarkGray))
            .header_left("Compact")
            .child(Text::new("body"))
            .into()
    }
}

impl Component for CompactFrameStatusRightComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .compact(true)
            .border(true)
            .header_padding(1)
            .footer_padding(1)
            .header_left("Files")
            .footer_right("1 of 6")
            .child(Text::new("body"))
            .into()
    }
}

impl Component for OffscreenModalOverlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("AAAAAAA").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("BBBBBBB").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("CCCCCCC").style(Style::new().fg(Color::LightCyan)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .border(false)
                    .frame_style(Style::new().bg(Color::Transparent))
                    .child(Text::new("OVER!")),
            )
            .into()
    }
}

impl Component for TransparentModalOverlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("012345678").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("ABCDEFGHI").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("JKLMNOPQR").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("STUVWXYZ1").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("234567890").style(Style::new().fg(Color::LightCyan)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .border(false)
                    .frame_style(Style::new().bg(Color::Transparent))
                    .child(Spacer::new()),
            )
            .into()
    }
}

impl Component for ExplicitOverlayBackgroundPaintComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("012345678").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("ABCDEFGHI").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("JKLMNOPQR").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("STUVWXYZ1").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("234567890").style(Style::new().fg(Color::LightCyan)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .border(false)
                    .frame_style(Style::new().bg(Color::Transparent))
                    .child(Text::new("     ").style(Style::new().bg(Color::Red))),
            )
            .into()
    }
}

impl Component for ToastTransitionUnderlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .style(Style::new().bg(Color::rgb(20, 40, 60)))
            .child(Spacer::new())
            .into()
    }
}

impl Component for ToastSurfaceBandsComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        // One distinctly coloured row per line, so anything that flattens the underlay is obvious.
        let band = |color: Color| -> crate::core::element::Element {
            VStack::new()
                .style(Style::new().bg(color))
                .height(Length::Px(3))
                .child(Spacer::new())
                .into()
        };
        VStack::new()
            .child(band(Color::rgb(255, 0, 0)))
            .child(band(Color::rgb(0, 255, 0)))
            .child(band(Color::rgb(0, 0, 255)))
            .child(band(Color::rgb(255, 255, 0)))
            .child(band(Color::rgb(255, 0, 255)))
            .into()
    }
}

impl Component for ToastTransitionDefaultUnderlayComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Spacer::new().into()
    }
}

impl Component for ExplicitOverlayForegroundOnlySpacesComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("012345678").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("ABCDEFGHI").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("JKLMNOPQR").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("STUVWXYZ1").style(Style::new().fg(Color::LightCyan)))
            .child(Text::new("234567890").style(Style::new().fg(Color::LightCyan)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .border(false)
                    .frame_style(Style::new().bg(Color::Transparent))
                    .child(Text::new("     ").style(Style::new().fg(Color::Red))),
            )
            .into()
    }
}

impl Component for TransparentModalBorderOverColoredBackgroundComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("AAAAAAAAA").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("BBBBBBBBB").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("CCCCCCCCC").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("DDDDDDDDD").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("EEEEEEEEE").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .frame_style(Style::new().fg(Color::Red).bg(Color::Transparent))
                    .child(Spacer::new()),
            )
            .into()
    }
}

impl Component for DefaultModalBackdropClearsForegroundComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("AAAAAAAAA").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("BBBBBBBBB").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("CCCCCCCCC").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("DDDDDDDDD").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(Text::new("EEEEEEEEE").style(Style::new().fg(Color::White).bg(Color::Blue)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .border(false)
                    .child(Spacer::new()),
            )
            .into()
    }
}

impl Component for TransparentModalBorderPreservesUnderlyingForegroundComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("AAAAAAAAA").style(Style::new().fg(Color::Yellow).bg(Color::Blue)))
            .child(Text::new("BBBBBBBBB").style(Style::new().fg(Color::Yellow).bg(Color::Blue)))
            .child(Text::new("CCCCCCCCC").style(Style::new().fg(Color::Yellow).bg(Color::Blue)))
            .child(Text::new("DDDDDDDDD").style(Style::new().fg(Color::Yellow).bg(Color::Blue)))
            .child(Text::new("EEEEEEEEE").style(Style::new().fg(Color::Yellow).bg(Color::Blue)))
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .padding(0)
                    .frame_style(Style::new().fg(Color::Transparent).bg(Color::Transparent))
                    .child(Spacer::new()),
            )
            .into()
    }
}

impl Component for TransparentFrameDecorationPreservesUnderlyingBackgroundComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ZStack::new()
            .child(
                VStack::new()
                    .child(Text::new("AAAAAAAAA").style(Style::new().bg(Color::Blue)))
                    .child(Text::new("BBBBBBBBB").style(Style::new().bg(Color::Blue)))
                    .child(Text::new("CCCCCCCCC").style(Style::new().bg(Color::Blue)))
                    .child(Text::new("DDDDDDDDD").style(Style::new().bg(Color::Blue)))
                    .child(Text::new("EEEEEEEEE").style(Style::new().bg(Color::Blue))),
            )
            .child(
                Frame::new()
                    .width(Length::Px(5))
                    .height(Length::Px(5))
                    .style(Style::new().bg(Color::Red))
                    .decoration(
                        EdgeDecoration::new(Edge::Left)
                            .glyph(DecorationGlyph::AutoBlock)
                            .style(Style::new().fg(Color::Yellow).bg(Color::Transparent)),
                    )
                    .decoration(
                        EdgeDecoration::new(Edge::Bottom)
                            .glyph(DecorationGlyph::HalfBlock)
                            .placement(DecorationPlacement::Outside)
                            .style(Style::new().fg(Color::Yellow).bg(Color::Transparent))
                            .cap_end(DecorationGlyph::CapBottom),
                    )
                    .child(Spacer::new()),
            )
            .into()
    }
}

impl Component for FrameHoverUnderModalComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ZStack::new()
            .child(
                Frame::new()
                    .border(false)
                    .padding(0)
                    .style(Style::new().bg(Color::Green))
                    .hover_style(Style::new().bg(Color::Red))
                    .child(Spacer::new()),
            )
            .child(
                Modal::new()
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .border(false)
                    .padding(0)
                    .child(
                        Frame::new()
                            .border(false)
                            .padding(0)
                            .style(Style::new().bg(Color::Blue))
                            .hover_style(Style::new().bg(Color::Yellow))
                            .child(Spacer::new()),
                    ),
            )
            .into()
    }
}

impl Component for ButtonAlphaHoverComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let panel_bg = Color::Rgb(0x15, 0x15, 0x19);
        let alpha_fg = Paint::rgba(0xff, 0xff, 0xff, 0x40);

        Frame::new()
            .border(false)
            .padding(0)
            .style(Style::new().bg(panel_bg))
            .child(
                Button::filled("Hover")
                    .style(Style::new().bg(panel_bg).fg(Color::White))
                    .hover_style(
                        Style::new()
                            .fg(alpha_fg)
                            .contrast_policy(ContrastPolicy::Off),
                    ),
            )
            .into()
    }
}

struct EffectScopeDimPreservesTerminalBlendBorderComponent;

impl Component for EffectScopeDimPreservesTerminalBlendBorderComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        EffectScope::new()
            .dim_by(0.3)
            .child(
                Frame::new()
                    .padding(0)
                    .style(Style::new().fg(Color::Transparent).bg(Color::Transparent))
                    .child(Spacer::new()),
            )
            .into()
    }
}

#[test]
fn frame_header_renders_on_border_row() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        HeaderFrameComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let mut top_row = String::new();
    for x in 0..viewport.w {
        top_row.push_str(buffer[(x, 0)].symbol());
    }

    assert!(
        top_row.contains("Search"),
        "expected header content on top border row, got: {top_row:?}"
    );
}

#[test]
fn effect_scope_applies_post_render_style_effects() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 2,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeRenderComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(50, 60, 70));

    let adjusted_fg = match buffer[(0, 1)].fg {
        ratatui::style::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
        other => panic!("expected RGB fg after contrast adjustment, got {other:?}"),
    };
    let adjusted_bg = match buffer[(0, 1)].bg {
        ratatui::style::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
        other => panic!("expected RGB bg after contrast adjustment, got {other:?}"),
    };
    assert!(contrast_ratio(adjusted_fg, adjusted_bg) >= 4.5);
}

#[test]
fn visual_effect_color_transform_can_target_foreground_only() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 1,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeColorTransformFgOnlyComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let cell = &terminal.backend().buffer()[(0, 0)];
    assert_eq!(cell.fg, ratatui::style::Color::Rgb(50, 60, 70));
    assert_eq!(cell.bg, ratatui::style::Color::Rgb(10, 20, 30));
}

#[test]
fn effect_scope_monochrome_remaps_fg_bg() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeMonochromeComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(128, 128, 128));
    assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(76, 76, 76));
}

#[test]
fn effect_scope_palette_quantize_clamps_to_palette() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopePaletteQuantizeComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(0, 0, 0));
    assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(255, 0, 0));
}

#[test]
fn effect_scope_rainbow_wave_phase_is_deterministic() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeRainbowWaveComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let draw_fg_at_phase = |phase| {
        let backend = TestBackend::new(viewport.w, viewport.h);
        let mut terminal = Terminal::new(backend).expect("terminal should init");
        let ctx = RenderContext {
            tree: &runtime.tree,
            focused: None,
            hovered: None,
            mouse_pos: None,
            suppress_pointer_item_hover_nodes: None,
            blink_visible: true,
            effect_phase: phase,
            images_enabled: true,
            contrast_policy: ContrastPolicy::Off,
            read_only_selection: None,
            scrollbar_metrics_cache: &RefCell::new(Default::default()),
            overlay_bg_snapshot: &RefCell::new(Vec::new()),
            join_index: &build_join_index(&runtime.tree),
            cursor_position: &Cell::new(None),
            terminal_bg: None,
            drag_preview_label: None,
            drag_preview_at_mouse: false,
            drag_preview_snapshot_rect: None,
            dnd_snapshot_cells: &RefCell::new(None),
            drag_preview_max_width: None,
            drag_preview_max_height: None,
            drag_preview_grab_offset: None,
            drop_slot_source_preview_rect: None,
            paint_glyph_caches: None,
            copy_feedback: None,
            copy_feedback_style: Style::default(),
        };
        terminal
            .draw(|f| render(f, &ctx))
            .expect("render should succeed");
        terminal.backend().buffer()[(0, 0)].fg
    };

    let p0a = draw_fg_at_phase(0);
    let p0b = draw_fg_at_phase(0);
    let p11 = draw_fg_at_phase(11);

    assert_eq!(p0a, p0b);
    assert_ne!(p0a, p11);
}

#[test]
fn effect_scope_reset_cells_skipped_by_color_remap() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeResetSkipComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 11,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Reset);
    assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Reset);
}

#[test]
fn effect_scope_scanlines_dim_only_matching_rows() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 3,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeScanlinesComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(50, 60, 70));
    assert_eq!(buffer[(0, 1)].fg, ratatui::style::Color::Rgb(100, 120, 140));
    assert_eq!(buffer[(0, 2)].fg, ratatui::style::Color::Rgb(50, 60, 70));
}

#[test]
fn effect_scope_ancestor_applies_to_nested_root_portal_overlay() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeNestedRootPortalComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let overlay_id = runtime
        .tree
        .overlay_roots()
        .first()
        .expect("overlay root should exist")
        .id;
    let overlay_rect = runtime.tree.node(overlay_id).rect;

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let x = overlay_rect.x as u16;
    let y = overlay_rect.y as u16;
    assert_eq!(buffer[(x, y)].symbol(), "M");
    assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Rgb(50, 60, 70));
}

#[test]
fn effect_scope_wrapping_component_root_portal_does_not_affect_backdrop_area() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeWrappedComponentRootPortalComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let overlay_id = runtime
        .tree
        .overlay_roots()
        .first()
        .expect("overlay root should exist")
        .id;
    let overlay_rect = runtime.tree.node(overlay_id).rect;

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "B");
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(100, 120, 140));

    let x = overlay_rect.x as u16;
    let y = overlay_rect.y as u16;
    assert_eq!(buffer[(x, y)].symbol(), "M");
    assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Rgb(50, 60, 70));
}

#[test]
fn nested_effect_scope_composition_order_inner_then_outer() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        NestedEffectScopeCompositionComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Rgb(186, 59, 59));
}

#[test]
fn transparent_modal_overlay_preserves_underlying_cells() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        TransparentModalOverlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let center_cell = &buffer[(4, 2)];
    assert_eq!(center_cell.symbol(), "N");
    assert_eq!(center_cell.fg, ratatui::style::Color::LightCyan);
}

#[test]
fn overlay_clear_is_clipped_when_overlay_extends_below_viewport() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 7,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        OffscreenModalOverlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let overlay_id = runtime
        .tree
        .overlay_roots()
        .first()
        .expect("overlay root should exist")
        .id;
    runtime.tree.node_mut(overlay_id).rect.y = 2;

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed even when overlay clear rect is off-screen");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "A");
}

#[test]
fn explicit_overlay_background_paint_is_not_restored() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ExplicitOverlayBackgroundPaintComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let painted_cell = &buffer[(4, 1)];
    assert_eq!(painted_cell.symbol(), " ");
    assert_eq!(painted_cell.bg, ratatui::style::Color::Red);
}

#[test]
fn toast_transition_blends_against_the_rendered_underlay() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ToastTransitionUnderlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.overlay_manager.borrow_mut().push_toast(
        Toast::new("T")
            .border(false)
            .width(Length::Px(3))
            .height(Length::Px(1))
            .frame_style(Style::new().bg(Color::rgb(220, 40, 60))),
    );
    runtime.render_element(viewport, None, None, None);

    let mut overlays = runtime.tree.overlay_roots().to_vec();
    let toast_rect = runtime.tree.node(overlays[0].id).rect;
    let first_frame = render_runtime_with_hover(&runtime, viewport, None, None);
    assert_eq!(
        first_frame[(toast_rect.x as u16, toast_rect.y as u16)].bg,
        ratatui::style::Color::Rgb(20, 40, 60)
    );

    overlays[0].opacity = 0.5;
    runtime.tree.set_overlay_roots(overlays);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let terminal_bg = ratatui::style::Color::Rgb(0, 0, 0);
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(terminal_bg),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let cell = &terminal.backend().buffer()[(toast_rect.x as u16, toast_rect.y as u16)];
    assert_eq!(cell.bg, ratatui::style::Color::Rgb(120, 40, 60));
    assert_ne!(cell.bg, terminal_bg);
}

#[test]
fn toast_transition_dims_foreground_only_decorations_over_terminal_background() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ToastTransitionDefaultUnderlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.overlay_manager.borrow_mut().push_toast(
        Toast::new("T")
            .border(false)
            .width(Length::Px(3))
            .height(Length::Px(3))
            .decoration(
                EdgeDecoration::new(Edge::Left)
                    .glyph(DecorationGlyph::AutoHeavy)
                    .style(Style::new().fg(Color::LightBlue)),
            ),
    );
    runtime.render_element(viewport, None, None, None);

    let mut overlays = runtime.tree.overlay_roots().to_vec();
    overlays[0].opacity = 0.5;
    runtime.tree.set_overlay_roots(overlays);

    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);
    let decoration = buffer
        .content
        .iter()
        .find(|cell| cell.fg == ratatui::style::Color::LightBlue && cell.symbol() != " ")
        .expect("toast decoration should render");
    assert!(
        decoration.modifier.contains(ratatui::style::Modifier::DIM),
        "foreground-only decoration should participate in partial opacity"
    );
}

#[test]
fn explicit_overlay_spaces_with_only_fg_are_restored() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ExplicitOverlayForegroundOnlySpacesComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let restored_cell = &buffer[(4, 1)];
    assert_eq!(restored_cell.symbol(), "E");
    assert_eq!(restored_cell.fg, ratatui::style::Color::LightCyan);
}

#[test]
fn transparent_modal_border_preserves_underlying_background() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        TransparentModalBorderOverColoredBackgroundComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let border_cell = &buffer[(2, 1)];
    assert_eq!(border_cell.symbol(), "┌");
    assert_eq!(border_cell.fg, ratatui::style::Color::Red);
    assert_eq!(border_cell.bg, ratatui::style::Color::Blue);
}

#[test]
fn default_modal_backdrop_clears_fg_but_keeps_underlying_bg() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        DefaultModalBackdropClearsForegroundComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let cleared_cell = &buffer[(4, 2)];
    assert_eq!(cleared_cell.symbol(), " ");
    assert_eq!(cleared_cell.fg, ratatui::style::Color::Reset);
    assert_eq!(cleared_cell.bg, ratatui::style::Color::Blue);
}

#[test]
fn frame_hover_uses_overlay_aware_hover_target() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        FrameHoverUnderModalComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let mouse_pos = (4, 2);
    let hovered = runtime
        .tree
        .hover_test(mouse_pos.0 as i16, mouse_pos.1 as i16);
    assert!(hovered.is_some(), "modal frame should be hover target");

    let buffer = render_runtime_with_hover(&runtime, viewport, hovered, Some(mouse_pos));

    assert_eq!(
        buffer[(0, 0)].bg,
        ratatui::style::Color::Green,
        "underlying frame must not receive hover behind modal backdrop",
    );
    assert_eq!(
        buffer[(4, 2)].bg,
        ratatui::style::Color::Yellow,
        "top modal frame should still receive hover",
    );
}

#[test]
fn button_alpha_hover_renders_from_runtime_hover_target() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        ButtonAlphaHoverComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let mouse_pos = (1, 0);
    let hovered = runtime
        .tree
        .hover_test(mouse_pos.0 as i16, mouse_pos.1 as i16);
    assert!(
        hovered.is_some_and(|id| matches!(runtime.tree.node(id).kind, NodeKind::Button(_))),
        "button should be the runtime hover target"
    );

    let buffer = render_runtime_with_hover(&runtime, viewport, hovered, Some(mouse_pos));
    let expected = ratatui::style::Color::Rgb(0x50, 0x50, 0x53);

    assert_eq!(buffer[(1, 0)].symbol(), "H");
    assert_eq!(buffer[(1, 0)].fg, expected);
}

#[test]
fn transparent_modal_border_preserves_underlying_foreground() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        TransparentModalBorderPreservesUnderlyingForegroundComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let border_cell = &buffer[(2, 1)];
    assert_eq!(border_cell.symbol(), "┌");
    assert_eq!(border_cell.fg, ratatui::style::Color::Yellow);
    assert_eq!(border_cell.bg, ratatui::style::Color::Blue);
}

#[test]
fn transparent_frame_decoration_preserves_underlying_background() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        TransparentFrameDecorationPreservesUnderlyingBackgroundComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let left_decoration_cell = &buffer[(0, 2)];
    assert_eq!(left_decoration_cell.fg, ratatui::style::Color::Yellow);
    assert_eq!(left_decoration_cell.bg, ratatui::style::Color::Blue);
}

#[test]
fn extra_root_renders_above_app_modal_backdrop_and_effect_scope() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    };
    let mut runtime = RuntimeCore::new_test(
        DevToolsTopmostAppBackdropComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.extra_root_element = Some(
        Text::new("DEVTOOLS")
            .style(Style::new().fg(Color::Green).bg(Color::Reset))
            .into(),
    );
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "D");
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Green);
}

#[cfg(feature = "devtools")]
#[test]
fn devtools_border_does_not_merge_with_app_layer_border() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 20,
    };
    let mut runtime = RuntimeCore::new_test(
        DevToolsBorderUnderlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    let state = Rc::new(RefCell::new(crate::devtools::DevToolsState::default()));
    state.borrow_mut().set_visible(true);
    runtime.extra_root_element = Some(crate::devtools::panel_element(state));
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let panel = runtime
        .tree
        .iter()
        .find(|node| {
            node.key
                .as_ref()
                .is_some_and(|key| key.as_ref() == crate::devtools::DEVTOOLS_KEY)
        })
        .expect("devtools panel should exist");
    let panel_bottom_right = (
        panel
            .rect
            .x
            .saturating_add(panel.rect.w as i16)
            .saturating_sub(1) as u16,
        panel
            .rect
            .y
            .saturating_add(panel.rect.h as i16)
            .saturating_sub(1) as u16,
    );
    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);

    assert_eq!(
        buffer[panel_bottom_right].symbol(),
        "╯",
        "the DevTools rounded corner must replace, not merge with, the app border beneath it"
    );
}

#[test]
fn effect_scope_dim_keeps_case2_resolved_fg_matching_dimmed_backdrop() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        EffectScopeDimPreservesTerminalBlendBorderComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let terminal_bg = ratatui::style::Color::Rgb(40, 42, 44);
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(terminal_bg),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let border_cell = &buffer[(0, 0)];
    assert_eq!(border_cell.symbol(), "┌");
    // The dim transform resolves the default (Reset) background against the
    // terminal bg and darkens it, so the case2-resolved transparent-border fg
    // must be darkened identically: fg and bg staying equal is what keeps the
    // border invisible under the dim.
    let dimmed = crate::backend::ratatui_backend::common::dim_ratatui_color(terminal_bg, 0.3);
    assert_eq!(border_cell.fg, dimmed);
    assert_eq!(border_cell.bg, dimmed);
}

#[test]
fn animated_opacity_zero_restores_pre_render_cells() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedOpacityFadeComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let terminal_bg = ratatui::style::Color::Rgb(40, 42, 44);
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(terminal_bg),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let faded_cell = &buffer[(0, 0)];
    assert_eq!(faded_cell.symbol(), " ");
    assert_eq!(faded_cell.bg, ratatui::style::Color::Reset);
    assert_eq!(faded_cell.fg, ratatui::style::Color::Reset);
}

#[test]
fn animated_opacity_zero_does_not_cover_zstack_content() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 5,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedOpacityZeroZStackComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(ratatui::style::Color::Rgb(40, 42, 44)),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "U");
    assert_eq!(buffer[(1, 0)].symbol(), "N");
    assert_eq!(buffer[(2, 0)].symbol(), "D");
    assert_eq!(buffer[(3, 0)].symbol(), "E");
    assert_eq!(buffer[(4, 0)].symbol(), "R");
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::LightCyan);
}

#[test]
fn animated_opacity_zero_fg_only_does_not_cover_zstack_content() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 5,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedOpacityZeroFgOnlyZStackComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(ratatui::style::Color::Rgb(40, 42, 44)),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "U");
    assert_eq!(buffer[(1, 0)].symbol(), "N");
    assert_eq!(buffer[(2, 0)].symbol(), "D");
    assert_eq!(buffer[(3, 0)].symbol(), "E");
    assert_eq!(buffer[(4, 0)].symbol(), "R");
    assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::LightCyan);
}

#[test]
fn animated_opacity_blends_toward_underlay_not_terminal_bg() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 2,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedOpacityHalfUnderlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(ratatui::style::Color::Rgb(0, 0, 0)),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let cell = &buffer[(0, 0)];
    assert_eq!(cell.symbol(), "X");
    assert_eq!(cell.bg, ratatui::style::Color::Rgb(60, 70, 80));
    assert_eq!(cell.fg, ratatui::style::Color::Rgb(130, 135, 140));
}

#[test]
fn animated_color_targets_override_rendered_cell_channels() {
    let active = Rc::new(Cell::new(false));
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedColorTargetComponent {
            active: active.clone(),
        },
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    active.set(true);
    runtime.render_element(viewport, None, None, None);

    let animated_id = runtime
        .tree
        .iter()
        .find_map(|node| matches!(&node.kind, NodeKind::Animated(_)).then_some(node.id))
        .expect("animated node should exist");
    match &mut runtime.tree.node_mut(animated_id).kind {
        NodeKind::Animated(animated) => {
            let result = animated.tick(Duration::from_millis(50));
            assert!(result.paint_dirty);
            assert!(!result.layout_dirty);
        }
        _ => panic!("expected animated node"),
    }

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let cell = &buffer[(0, 0)];
    assert_eq!(cell.symbol(), "Z");
    assert_eq!(cell.fg, ratatui::style::Color::Rgb(60, 70, 80));
    assert_eq!(cell.bg, ratatui::style::Color::Rgb(80, 90, 100));
}

#[test]
fn compact_frame_only_paints_the_visible_row() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 16,
        h: 3,
    };
    let mut runtime = RuntimeCore::new_test(
        CompactFramePaintLeakComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::DarkGray);
    assert_eq!(buffer[(0, 1)].bg, ratatui::style::Color::Reset);
}

#[test]
fn compact_frame_with_footer_right_fills_the_visible_row() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 1,
    };
    let mut runtime = RuntimeCore::new_test(
        CompactFrameStatusRightComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let mut row = String::new();
    for x in 0..viewport.w {
        row.push_str(buffer[(x, 0)].symbol());
    }

    assert_eq!(row, "──Files─────────1 of 6──");
}

struct IntegratedListScrollbarComponent;

struct HorizontalCapsFrameComponent;

impl Component for HorizontalCapsFrameComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(10))
            .height(Length::Px(4))
            .border_edges(BorderEdges::HorizontalCaps)
            .child(Text::new("abcdefghij"))
            .into()
    }
}

#[test]
fn horizontal_caps_frame_renders_corners_without_vertical_edges() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        HorizontalCapsFrameComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..viewport.h)
        .map(|y| {
            (0..viewport.w)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();

    assert_eq!(rows[0], "┌────────┐");
    assert_eq!(rows[1], "abcdefghij");
    assert_eq!(rows[3], "└────────┘");
}

impl Component for IntegratedListScrollbarComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let items: Vec<ListItem> = (0..20).map(|i| ListItem::new(format!("row {i}"))).collect();

        Frame::new()
            .style(Style::new().fg(Color::DarkGray))
            .focus_style(Style::new().fg(Color::Cyan))
            .child(
                List::new()
                    .items(items)
                    .focusable(false)
                    .scrollbar(true)
                    .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Integrated)),
            )
            .into()
    }
}

#[test]
fn integrated_scrollbar_track_uses_parent_frame_border_style() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    };
    let mut runtime = RuntimeCore::new_test(
        IntegratedListScrollbarComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let track_cell = &buffer[(viewport.w - 1, viewport.h - 2)];

    assert_eq!(
        track_cell.fg,
        ratatui::style::Color::DarkGray,
        "integrated scrollbar track should inherit frame border fg"
    );
}

struct FrameBorderMergeExactComponent;

impl Component for FrameBorderMergeExactComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(8))
            .height(Length::Px(4))
            .decoration(
                EdgeDecoration::new(Edge::Bottom)
                    .placement(DecorationPlacement::Border)
                    .glyph(DecorationGlyph::Custom('┏')),
            )
            .child(Text::new("body"))
            .into()
    }
}

struct FrameBorderMergeReplaceComponent;

impl Component for FrameBorderMergeReplaceComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .width(Length::Px(8))
            .height(Length::Px(4))
            .border_merge_mode(BorderMergeMode::Replace)
            .decoration(
                EdgeDecoration::new(Edge::Bottom)
                    .placement(DecorationPlacement::Border)
                    .glyph(DecorationGlyph::Custom('┏')),
            )
            .child(Text::new("body"))
            .into()
    }
}

#[test]
fn frame_border_overlap_merges_symbols_by_default() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 6,
    };
    let mut runtime = RuntimeCore::new_test(
        FrameBorderMergeExactComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let frame_rect = runtime.tree.node(runtime.tree.root).rect;
    let corner_x = frame_rect
        .x
        .saturating_add(frame_rect.w as i16)
        .saturating_sub(1) as u16;
    let corner_y = frame_rect
        .y
        .saturating_add(frame_rect.h as i16)
        .saturating_sub(1) as u16;
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(corner_x, corner_y)].symbol(),
        "╆",
        "bottom-right corner should merge with overlapping border decoration"
    );
}

#[test]
fn frame_border_overlap_replace_strategy_keeps_last_symbol() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 6,
    };
    let mut runtime = RuntimeCore::new_test(
        FrameBorderMergeReplaceComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let frame_rect = runtime.tree.node(runtime.tree.root).rect;
    let corner_x = frame_rect
        .x
        .saturating_add(frame_rect.w as i16)
        .saturating_sub(1) as u16;
    let corner_y = frame_rect
        .y
        .saturating_add(frame_rect.h as i16)
        .saturating_sub(1) as u16;
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(corner_x, corner_y)].symbol(),
        "┏",
        "replace strategy should keep the last overlapping symbol"
    );
}

struct OverlappingBorderTitlePreserveComponent;

impl Component for OverlappingBorderTitlePreserveComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        // Lower titled frame first, untitled upper frame last (bottom edge shares the title row).
        Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 3,
                    w: 20,
                    h: 5,
                },
                Frame::new()
                    .border(true)
                    .border_merge_mode(BorderMergeMode::Fuzzy)
                    .header(BorderLabels::new().left("KEEP  ME").padding(1))
                    .child(Text::new("below")),
            )
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 20,
                    h: 4,
                },
                Frame::new()
                    .border(true)
                    .border_merge_mode(BorderMergeMode::Fuzzy)
                    .style(Style::new().fg(Color::Cyan))
                    .child(Text::new("above")),
            )
            .into()
    }
}

#[test]
fn fuzzy_merge_preserves_neighbor_border_title_on_shared_seam() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        OverlappingBorderTitlePreserveComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let mut seam = String::new();
    for x in 0..20 {
        seam.push_str(buffer[(x, 3)].symbol());
    }
    assert!(
        seam.contains("KEEP  ME"),
        "later overlapping bottom border must not wipe titled seam spaces: {seam:?}"
    );
}

struct BorderOnStyledBackdropComponent;

impl Component for BorderOnStyledBackdropComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        // Parent fill with an explicit fg (like hyprmux's theme.primary + backdrop) must not
        // suppress Fuzzy border drawing on those styled blank cells.
        ZStack::new()
            .style(Style::new().fg(Color::Yellow).bg(Color::Black))
            .child(
                Frame::new()
                    .border(true)
                    .border_merge_mode(BorderMergeMode::Fuzzy)
                    .header(BorderLabels::new().left("X  Y").padding(1))
                    .child(Text::new("body")),
            )
            .into()
    }
}

#[test]
fn fuzzy_border_still_draws_over_styled_backdrop_spaces() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        BorderOnStyledBackdropComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let buffer = terminal.backend().buffer();
    let top: String = (0..12).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        top.starts_with('┌') || top.starts_with('╭'),
        "corner must survive a styled backdrop fill: {top:?}"
    );
    assert!(
        top.contains("X  Y"),
        "title gap spaces must remain: {top:?}"
    );
    assert_eq!(
        buffer[(0, 1)].symbol(),
        "│",
        "vertical border must draw over styled backdrop spaces"
    );
}

struct AdjacentFramesNoJoin;

impl Component for AdjacentFramesNoJoin {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        HStack::new()
            .gap(0)
            .child(Frame::new().child(Text::new("left")))
            .child(Frame::new().child(Text::new("right")))
            .into()
    }
}

struct AdjacentFramesJoin;

impl Component for AdjacentFramesJoin {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        HStack::new()
            .gap(0)
            .child(Frame::new().join_frame(true).child(Text::new("left")))
            .child(Frame::new().join_frame(true).child(Text::new("right")))
            .into()
    }
}

struct StackedFramesJoin;

impl Component for StackedFramesJoin {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .gap(0)
            .child(Frame::new().join_frame(true).child(Text::new("top")))
            .child(Frame::new().join_frame(true).child(Text::new("bottom")))
            .into()
    }
}

struct NestedSplitterFramesJoin;

impl Component for NestedSplitterFramesJoin {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let right = Splitter::horizontal()
            .handle_mode(SplitterHandleMode::Border)
            .child(Frame::new().join_frame(true).child(Text::new("top")))
            .child(Frame::new().join_frame(true).child(Text::new("bottom")));

        Splitter::vertical()
            .handle_mode(SplitterHandleMode::Border)
            .child(Frame::new().join_frame(true).child(Text::new("left")))
            .child(right)
            .into()
    }
}

struct FrameDividerJoin;

impl Component for FrameDividerJoin {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Frame::new()
            .border(true)
            .child(
                VStack::new()
                    .child(Text::new("top"))
                    .child(crate::widgets::Divider::horizontal().join_frame(true))
                    .child(Text::new("bottom")),
            )
            .into()
    }
}

struct DividerJunctions;

impl Component for DividerJunctions {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Canvas::new()
            .child_at(
                Rect {
                    x: 1,
                    y: 2,
                    w: 7,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 4,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            .child_at(
                Rect {
                    x: 9,
                    y: 2,
                    w: 4,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 12,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            .child_at(
                Rect {
                    x: 16,
                    y: 0,
                    w: 5,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 16,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            .child_at(
                Rect {
                    x: 20,
                    y: 2,
                    w: 5,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 22,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            .child_at(
                Rect {
                    x: 22,
                    y: 2,
                    w: 1,
                    h: 1,
                },
                Text::new("─"),
            )
            .child_at(
                Rect {
                    x: 27,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            .child_at(
                Rect {
                    x: 25,
                    y: 2,
                    w: 3,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 27,
                    y: 2,
                    w: 3,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 31,
                    y: 2,
                    w: 5,
                    h: 1,
                },
                Divider::horizontal(),
            )
            .child_at(
                Rect {
                    x: 33,
                    y: 2,
                    w: 1,
                    h: 1,
                },
                Text::new("─").style(Style::new().fg(Color::Red)),
            )
            .child_at(
                Rect {
                    x: 33,
                    y: 0,
                    w: 1,
                    h: 5,
                },
                Divider::vertical(),
            )
            // Two titled-style horizontals meeting a vertical that starts on their row must tee
            // (`┬`), not corner (`┌`) - the second horizontal used to replace the first in junction
            // state and leave only an endpoint half for Exact merge.
            .child_at(
                Rect {
                    x: 0,
                    y: 5,
                    w: 6,
                    h: 1,
                },
                Divider::horizontal()
                    .label(Text::new("L"))
                    .label_padding_axes(1, 0),
            )
            .child_at(
                Rect {
                    x: 5,
                    y: 5,
                    w: 6,
                    h: 1,
                },
                Divider::horizontal()
                    .label(Text::new("R"))
                    .label_padding_axes(2, 0),
            )
            .child_at(
                Rect {
                    x: 5,
                    y: 5,
                    w: 1,
                    h: 3,
                },
                Divider::vertical(),
            )
            .into()
    }
}

fn adjacent_frames_seam(runtime: &RuntimeCore<impl Component>) -> (u16, u16) {
    let mut frames = runtime
        .tree
        .iter()
        .filter_map(|node| matches!(&node.kind, NodeKind::Frame(_)).then_some(node.rect))
        .collect::<Vec<_>>();
    frames.sort_by_key(|rect| rect.x);
    let left = frames[0];
    let seam_x = left.x.saturating_add(left.w as i16).saturating_sub(1) as u16;
    let seam_y = left.y as u16;
    (seam_x, seam_y)
}

fn frame_rects_sorted_by_x(runtime: &RuntimeCore<impl Component>) -> Vec<Rect> {
    let mut frames = runtime
        .tree
        .iter()
        .filter_map(|node| matches!(&node.kind, NodeKind::Frame(_)).then_some(node.rect))
        .collect::<Vec<_>>();
    frames.sort_by_key(|rect| rect.x);
    frames
}

fn frame_rects_sorted_by_y(runtime: &RuntimeCore<impl Component>) -> Vec<Rect> {
    let mut frames = runtime
        .tree
        .iter()
        .filter_map(|node| matches!(&node.kind, NodeKind::Frame(_)).then_some(node.rect))
        .collect::<Vec<_>>();
    frames.sort_by_key(|rect| rect.y);
    frames
}

fn vertical_splitter_seam(runtime: &RuntimeCore<impl Component>) -> (u16, u16) {
    let seam = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::Splitter(splitter)
                if splitter.orientation == crate::widgets::Orientation::Vertical
                    && !splitter.handle_rects.is_empty() =>
            {
                Some(splitter.handle_rects[0])
            }
            _ => None,
        })
        .expect("vertical splitter seam should exist");

    (seam.x as u16, seam.y as u16)
}

fn horizontal_divider_seam(runtime: &RuntimeCore<impl Component>) -> (u16, u16, u16) {
    let frame_rect = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::Frame(_) => Some(node.rect),
            _ => None,
        })
        .expect("frame should exist");

    let divider_rect = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::Divider(divider)
                if divider.orientation == crate::widgets::Orientation::Horizontal =>
            {
                Some(node.rect)
            }
            _ => None,
        })
        .expect("horizontal divider should exist");

    let left_x = frame_rect.x as u16;
    let right_x = frame_rect
        .x
        .saturating_add(frame_rect.w as i16)
        .saturating_sub(1) as u16;
    (left_x, right_x, divider_rect.y as u16)
}

#[test]
fn adjacent_frames_without_join_keep_double_seam() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        AdjacentFramesNoJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let (seam_x, seam_y) = adjacent_frames_seam(&runtime);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(seam_x, seam_y)].symbol(), "┐");
    assert_eq!(buffer[(seam_x + 1, seam_y)].symbol(), "┌");
}

#[test]
fn adjacent_frames_with_join_collapse_seam() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        AdjacentFramesJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let (seam_x, seam_y) = adjacent_frames_seam(&runtime);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(seam_x, seam_y)].symbol(), "┬");
    assert_eq!(buffer[(seam_x + 1, seam_y)].symbol(), "─");
}

#[test]
fn adjacent_frames_with_join_do_not_add_left_content_margin() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        AdjacentFramesJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let frames = frame_rects_sorted_by_x(&runtime);
    let right = frames[1];
    let (seam_x, _) = adjacent_frames_seam(&runtime);
    let expected_x = seam_x.saturating_add(1);
    let expected_y = right.y.saturating_add(1) as u16;
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(expected_x, expected_y)].symbol(),
        "r",
        "joined right frame should not keep an extra left content margin"
    );
}

#[test]
fn stacked_frames_with_join_do_not_add_top_content_margin() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        StackedFramesJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let frames = frame_rects_sorted_by_y(&runtime);
    let top = frames[0];
    let bottom = frames[1];
    let expected_x = bottom.x.saturating_add(1) as u16;
    let seam_y = top.y.saturating_add(top.h as i16).saturating_sub(1) as u16;
    let expected_y = seam_y.saturating_add(1);
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(expected_x, expected_y)].symbol(),
        "b",
        "joined bottom frame should not keep an extra top content margin"
    );
}

#[test]
fn nested_splitter_join_merges_frame_seam() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 10,
    };
    let mut runtime = RuntimeCore::new_test(
        NestedSplitterFramesJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let (seam_x, seam_y) = vertical_splitter_seam(&runtime);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(seam_x, seam_y)].symbol(), "┬");
    assert_eq!(buffer[(seam_x + 1, seam_y)].symbol(), "─");
}

#[test]
fn divider_join_frame_uses_merged_border_intersections() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        FrameDividerJoin,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,

        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let (left_x, right_x, y) = horizontal_divider_seam(&runtime);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(left_x, y)].symbol(), "├");
    assert_eq!(buffer[(right_x, y)].symbol(), "┤");
}

#[test]
fn perpendicular_dividers_form_directional_junctions() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 38,
        h: 8,
    };
    let mut runtime = RuntimeCore::new_test(
        DividerJunctions,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let buffer = render_runtime_with_hover(&runtime, viewport, None, None);

    assert_eq!(buffer[(4, 2)].symbol(), "┼");
    assert_eq!(buffer[(12, 2)].symbol(), "┤");
    assert_eq!(buffer[(16, 0)].symbol(), "┌");
    assert_eq!(buffer[(22, 2)].symbol(), "─");
    assert_eq!(buffer[(27, 2)].symbol(), "┼");
    assert_eq!(buffer[(33, 2)].symbol(), "│");
    assert_eq!(
        buffer[(5, 5)].symbol(),
        "┬",
        "titled horizontal segments meeting a descending vertical must tee"
    );
}

#[test]
fn animated_position_offset_renders_subtree_at_visual_offset() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedPositionOffsetComponent { opacity: 1.0 },
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let animated_id = runtime
        .tree
        .iter()
        .find_map(|node| matches!(&node.kind, NodeKind::Animated(_)).then_some(node.id))
        .expect("animated node should exist");
    let final_rect = runtime.tree.node(animated_id).rect;
    match &mut runtime.tree.node_mut(animated_id).kind {
        NodeKind::Animated(animated) => {
            animated.current_x_offset = -2.0;
            animated.current_y_offset = 1.0;
        }
        _ => panic!("expected animated node"),
    }

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let visual_x = final_rect.x.saturating_sub(2) as u16;
    let visual_y = final_rect.y.saturating_add(1) as u16;
    let final_x = final_rect.x as u16;
    let final_y = final_rect.y as u16;
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(visual_x, visual_y)].symbol(), "X");
    assert_ne!(buffer[(final_x, final_y)].symbol(), "X");
}

#[test]
fn animated_position_offset_post_pass_uses_shifted_rect() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        AnimatedPositionOffsetComponent { opacity: 0.5 },
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let animated_id = runtime
        .tree
        .iter()
        .find_map(|node| matches!(&node.kind, NodeKind::Animated(_)).then_some(node.id))
        .expect("animated node should exist");
    let final_rect = runtime.tree.node(animated_id).rect;
    match &mut runtime.tree.node_mut(animated_id).kind {
        NodeKind::Animated(animated) => {
            animated.current_x_offset = -2.0;
            animated.current_y_offset = 1.0;
        }
        _ => panic!("expected animated node"),
    }

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let terminal_bg = ratatui::style::Color::Rgb(40, 42, 44);
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Wcag,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(terminal_bg),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    let visual_x = final_rect.x.saturating_sub(2) as u16;
    let visual_y = final_rect.y.saturating_add(1) as u16;
    let buffer = terminal.backend().buffer();
    let faded = &buffer[(visual_x, visual_y)];
    assert_eq!(faded.symbol(), "X");
    assert_ne!(faded.fg, ratatui::style::Color::White);
    assert_ne!(faded.bg, ratatui::style::Color::Black);
}

#[test]
fn translucent_overlay_surface_blends_with_the_cells_it_covers() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ToastSurfaceBandsComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    // Half-strength black over rows that are each a different colour.
    runtime.overlay_manager.borrow_mut().push_toast(
        Toast::new("T")
            .border(false)
            .width(Length::Px(3))
            .height(Length::Px(3))
            .frame_style(Style::new().bg_alpha(Color::rgb(0, 0, 0), 0.5)),
    );
    runtime.render_element(viewport, None, None, None);
    // A freshly pushed toast starts its fade at zero opacity, which restores the underlay
    // wholesale; settle it so this measures the surface blend rather than the transition.
    let mut overlays = runtime.tree.overlay_roots().to_vec();
    let toast_rect = runtime.tree.node(overlays[0].id).rect;
    overlays[0].opacity = 1.0;
    runtime.tree.set_overlay_roots(overlays);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(ratatui::style::Color::Rgb(0, 0, 0)),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };
    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    // Every covered cell must be its own underlying colour at half strength. Blending against the
    // cleared region instead produced one flat colour for the whole surface, discarding exactly the
    // variation that makes a translucent panel worth having.
    let buffer = terminal.backend().buffer();
    let x = toast_rect.x as u16;
    let mut seen = Vec::new();
    for row in 0..toast_rect.h {
        let y = toast_rect.y as u16 + row;
        let under = buffer[(0, y)].bg;
        let over = buffer[(x, y)].bg;
        let ratatui::style::Color::Rgb(ur, ug, ub) = under else {
            panic!("underlay row {y} should be a concrete colour, got {under:?}");
        };
        assert_eq!(
            over,
            ratatui::style::Color::Rgb(ur / 2, ug / 2, ub / 2),
            "row {y} must keep its own colour at half strength",
        );
        seen.push(over);
    }
    seen.dedup();
    assert!(
        seen.len() > 1,
        "the surface spans rows of different colours, so it cannot render as one flat colour",
    );
}

#[test]
fn translucent_toast_surface_is_uniform_behind_its_text() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 9,
        h: 5,
    };
    let mut runtime = RuntimeCore::new_test(
        ToastTransitionUnderlayComponent,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.overlay_manager.borrow_mut().push_toast(
        // Mirrors an application toast: bordered, wrapped, with its own message foreground.
        Toast::new("ab")
            .border(true)
            .wrap(true)
            .max_width(Length::Px(64))
            .message_style(Style::new().fg(Color::rgb(226, 232, 240)))
            .padding((0, 0, 0, 0))
            .frame_style(Style::new().bg_alpha(Color::rgb(200, 40, 40), 0.5)),
    );
    runtime.render_element(viewport, None, None, None);
    let mut overlays = runtime.tree.overlay_roots().to_vec();
    let toast_rect = runtime.tree.node(overlays[0].id).rect;
    overlays[0].opacity = 1.0;
    runtime.tree.set_overlay_roots(overlays);

    let backend = TestBackend::new(viewport.w, viewport.h);
    let mut terminal = Terminal::new(backend).expect("terminal should init");
    let ctx = RenderContext {
        tree: &runtime.tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: true,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &RefCell::new(Default::default()),
        overlay_bg_snapshot: &RefCell::new(Vec::new()),
        join_index: &build_join_index(&runtime.tree),
        cursor_position: &Cell::new(None),
        terminal_bg: Some(ratatui::style::Color::Rgb(0, 0, 0)),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };
    terminal
        .draw(|f| render(f, &ctx))
        .expect("render should succeed");

    // Border and text sit on one surface and must read as one colour. Copying the frame's alpha
    // paint onto the message style made text cells composite that alpha a second time, leaving a
    // darker patch behind the words.
    let buffer = terminal.backend().buffer();
    let mut sampled = Vec::new();
    for dy in 0..toast_rect.h {
        for dx in 0..toast_rect.w {
            sampled.push((
                dx,
                dy,
                buffer[(toast_rect.x as u16 + dx, toast_rect.y as u16 + dy)].bg,
            ));
        }
    }
    let first = sampled[0].2;
    assert!(
        sampled.iter().all(|(_, _, bg)| *bg == first),
        "the surface must be one colour across the toast, got {sampled:?}",
    );
}
