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
use crate::style::{
    BorderEdges, Color, ColorTransform, Edge, EffectAxis, EffectPalette, Length, Paint, Rect,
    ScrollbarConfig, ScrollbarVariant, Style, Theme, VisualEffect,
};
use crate::utils::color_contrast::contrast_ratio;
use crate::widgets::{
    Animated, BorderMergeMode, Button, DecorationGlyph, DecorationPlacement, EdgeDecoration,
    EffectScope, Frame, HStack, List, ListItem, Modal, Spacer, Splitter, SplitterHandleMode, Text,
    VStack, ZStack,
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

struct TransparentModalOverlayComponent;

struct ExplicitOverlayBackgroundPaintComponent;

struct ExplicitOverlayForegroundOnlySpacesComponent;

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
            .header(Text::new("Search"))
            .child(Text::new("Body"))
            .into()
    }
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
            .title("Compact")
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
            .title("Files")
            .status_right("1 of 6")
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
fn compact_frame_with_status_right_fills_the_visible_row() {
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
