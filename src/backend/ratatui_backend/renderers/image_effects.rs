//! Cell passes inside one render layer that recolor images drawn beneath them.
//!
//! A root overlay's backdrop is known before anything draws (see
//! [`super::image::set_image_backdrops`]). The passes here live in the tree instead: an
//! `EffectScope`, an `Animated` opacity toward a color, and a `Canvas` or `Center` whose style dims
//! or tints the earlier siblings it is drawn over, such as a `Local` modal's backdrop. The renderer lists them before a
//! layer draws, in the order they will apply, and drops each one once it has applied, so an image
//! drawn later is not put through it.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use ratatui::style::Color as RColor;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, apply_visual_effects_over_backdrop, from_ratatui_color,
};
use crate::backend::ratatui_backend::renderers::image::ImageBackdrop;
use crate::core::node::NodeId;
use crate::style::{ColorTransform, Rect, Style, VisualEffect};

thread_local! {
    /// Whether the layer drawing now holds anything that draws an image.
    static LAYER_DRAWS_IMAGES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Cells of the layer drawing now that a `Local` modal will cover once its turn comes, by the
    /// node that draws the modal.
    static PENDING_IMAGE_OCCLUSIONS: RefCell<Vec<(NodeId, ratatui::layout::Rect)>> =
        const { RefCell::new(Vec::new()) };
    /// Passes of the layer drawing now that have yet to apply, in the order they will.
    static PENDING_IMAGE_EFFECTS: RefCell<Vec<PendingImageEffect>> = const { RefCell::new(Vec::new()) };
    /// One cell that passes are replayed on to recolor a pixel.
    static REPLAY_TERMINAL: RefCell<Option<ratatui::Terminal<ratatui::backend::TestBackend>>> =
        const { RefCell::new(None) };
}

/// A pass the node `node` will run over `backdrop.rect` once its turn in the layer comes.
#[derive(Clone, Debug)]
pub(crate) struct PendingImageEffect {
    pub(crate) node: NodeId,
    pub(crate) backdrop: ImageBackdrop,
}

/// What the renderer found in a layer before drawing it. See [`set_pending_image_effects`].
#[derive(Default)]
pub(crate) struct PendingImageLayer {
    pub(crate) effects: Vec<PendingImageEffect>,
    pub(crate) occlusions: Vec<(NodeId, ratatui::layout::Rect)>,
    pub(crate) draws_images: bool,
}

/// Install what the layer about to draw does over the images in it.
pub(crate) fn set_pending_image_effects(layer: PendingImageLayer) {
    PENDING_IMAGE_EFFECTS.with(|slot| *slot.borrow_mut() = layer.effects);
    PENDING_IMAGE_OCCLUSIONS.with(|slot| *slot.borrow_mut() = layer.occlusions);
    LAYER_DRAWS_IMAGES.with(|slot| slot.set(layer.draws_images));
}

/// Drop whatever passes are left, at the end of a layer.
pub(crate) fn clear_pending_image_effects() {
    PENDING_IMAGE_EFFECTS.with(|slot| slot.borrow_mut().clear());
    PENDING_IMAGE_OCCLUSIONS.with(|slot| slot.borrow_mut().clear());
    LAYER_DRAWS_IMAGES.with(|slot| slot.set(false));
}

/// Rects that `Local` modals still to draw in this layer will cover.
///
/// A Kitty placeholder row is one escape in its first cell that walks the whole row, so a modal
/// painted over the row later does not cut it on the host: the row has to leave the modal out, as
/// it leaves out a root overlay.
#[cfg(feature = "terminal-images")]
pub(crate) fn pending_image_occlusions() -> Vec<ratatui::layout::Rect> {
    PENDING_IMAGE_OCCLUSIONS.with(|slot| slot.borrow().iter().map(|&(_, rect)| rect).collect())
}

/// Whether the layer drawing now draws images, so its passes may run over Kitty placeholders.
pub(crate) fn layer_draws_images() -> bool {
    LAYER_DRAWS_IMAGES.with(std::cell::Cell::get)
}

/// `node` has run its passes, or will not run them this frame; images drawn from here on are not
/// under them.
pub(crate) fn image_effects_applied(node: NodeId) {
    PENDING_IMAGE_EFFECTS.with(|slot| {
        let mut pending = slot.borrow_mut();
        if !pending.is_empty() {
            pending.retain(|effect| effect.node != node);
        }
    });
    PENDING_IMAGE_OCCLUSIONS.with(|slot| {
        let mut pending = slot.borrow_mut();
        if !pending.is_empty() {
            pending.retain(|&(owner, _)| owner != node);
        }
    });
}

/// Visit the passes still to apply, in order.
pub(crate) fn for_each_pending_image_effect(mut visit: impl FnMut(&ImageBackdrop)) {
    PENDING_IMAGE_EFFECTS.with(|slot| {
        for effect in slot.borrow().iter() {
            visit(&effect.backdrop);
        }
    });
}

/// What a node does to the cells beneath it.
#[derive(Clone, Debug, Hash)]
pub(crate) enum CellPass {
    /// A style's effects over what is drawn: an `Animated` opacity, or a surface that recolors
    /// without painting a background of its own.
    Effect(Style),
    /// One `EffectScope` effect, with any clip taken out and kept as the layer's rect instead.
    Visual(VisualEffect),
}

/// A [`CellPass`] as a recolor of image pixels. The pass itself runs on a one-cell buffer holding
/// the pixel's color as its background, so a pixel ends up exactly as a cell of that color does.
#[derive(Clone, Debug)]
pub(crate) struct ReplayedEffect(Arc<Replayed>);

#[derive(Debug)]
struct Replayed {
    pass: CellPass,
    terminal_bg: Option<RColor>,
    per_channel: bool,
    key: u64,
}

impl ReplayedEffect {
    /// The pass as a pixel recolor, or `None` when it leaves backgrounds alone, or covers them
    /// with one color and so hides the image rather than recoloring it.
    pub(crate) fn new(pass: CellPass, terminal_bg: Option<RColor>) -> Option<Self> {
        let per_channel = match &pass {
            CellPass::Effect(style) => {
                !matches!(style.bg_transform, Some(ColorTransform::Elevate(_)))
            }
            CellPass::Visual(effect) => visual_effect_is_per_channel(effect),
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        pass.hash(&mut hasher);
        terminal_bg.hash(&mut hasher);
        let effect = Self(Arc::new(Replayed {
            pass,
            terminal_bg,
            per_channel,
            key: hasher.finish(),
        }));

        const PROBES: [(u8, u8, u8); 4] =
            [(0, 0, 0), (255, 255, 255), (128, 128, 128), (200, 40, 90)];
        let recolored = PROBES.map(|rgb| effect.apply_rgb(rgb));
        let unchanged = recolored == PROBES;
        let covers = recolored.iter().all(|&rgb| rgb == recolored[0]);
        (!unchanged && !covers).then_some(effect)
    }

    /// Whether each output channel depends only on the same input channel. See
    /// [`crate::backend::ratatui_backend::common::BackdropBackgroundEffect::is_per_channel`].
    pub(crate) fn is_per_channel(&self) -> bool {
        self.0.per_channel
    }

    /// The color a cell background of `rgb` ends up after the pass.
    pub(crate) fn apply_rgb(&self, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        let terminal_bg = self.0.terminal_bg;
        REPLAY_TERMINAL.with(|slot| {
            let mut slot = slot.borrow_mut();
            let terminal = slot.get_or_insert_with(|| {
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1))
                    .expect("a test backend cannot fail")
            });
            let mut frame = terminal.get_frame();
            let cell = &mut frame.buffer_mut()[(0, 0)];
            cell.reset();
            cell.bg = RColor::Rgb(rgb.0, rgb.1, rgb.2);
            let rect = Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            };
            match &self.0.pass {
                CellPass::Effect(style) => {
                    apply_effect_style_clipped(&mut frame, rect, *style, None, terminal_bg);
                }
                CellPass::Visual(effect) => apply_visual_effects_over_backdrop(
                    &mut frame,
                    rect,
                    std::slice::from_ref(effect),
                    0,
                    None,
                    terminal_bg,
                    None,
                ),
            }
            let bg = match frame.buffer_mut()[(0, 0)].bg {
                RColor::Reset => terminal_bg.unwrap_or(RColor::Rgb(rgb.0, rgb.1, rgb.2)),
                bg => bg,
            };
            from_ratatui_color(bg).to_rgb().unwrap_or(rgb)
        })
    }
}

impl PartialEq for ReplayedEffect {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key
    }
}

impl Eq for ReplayedEffect {}

impl Hash for ReplayedEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.key.hash(state);
    }
}

fn visual_effect_is_per_channel(effect: &VisualEffect) -> bool {
    match effect {
        VisualEffect::Channels { inner, .. } | VisualEffect::Clipped { inner, .. } => {
            visual_effect_is_per_channel(inner)
        }
        VisualEffect::ColorTransform { bg, .. } => !matches!(bg, Some(ColorTransform::Elevate(_))),
        _ => false,
    }
}

/// `effect` with its rectangular clips taken out, and the scope-local bounds they left, or `None`
/// when the effect cannot be put through pixels: it depends on where a cell is, changes over time,
/// reads more than the cell's own color, or clips to a per-cell mask.
pub(crate) fn pixel_visual_effect(effect: &VisualEffect) -> Option<(VisualEffect, Option<Rect>)> {
    match effect {
        VisualEffect::Clipped {
            bounds,
            mask: None,
            inner,
        } => {
            let (inner, inner_bounds) = pixel_visual_effect(inner)?;
            let bounds = match (*bounds, inner_bounds) {
                (Some(outer), Some(inner)) => Some(outer.intersection(&inner)),
                (outer, inner) => outer.or(inner),
            };
            Some((inner, bounds))
        }
        VisualEffect::Channels { channels, inner } => {
            let (inner, bounds) = pixel_visual_effect(inner)?;
            Some((
                VisualEffect::Channels {
                    channels: *channels,
                    inner: Box::new(inner),
                },
                bounds,
            ))
        }
        VisualEffect::ColorTransform { .. }
        | VisualEffect::Monochrome { .. }
        | VisualEffect::PaletteQuantize { .. } => Some((effect.clone(), None)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ratatui_backend::common::BackdropBackgroundEffect;
    use crate::style::{Color, EffectChannels};

    #[test]
    fn a_replayed_style_recolors_as_the_backdrop_arithmetic_does() {
        let terminal_bg = Some(RColor::Rgb(12, 14, 20));
        for style in [
            Style::new().dim_by(0.5),
            Style::new().tint_by(Color::Rgb(0, 0, 40), 0.5),
            Style::new().transform_bg(ColorTransform::Elevate(0.5)),
        ] {
            let replayed = ReplayedEffect::new(CellPass::Effect(style), terminal_bg)
                .expect("the style recolors backgrounds");
            let direct = BackdropBackgroundEffect::from_style(style, terminal_bg).unwrap();
            assert_eq!(replayed.is_per_channel(), direct.is_per_channel());
            for rgb in [(0, 0, 0), (255, 0, 0), (17, 200, 99), (255, 255, 255)] {
                assert_eq!(
                    replayed.apply_rgb(rgb),
                    direct.apply_rgb(rgb),
                    "{style:?} {rgb:?}"
                );
            }
        }
    }

    #[test]
    fn passes_that_do_not_recolor_backgrounds_are_not_kept() {
        let foreground = VisualEffect::dim(0.5).foreground_only();
        assert!(ReplayedEffect::new(CellPass::Visual(foreground), None).is_none());
        assert!(ReplayedEffect::new(CellPass::Effect(Style::new().bold()), None).is_none());
        let cover = Style::new().transform_bg(ColorTransform::OpacityToward {
            factor: 0.0,
            target: Color::Rgb(1, 2, 3),
        });
        assert!(ReplayedEffect::new(CellPass::Effect(cover), None).is_none());
    }

    #[test]
    fn only_effects_of_a_cells_own_color_reach_pixels() {
        assert!(pixel_visual_effect(&VisualEffect::Monochrome { strength: 1.0 }).is_some());
        assert!(pixel_visual_effect(&VisualEffect::dim(0.5).background_only()).is_some());
        assert!(
            pixel_visual_effect(&VisualEffect::Scanlines {
                strength: 0.5,
                spacing: 2,
            })
            .is_none()
        );
        let masked = VisualEffect::Clipped {
            bounds: None,
            mask: Some(Arc::new(crate::core::mask::CellMask {
                origin: (0, 0),
                w: 1,
                h: 1,
                bits: Arc::from([1u64]),
            })),
            inner: Box::new(VisualEffect::dim(0.5)),
        };
        assert!(pixel_visual_effect(&masked).is_none());
    }

    #[test]
    fn a_rectangular_clip_becomes_the_layer_bounds() {
        let bounds = Rect {
            x: 1,
            y: 2,
            w: 3,
            h: 4,
        };
        let clipped = VisualEffect::Channels {
            channels: EffectChannels::Background,
            inner: Box::new(VisualEffect::Clipped {
                bounds: Some(bounds),
                mask: None,
                inner: Box::new(VisualEffect::dim(0.5)),
            }),
        };
        let (effect, found) = pixel_visual_effect(&clipped).expect("a rect clip is kept");
        assert_eq!(found, Some(bounds));
        assert!(matches!(effect, VisualEffect::Channels { .. }));
        let replayed = ReplayedEffect::new(CellPass::Visual(effect), None).unwrap();
        assert_eq!(replayed.apply_rgb((200, 100, 50)), (100, 50, 25));
    }
}
