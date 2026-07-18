use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::app::ContrastPolicy;

use super::{Color, HostTerminalColors, Paint};

/// A relative transform applied to an already-resolved color.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug)]
pub enum ColorTransform {
    /// Dim toward black by an amount in `[0.0, 1.0]`.
    Dim(f32),
    /// Lighten toward white by an amount in `[0.0, 1.0]`.
    Lighten(f32),
    /// Blend toward the resolved background by alpha `(1.0 - opacity)`.
    ///
    /// `1.0` keeps the original color unchanged, while `0.0` fully washes it
    /// into the current background. This is most useful for foreground colors
    /// on both dark and light themes.
    Opacity(f32),
    /// Like [`Self::Opacity`], but blend toward a fixed `target` instead of the cell backdrop.
    OpacityToward {
        /// Same semantics as [`Self::Opacity`]: `1.0` is unchanged, `0.0` is fully `target`.
        factor: f32,
        /// Destination color when `factor` approaches `0.0`.
        target: Color,
    },
    /// Blend toward `color` by `alpha` in `[0.0, 1.0]`.
    Tint(Color, f32),
}

impl PartialEq for ColorTransform {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Dim(a), Self::Dim(b))
            | (Self::Lighten(a), Self::Lighten(b))
            | (Self::Opacity(a), Self::Opacity(b)) => a.to_bits() == b.to_bits(),
            (
                Self::OpacityToward {
                    factor: fa,
                    target: ta,
                },
                Self::OpacityToward {
                    factor: fb,
                    target: tb,
                },
            ) => fa.to_bits() == fb.to_bits() && ta == tb,
            (Self::Tint(color_a, alpha_a), Self::Tint(color_b, alpha_b)) => {
                color_a == color_b && alpha_a.to_bits() == alpha_b.to_bits()
            }
            _ => false,
        }
    }
}

impl Eq for ColorTransform {}

impl Hash for ColorTransform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            Self::Dim(amount) => {
                0u8.hash(state);
                amount.to_bits().hash(state);
            }
            Self::Lighten(amount) => {
                1u8.hash(state);
                amount.to_bits().hash(state);
            }
            Self::Opacity(amount) => {
                2u8.hash(state);
                amount.to_bits().hash(state);
            }
            Self::OpacityToward { factor, target } => {
                4u8.hash(state);
                factor.to_bits().hash(state);
                target.hash(state);
            }
            Self::Tint(color, alpha) => {
                3u8.hash(state);
                color.hash(state);
                alpha.to_bits().hash(state);
            }
        }
    }
}

impl ColorTransform {
    /// Apply this transform to `color`.
    pub fn apply(self, color: Color) -> Color {
        self.apply_with_backdrop(color, None)
    }

    /// Apply this transform to `color`, optionally using a resolved backdrop.
    pub fn apply_with_backdrop(self, color: Color, backdrop: Option<Color>) -> Color {
        if matches!(color, Color::Transparent | Color::Backdrop) {
            return color;
        }
        match self {
            Self::Dim(amount) => color.dim_by(amount),
            Self::Lighten(amount) => color.lighten_by(amount),
            Self::Opacity(opacity) => backdrop.map_or(color, |bg| {
                color.blend_toward(bg, (1.0 - opacity).clamp(0.0, 1.0))
            }),
            Self::OpacityToward { factor, target } => {
                color.blend_toward(target, (1.0 - factor).clamp(0.0, 1.0))
            }
            Self::Tint(target, alpha) => color.blend_toward(target, alpha),
        }
    }

    /// Apply this transform to `paint`.
    ///
    /// Pigment transforms preserve the paint alpha; [`Self::Opacity`] composes
    /// with the existing alpha by multiplying it by the opacity factor.
    pub fn apply_paint(self, paint: Paint) -> Paint {
        self.apply_paint_with_backdrop(paint, None)
    }

    /// Apply this transform to `paint`, optionally using a resolved backdrop paint.
    pub fn apply_paint_with_backdrop(self, paint: Paint, backdrop: Option<Paint>) -> Paint {
        if matches!(paint, Paint::Solid(Color::Transparent | Color::Backdrop)) {
            return paint;
        }
        if let Self::Opacity(opacity) = self {
            let alpha = (paint.alpha_u8() as f32 * opacity.clamp(0.0, 1.0))
                .round()
                .clamp(0.0, 255.0) as u8;
            return Paint::from_color_alpha_u8(paint.color(), alpha);
        }
        let backdrop = backdrop.map(Paint::color);
        match paint {
            Paint::Solid(color) => Paint::Solid(self.apply_with_backdrop(color, backdrop)),
            Paint::Alpha { color, alpha } => {
                Paint::from_color_alpha_u8(self.apply_with_backdrop(color, backdrop), alpha)
            }
        }
    }

    pub(crate) fn needs_backdrop(self) -> bool {
        matches!(self, Self::Opacity(_))
    }

    fn normalized(self) -> Self {
        match self {
            Self::Dim(amount) => Self::Dim(amount.clamp(0.0, 1.0)),
            Self::Lighten(amount) => Self::Lighten(amount.clamp(0.0, 1.0)),
            Self::Opacity(opacity) => Self::Opacity(opacity.clamp(0.0, 1.0)),
            Self::OpacityToward { factor, target } => Self::OpacityToward {
                factor: factor.clamp(0.0, 1.0),
                target,
            },
            Self::Tint(color, alpha) => Self::Tint(color, alpha.clamp(0.0, 1.0)),
        }
    }
}

/// Marker trait for typed app-specific theme data stored inside [`Theme`].
///
/// Use this when your app needs semantic theme tokens that are not part of the
/// core framework palettes, while still keeping those tokens inside the active
/// framework theme rather than a parallel global palette.
pub trait ThemeExtension: Clone + fmt::Debug + PartialEq + 'static {}

impl<T> ThemeExtension for T where T: Clone + fmt::Debug + PartialEq + 'static {}

trait ThemeExtensionValue: Any {
    fn as_any(&self) -> &dyn Any;
    fn eq_value(&self, other: &dyn ThemeExtensionValue) -> bool;
}

impl<T> ThemeExtensionValue for T
where
    T: ThemeExtension,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn eq_value(&self, other: &dyn ThemeExtensionValue) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }
}

#[derive(Clone, Default)]
#[doc(hidden)]
pub struct ThemeExtensions(HashMap<TypeId, Arc<dyn ThemeExtensionValue>>);

impl ThemeExtensions {
    fn insert<T>(&mut self, extension: T)
    where
        T: ThemeExtension,
    {
        self.0.insert(TypeId::of::<T>(), Arc::new(extension));
    }

    fn get<T>(&self) -> Option<&T>
    where
        T: ThemeExtension,
    {
        self.0
            .get(&TypeId::of::<T>())
            .and_then(|value| value.as_any().downcast_ref::<T>())
    }

    fn remove<T>(&mut self)
    where
        T: ThemeExtension,
    {
        self.0.remove(&TypeId::of::<T>());
    }
}

impl PartialEq for ThemeExtensions {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|(type_id, value)| {
                other
                    .0
                    .get(type_id)
                    .is_some_and(|other_value| value.eq_value(other_value.as_ref()))
            })
    }
}

impl Eq for ThemeExtensions {}

impl fmt::Debug for ThemeExtensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThemeExtensions")
            .field("count", &self.0.len())
            .finish()
    }
}

/// Styling information (kept backend-agnostic).
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    /// Foreground color.
    pub fg: Option<Paint>,
    /// Background color.
    pub bg: Option<Paint>,
    /// Relative transform applied to the resolved foreground color.
    pub fg_transform: Option<ColorTransform>,
    /// Relative transform applied to the resolved background color.
    pub bg_transform: Option<ColorTransform>,
    /// Contrast override applied after color transforms resolve.
    pub contrast_policy: Option<ContrastPolicy>,
    /// Bold modifier.
    pub bold: Option<bool>,
    /// Dim modifier.
    pub dim: Option<bool>,
    /// Italic modifier.
    pub italic: Option<bool>,
    /// Underline modifier.
    pub underline: Option<bool>,
    /// Reverse video modifier.
    pub reverse: Option<bool>,
    /// Strikethrough modifier.
    pub strikethrough: Option<bool>,
    /// Underline color (requires underline to be enabled).
    pub underline_color: Option<Paint>,
    /// Cell-level dim amount in `[0.0, 1.0]`.
    ///
    /// When set, the renderer scales the existing rendered colors of every cell
    /// in the area by `(1.0 - dim_amount)` before drawing this style on top.
    /// This makes `dim_by` work even when no explicit fg/bg colors are set,
    /// which is the typical backdrop use-case.
    pub dim_amount: Option<f32>,
    /// Tint color and blend alpha in `[0.0, 1.0]`.
    ///
    /// When set, the renderer blends every existing cell color in the area
    /// toward this color by the given alpha before drawing this style on top.
    /// `Color::Reset` cell backgrounds are treated as black `(0, 0, 0)` for
    /// blending, so the tint is visible even on transparent-background
    /// terminals.
    pub tint: Option<(Color, f32)>,
}

impl PartialEq for Style {
    fn eq(&self, other: &Self) -> bool {
        self.fg == other.fg
            && self.bg == other.bg
            && self.fg_transform == other.fg_transform
            && self.bg_transform == other.bg_transform
            && self.contrast_policy == other.contrast_policy
            && self.bold == other.bold
            && self.dim == other.dim
            && self.italic == other.italic
            && self.underline == other.underline
            && self.reverse == other.reverse
            && self.strikethrough == other.strikethrough
            && self.underline_color == other.underline_color
            && self.dim_amount.map(f32::to_bits) == other.dim_amount.map(f32::to_bits)
            && self.tint.map(|(c, a)| (c, a.to_bits())) == other.tint.map(|(c, a)| (c, a.to_bits()))
    }
}

impl Eq for Style {}

#[cfg(all(test, feature = "terminal-serde"))]
mod terminal_serde_tests {
    use super::*;

    #[test]
    fn color_transform_round_trips() {
        let transform = ColorTransform::OpacityToward {
            factor: 0.42,
            target: Color::rgb(1, 2, 3),
        };
        let json = serde_json::to_string(&transform).unwrap();
        assert_eq!(
            serde_json::from_str::<ColorTransform>(&json).unwrap(),
            transform
        );
    }

    #[test]
    fn style_round_trips() {
        let style = Style::default()
            .fg(Paint::rgb(20, 30, 40))
            .bg(Paint::rgba(1, 2, 3, 180))
            .bold()
            .underline()
            .contrast_policy(ContrastPolicy::BlackOrWhite)
            .tint_by(Color::Cyan, 0.25);
        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(serde_json::from_str::<Style>(&json).unwrap(), style);
    }
}

/// Describes how a widget-owned style slot consumes the active theme.
///
/// `Style` itself remains a partial overlay where `None` means “fall through to
/// the layer below”. `StyleSlot` adds the missing slot-level intent for themed
/// state styles such as selection, hover, focus, and active rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StyleSlot {
    /// Use the active theme role verbatim.
    #[default]
    Inherit,
    /// Patch this style on top of the active theme role.
    Extend(Style),
    /// Use this style as the complete slot overlay; ignore the theme role.
    Replace(Style),
}

impl StyleSlot {
    /// Create a replacement slot from a style.
    pub fn replace(style: Style) -> Self {
        Self::Replace(style)
    }

    /// Create an extending slot from a style.
    pub fn extend(style: Style) -> Self {
        Self::Extend(style)
    }

    /// Return the explicit style when this slot carries one.
    pub fn explicit_style(self) -> Option<Style> {
        match self {
            Self::Inherit => None,
            Self::Extend(style) | Self::Replace(style) => Some(style),
        }
    }

    /// Whether this slot carries a non-empty explicit style.
    pub fn has_explicit_style(self) -> bool {
        self.explicit_style().is_some_and(|style| !style.is_empty())
    }

    /// Whether this slot is guaranteed to resolve to an empty overlay without theme lookup.
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Replace(style) if style.is_empty())
    }
}

impl From<Style> for StyleSlot {
    fn from(style: Style) -> Self {
        Self::Replace(style)
    }
}

/// Semantic style roles exposed by [`Theme`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeRole {
    /// Default widget text/surface style.
    Base,
    /// Interactive accent/emphasis style.
    Accent,
    /// Selected/current item style.
    Selection,
    /// Text/range selection style.
    TextSelection,
    /// Selection style for unfocused widgets. Falls back to [`Selection`](Self::Selection).
    UnfocusedSelection,
    /// Hover state style.
    Hover,
    /// Drag-source active style used while a source is being dragged.
    ///
    /// Currently resolves to the same palette as [`Hover`](Self::Hover) so existing
    /// drag feedback stays unchanged, but it is semantically separate from pointer
    /// hover and may receive a dedicated palette in a future theme revision.
    DragSource,
    /// Drop-zone affordance style for inactive but available drop targets.
    ///
    /// This role is intended for future always-visible or pre-active drop-zone
    /// affordances. It currently resolves like [`Hover`](Self::Hover) and may be
    /// unused by widgets until inactive drop-zone styling is introduced.
    DropTarget,
    /// Active drop-target style used while a compatible drag is over the target.
    ///
    /// Currently resolves to the same palette as [`Hover`](Self::Hover) so existing
    /// drop highlight feedback stays unchanged, but it is semantically separate
    /// from genuine pointer hover.
    DropTargetActive,
    /// Focus state style.
    Focus,
    /// Active state style. Currently resolves to the selection role by default.
    Active,
    /// Per-item hover state style.
    ItemHover,
    /// Border/frame style.
    Border,
    /// Disabled or muted secondary-content style.
    Disabled,
    /// Muted secondary-content style.
    Muted,
    /// Error/status style.
    Error,
    /// Focused input content style.
    InputFocusContent,
    /// Focused text-area content style.
    TextAreaFocusContent,
    /// Focused document-view content style.
    DocumentViewFocusContent,
    /// Focused hex-area content style.
    HexAreaFocusContent,
    /// Hex-area cursor style.
    HexAreaCursor,
    /// Focused terminal content style.
    TerminalFocusContent,
    /// Scrollbar thumb style.
    ScrollbarThumb,
    /// Focused scrollbar thumb style.
    ScrollbarThumbFocus,
    /// Scrollbar track style.
    ScrollbarTrack,
    /// Splitter hover handle style.
    SplitterHover,
    /// Splitter active handle style.
    SplitterActive,
}

impl Hash for Style {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fg.hash(state);
        self.bg.hash(state);
        self.fg_transform.hash(state);
        self.bg_transform.hash(state);
        self.contrast_policy.hash(state);
        self.bold.hash(state);
        self.dim.hash(state);
        self.italic.hash(state);
        self.underline.hash(state);
        self.reverse.hash(state);
        self.strikethrough.hash(state);
        self.underline_color.hash(state);
        self.dim_amount.map(f32::to_bits).hash(state);
        if let Some((c, a)) = self.tint {
            c.hash(state);
            a.to_bits().hash(state);
        }
    }
}

impl Style {
    /// Create a new, empty style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set foreground color.
    pub fn fg(mut self, color: impl Into<Paint>) -> Self {
        self.fg = Some(color.into());
        self
    }

    /// Set background color.
    pub fn bg(mut self, color: impl Into<Paint>) -> Self {
        self.bg = Some(color.into());
        self
    }

    /// Set foreground color with alpha in `[0.0, 1.0]`.
    pub fn fg_alpha(mut self, color: Color, alpha: f32) -> Self {
        self.fg = Some(Paint::from_color_alpha(color, alpha));
        self
    }

    /// Set background color with alpha in `[0.0, 1.0]`.
    pub fn bg_alpha(mut self, color: Color, alpha: f32) -> Self {
        self.bg = Some(Paint::from_color_alpha(color, alpha));
        self
    }

    /// Apply a relative transform to the resolved foreground color.
    pub fn transform_fg(mut self, transform: ColorTransform) -> Self {
        self.fg_transform = Some(transform.normalized());
        self
    }

    /// Apply a relative transform to the resolved background color.
    pub fn transform_bg(mut self, transform: ColorTransform) -> Self {
        self.bg_transform = Some(transform.normalized());
        self
    }

    /// Override contrast adjustment for this style only.
    pub fn contrast_policy(mut self, policy: ContrastPolicy) -> Self {
        self.contrast_policy = Some(policy);
        self
    }

    /// Enable bold.
    pub fn bold(mut self) -> Self {
        self.bold = Some(true);
        self
    }

    /// Explicitly disable bold.
    ///
    /// Sets `bold` to `Some(false)`, which prevents renderer-level bold
    /// fallbacks from triggering and removes bold when patched onto a style
    /// that already has it.
    pub fn not_bold(mut self) -> Self {
        self.bold = Some(false);
        self
    }

    /// Enable dim.
    pub fn dim(mut self) -> Self {
        self.dim = Some(true);
        self
    }

    /// Dim by an explicit amount in `[0.0, 1.0]`.
    ///
    /// - For explicit `fg`/`bg` colors the channels are scaled in color-space.
    /// - Additionally, `dim_amount` is stored so the renderer can scale the
    ///   existing rendered colors of every cell in the area (e.g. a backdrop)
    ///   even when no explicit colors are set on this style.
    pub fn dim_by(mut self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        self.fg_transform = Some(ColorTransform::Dim(amount));
        self.bg_transform = Some(ColorTransform::Dim(amount));
        self.dim_amount = Some(amount);
        self
    }

    /// Tint backdrop by blending existing rendered cell colors toward `color`
    /// by `alpha` in `[0.0, 1.0]`.
    ///
    /// - `0.0` leaves cells unchanged.
    /// - `1.0` replaces all cell colors with `color`.
    ///
    /// Unlike `dim_by`, this blends toward a specific color rather than black,
    /// making it visible even on terminals that use `Color::Reset` as their
    /// background (Reset is treated as black for blending).
    ///
    /// This sets the compositor hook only and does not modify any explicit
    /// `fg`/`bg` colors on this style.
    pub fn tint_by(mut self, color: Color, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        self.tint = Some((color, alpha));
        self
    }

    /// Lighten explicit `fg`/`bg` colors by an amount in `[0.0, 1.0]`.
    ///
    /// - `0.0` keeps explicit colors unchanged.
    /// - `1.0` moves explicit colors to white.
    ///
    /// Unlike `dim_by`, this only affects colors explicitly set on this style.
    pub fn lighten_by(mut self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        self.fg_transform = Some(ColorTransform::Lighten(amount));
        self.bg_transform = Some(ColorTransform::Lighten(amount));
        self
    }

    /// Enable italic.
    pub fn italic(mut self) -> Self {
        self.italic = Some(true);
        self
    }

    /// Enable underline.
    pub fn underline(mut self) -> Self {
        self.underline = Some(true);
        self
    }

    /// Enable reverse video.
    pub fn reverse(mut self) -> Self {
        self.reverse = Some(true);
        self
    }

    /// Enable strikethrough.
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = Some(true);
        self
    }

    /// Set underline color. Also enables underline automatically.
    pub fn underline_color(mut self, color: impl Into<Paint>) -> Self {
        self.underline_color = Some(color.into());
        self.underline = Some(true);
        self
    }

    /// Returns `true` if this style has no colors or modifiers set.
    ///
    /// This is used to check whether a hover/focus style would have any effect.
    pub fn is_empty(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && self.fg_transform.is_none()
            && self.bg_transform.is_none()
            && self.contrast_policy.is_none()
            && self.bold.is_none()
            && self.dim.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.reverse.is_none()
            && self.strikethrough.is_none()
            && self.underline_color.is_none()
            && self.dim_amount.is_none()
            && self.tint.is_none()
    }

    /// Merge another style on top of this one.
    ///
    /// Colors from `other` take precedence if set.
    /// Modifiers from `other` take precedence if set (Some).
    ///
    /// Color transforms (`transform_fg` / `transform_bg`) on `other` compose
    /// on the *previous resolved color*: if `self` has an explicit color and
    /// `other` only has a transform, the transform is applied to that resolved
    /// color and baked in. Chaining `.patch()` calls therefore stacks
    /// transforms in cascade order (e.g. `base.patch(hover).patch(focus)`
    /// applies hover's transform to base, then focus's transform to the
    /// hover-resolved color). Transforms only carry forward unresolved when
    /// no color is yet available - see `merge_channel`.
    pub fn patch(self, other: Style) -> Self {
        let (bg, bg_transform) = merge_channel(
            self.bg,
            self.bg_transform,
            other.bg,
            other.bg_transform,
            None,
        );
        let backdrop = bg.or(other.bg).or(self.bg);
        let (fg, fg_transform) = merge_channel(
            self.fg,
            self.fg_transform,
            other.fg,
            other.fg_transform,
            backdrop,
        );

        Self {
            fg,
            bg,
            fg_transform,
            bg_transform,
            contrast_policy: other.contrast_policy.or(self.contrast_policy),
            bold: other.bold.or(self.bold),
            dim: other.dim.or(self.dim),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            reverse: other.reverse.or(self.reverse),
            strikethrough: other.strikethrough.or(self.strikethrough),
            underline_color: merge_underline_color(other.underline_color, self.underline_color),
            dim_amount: other.dim_amount.or(self.dim_amount),
            tint: other.tint.or(self.tint),
        }
    }

    pub(crate) fn resolve_color_transforms(self) -> Self {
        let bg = resolve_channel(self.bg, self.bg_transform, None);
        let mut fg = resolve_channel(self.fg, self.fg_transform, bg);
        let mut fg_transform_remaining = None;

        if matches!(fg, Some(Paint::Solid(Color::Transparent))) {
            if let Some(c) = bg
                && !matches!(c, Paint::Solid(Color::Transparent | Color::Backdrop))
            {
                fg = if let Some(t) = self.fg_transform {
                    Some(t.apply_paint_with_backdrop(c, bg))
                } else {
                    Some(c)
                };
            } else {
                fg_transform_remaining = self.fg_transform;
            }
        }
        Self {
            fg,
            bg,
            fg_transform: fg_transform_remaining,
            bg_transform: None,
            ..self
        }
    }
}

fn merge_underline_color(overlay: Option<Paint>, base: Option<Paint>) -> Option<Paint> {
    match overlay {
        None => base,
        Some(Paint::Solid(Color::Transparent)) => base,
        Some(c) => Some(c),
    }
}

pub(crate) fn merge_channel(
    base_color: Option<Paint>,
    base_transform: Option<ColorTransform>,
    overlay_color: Option<Paint>,
    overlay_transform: Option<ColorTransform>,
    backdrop: Option<Paint>,
) -> (Option<Paint>, Option<ColorTransform>) {
    let mut color = resolve_channel(base_color, base_transform, backdrop);
    let mut transform = None;

    if let Some(overlay_color) = overlay_color
        && !matches!(overlay_color, Paint::Solid(Color::Transparent))
    {
        color = Some(overlay_color);
    }

    if let Some(overlay_transform) = overlay_transform {
        if let Some(current) = color
            && (!overlay_transform.needs_backdrop() || backdrop.is_some())
        {
            color = Some(overlay_transform.apply_paint_with_backdrop(current, backdrop));
        } else {
            transform = Some(overlay_transform.normalized());
        }
    }

    (color, transform)
}

pub(crate) fn resolve_channel(
    color: Option<Paint>,
    transform: Option<ColorTransform>,
    backdrop: Option<Paint>,
) -> Option<Paint> {
    match (color, transform) {
        (Some(color), Some(transform)) => {
            Some(transform.apply_paint_with_backdrop(color, backdrop))
        }
        (color, None) => color,
        (None, Some(_)) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorTransform, Style, Theme, ThemePalette, ThemeRole};
    use crate::app::ContrastPolicy;
    use crate::style::{Color, HostTerminalColors, Paint};

    fn p(color: Color) -> Option<Paint> {
        Some(Paint::Solid(color))
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BrandTheme {
        accent_badge: Color,
    }

    #[test]
    fn drag_drop_roles_initially_resolve_to_hover() {
        let theme = Theme::default().hover(Style::new().fg(Color::White).bg(Color::Blue));

        assert_eq!(theme.role(ThemeRole::DragSource), theme.hover);
        assert_eq!(theme.role(ThemeRole::DropTarget), theme.hover);
        assert_eq!(theme.role(ThemeRole::DropTargetActive), theme.hover);
    }

    #[test]
    fn text_selection_role_is_distinct_from_item_selection() {
        let theme = Theme::default()
            .selection(Style::new().fg(Color::Red))
            .text_selection(Style::new().fg(Color::Blue));

        assert_eq!(theme.role(ThemeRole::Selection), theme.selection);
        assert_eq!(theme.role(ThemeRole::TextSelection), theme.text_selection);
        assert_ne!(
            theme.role(ThemeRole::Selection),
            theme.role(ThemeRole::TextSelection)
        );
    }

    #[test]
    fn focus_decoration_does_not_change_unfocused_selection() {
        let selection = Style::new().fg(Color::Yellow).bg(Color::Blue);
        let theme = Theme::default()
            .selection(selection)
            .focus(Style::new().fg(Color::Green))
            .focus_decoration(false);

        assert!(theme.role(ThemeRole::Focus).is_empty());
        assert_eq!(theme.role(ThemeRole::UnfocusedSelection), selection);
    }

    #[test]
    fn theme_palette_derives_distinct_selection_colors() {
        let theme = ThemePalette::new(Color::White, Color::Black, Color::Blue)
            .selection(Color::Green)
            .text_selection(Color::Magenta)
            .into_theme();

        assert_eq!(theme.selection.fg, p(Color::Green));
        assert_eq!(theme.text_selection.fg, p(Color::Magenta));
    }

    #[test]
    fn from_host_colors_uses_host_palette() {
        let mut ansi = std::array::from_fn(|i| Color::rgb(i as u8, i as u8, i as u8));
        ansi[1] = Color::rgb(210, 30, 40);
        ansi[2] = Color::rgb(30, 210, 40);
        ansi[3] = Color::rgb(210, 180, 40);
        ansi[4] = Color::rgb(30, 80, 210);
        let colors = HostTerminalColors {
            fg: Color::rgb(230, 231, 232),
            bg: Color::rgb(10, 11, 12),
            ansi,
        };

        let theme = Theme::from_host_colors(colors);

        assert_eq!(theme.primary.fg, p(colors.fg));
        assert_eq!(theme.primary.bg, p(colors.bg));
        assert_eq!(theme.accent.fg, p(colors.ansi[4]));
        assert_eq!(theme.status.success, colors.ansi[2]);
        assert_eq!(theme.status.warning, colors.ansi[3]);
        assert_eq!(theme.status.error, colors.ansi[1]);
        assert_eq!(theme.status.info, colors.ansi[4]);
    }

    #[test]
    fn transform_fg_dims_inherited_color() {
        let base = Style::new().fg(Color::rgb(100, 120, 140));
        let overlay = Style::new().transform_fg(ColorTransform::Dim(0.5));

        assert_eq!(
            base.patch(overlay).resolve_color_transforms().fg,
            p(Color::rgb(50, 60, 70))
        );
    }

    #[test]
    fn lower_fg_transform_does_not_affect_overlay_color() {
        let base = Style::new()
            .fg(Color::rgb(100, 120, 140))
            .transform_fg(ColorTransform::Dim(0.5));
        let overlay = Style::new().fg(Color::rgb(10, 20, 30));

        assert_eq!(
            base.patch(overlay).resolve_color_transforms().fg,
            p(Color::rgb(10, 20, 30))
        );
    }

    #[test]
    fn patch_transparent_fg_preserves_base() {
        let base = Style::new().fg(Color::rgb(10, 20, 30));
        let overlay = Style::new().fg(Color::Transparent);
        assert_eq!(
            base.patch(overlay).resolve_color_transforms().fg,
            p(Color::rgb(10, 20, 30))
        );
    }

    #[test]
    fn patch_transparent_bg_preserves_base() {
        let base = Style::new().bg(Color::rgb(40, 50, 60));
        let overlay = Style::new().bg(Color::Transparent);
        assert_eq!(
            base.patch(overlay).resolve_color_transforms().bg,
            p(Color::rgb(40, 50, 60))
        );
    }

    #[test]
    fn patch_alpha_zero_bg_is_not_transparent_sentinel() {
        let base = Style::new().bg(Color::rgb(40, 50, 60));
        let overlay = Style::new().bg_alpha(Color::Red, 0.0);
        assert_eq!(
            base.patch(overlay).resolve_color_transforms().bg,
            Some(Paint::Alpha {
                color: Color::Red,
                alpha: 0,
            })
        );
    }

    #[test]
    fn color_transform_apply_paint_preserves_alpha() {
        let paint = Paint::Alpha {
            color: Color::rgb(100, 120, 140),
            alpha: 128,
        };

        assert_eq!(
            ColorTransform::Dim(0.5).apply_paint(paint),
            Paint::Alpha {
                color: Color::rgb(50, 60, 70),
                alpha: 128,
            }
        );
    }

    #[test]
    fn patch_transparent_underline_color_preserves_base() {
        let base = Style::new().underline_color(Color::Red);
        let overlay = Style::new().underline_color(Color::Transparent);
        let patched = base.patch(overlay);
        assert_eq!(patched.underline_color, p(Color::Red));
    }

    #[test]
    fn builder_order_does_not_change_transform_result() {
        let a = Style::new()
            .transform_fg(ColorTransform::Dim(0.5))
            .fg(Color::rgb(100, 120, 140));
        let b = Style::new()
            .fg(Color::rgb(100, 120, 140))
            .transform_fg(ColorTransform::Dim(0.5));

        assert_eq!(a.resolve_color_transforms(), b.resolve_color_transforms());
        assert_eq!(a.resolve_color_transforms().fg, p(Color::rgb(50, 60, 70)));
    }

    #[test]
    fn transform_chain_applies_in_patch_order() {
        let style = Style::new()
            .fg(Color::rgb(100, 120, 140))
            .patch(Style::new().transform_fg(ColorTransform::Dim(0.5)))
            .patch(Style::new().transform_fg(ColorTransform::Lighten(0.5)))
            .resolve_color_transforms();

        assert_eq!(style.fg, p(Color::rgb(153, 158, 163)));
    }

    #[test]
    fn state_cascade_stacks_bg_transforms_on_resolved_color() {
        // Models base.patch(hover).patch(focus) where hover and focus only
        // carry transforms - each must compose on the previous resolved bg,
        // not independently against the original base.
        let style = Style::new()
            .bg(Color::rgb(100, 100, 100))
            .patch(Style::new().transform_bg(ColorTransform::Dim(0.5)))
            .patch(Style::new().transform_bg(ColorTransform::Dim(0.5)))
            .resolve_color_transforms();

        assert_eq!(style.bg, p(Color::rgb(25, 25, 25)));
    }

    #[test]
    fn opacity_turns_foreground_into_alpha_paint() {
        let style = Style::new()
            .fg(Color::rgb(245, 167, 66))
            .bg(Color::rgb(255, 255, 255))
            .transform_fg(ColorTransform::Opacity(0.6))
            .resolve_color_transforms();

        assert_eq!(
            style.fg,
            Some(Paint::Alpha {
                color: Color::rgb(245, 167, 66),
                alpha: 153,
            })
        );
    }

    #[test]
    fn opacity_builder_order_is_independent_when_background_arrives_later() {
        let a = Style::new()
            .transform_fg(ColorTransform::Opacity(0.6))
            .fg(Color::rgb(245, 167, 66))
            .bg(Color::rgb(255, 255, 255));
        let b = Style::new()
            .fg(Color::rgb(245, 167, 66))
            .bg(Color::rgb(255, 255, 255))
            .transform_fg(ColorTransform::Opacity(0.6));

        assert_eq!(a.resolve_color_transforms(), b.resolve_color_transforms());
    }

    #[test]
    fn opacity_toward_uses_fixed_target_not_backdrop() {
        let c = Color::rgb(0, 100, 200);
        let target = Color::rgb(200, 10, 30);
        assert_eq!(
            ColorTransform::OpacityToward {
                factor: 1.0,
                target,
            }
            .apply_with_backdrop(c, Some(Color::White)),
            c
        );
        assert_eq!(
            ColorTransform::OpacityToward {
                factor: 0.0,
                target,
            }
            .apply_with_backdrop(c, Some(Color::White)),
            target
        );
    }

    #[test]
    fn patch_prefers_overlay_contrast_policy() {
        let base = Style::new().contrast_policy(ContrastPolicy::Wcag);
        let overlay = Style::new().contrast_policy(ContrastPolicy::Off);

        assert_eq!(
            base.patch(overlay).contrast_policy,
            Some(ContrastPolicy::Off)
        );
    }

    #[test]
    fn theme_extensions_roundtrip_and_affect_equality() {
        let a = Theme::default().with_extension(BrandTheme {
            accent_badge: Color::rgb(1, 2, 3),
        });
        let b = Theme::default().with_extension(BrandTheme {
            accent_badge: Color::rgb(1, 2, 3),
        });
        let c = Theme::default().with_extension(BrandTheme {
            accent_badge: Color::rgb(9, 8, 7),
        });

        assert_eq!(a.extension::<BrandTheme>(), b.extension::<BrandTheme>());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

/// Visual shape of the caret/cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaretShape {
    /// Block cursor (█), usually rendered with reverse video.
    #[default]
    Block,
    /// Bar cursor (│), rendered as a vertical line.
    Bar,
    /// Underline cursor (_), rendered as an underline.
    Underline,
}

/// Custom glyphs for borders.
///
/// Mirrors `ratatui::symbols::border::Set`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BorderGlyphs {
    /// Top left corner.
    pub top_left: &'static str,
    /// Top horizontal line.
    pub top: &'static str,
    /// Top right corner.
    pub top_right: &'static str,
    /// Left vertical line.
    pub left: &'static str,
    /// Right vertical line.
    pub right: &'static str,
    /// Bottom left corner.
    pub bottom_left: &'static str,
    /// Bottom horizontal line.
    pub bottom: &'static str,
    /// Bottom right corner.
    pub bottom_right: &'static str,
}

impl Default for BorderGlyphs {
    fn default() -> Self {
        Self::PLAIN
    }
}

impl BorderGlyphs {
    /// Standard plain border glyphs.
    pub const PLAIN: Self = Self {
        top_left: "┌",
        top: "─",
        top_right: "┐",
        left: "│",
        right: "│",
        bottom_left: "└",
        bottom: "─",
        bottom_right: "┘",
    };

    /// Create a new custom border glyph set.
    pub fn new(parts: BorderGlyphsParts) -> Self {
        Self {
            top_left: parts.top_left,
            top: parts.top,
            top_right: parts.top_right,
            left: parts.left,
            right: parts.right,
            bottom_left: parts.bottom_left,
            bottom: parts.bottom,
            bottom_right: parts.bottom_right,
        }
    }
}

/// Corner and edge glyphs for [`BorderGlyphs::new`].
pub struct BorderGlyphsParts {
    /// Top left corner.
    pub top_left: &'static str,
    /// Top horizontal line.
    pub top: &'static str,
    /// Top right corner.
    pub top_right: &'static str,
    /// Left vertical line.
    pub left: &'static str,
    /// Right vertical line.
    pub right: &'static str,
    /// Bottom left corner.
    pub bottom_left: &'static str,
    /// Bottom horizontal line.
    pub bottom: &'static str,
    /// Bottom right corner.
    pub bottom_right: &'static str,
}

/// Border style for widgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    /// Standard single-line border (─│┌┐└┘).
    #[default]
    Plain,
    /// Rounded corners (─│╭╮╰╯).
    Rounded,
    /// Double-line border (═║╔╗╚╝).
    Double,
    /// Thick border.
    Thick,
    /// Light double-dashed border.
    LightDoubleDashed,
    /// Heavy double-dashed border.
    HeavyDoubleDashed,
    /// Light triple-dashed border.
    LightTripleDashed,
    /// Heavy triple-dashed border.
    HeavyTripleDashed,
    /// Light quadruple-dashed border.
    LightQuadrupleDashed,
    /// Heavy quadruple-dashed border.
    HeavyQuadrupleDashed,
    /// Custom border glyphs.
    Custom {
        /// The glyph set to use.
        glyphs: BorderGlyphs,
    },
}

/// Scrollbar rendering variant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollbarVariant {
    /// Integrate scrollbar into the right border (lazygit-style).
    /// Falls back to `Standalone` when widget has no border.
    Integrated,
    /// Render scrollbar as a separate column consuming content width.
    #[default]
    Standalone,
}

/// Scrollbar appearance (layout variant, gap, thumb, track styles).
///
/// Visibility is controlled by each widget's `.scrollbar(bool)` /
/// `.h_scrollbar(bool)`, not by this struct.
///
/// Node structs keep flat fields for efficient hot-path access; reconcile
/// unpacks `ScrollbarConfig` into individual fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollbarConfig {
    /// Rendering variant (integrated into border or standalone column).
    pub variant: ScrollbarVariant,
    /// Empty cells reserved between content and a standalone scrollbar.
    pub gap: u16,
    /// Custom thumb character (default: '█').
    pub thumb: Option<char>,
    /// Custom thumb style.
    pub thumb_style: Option<Style>,
    /// Custom thumb style when the widget is focused.
    pub thumb_focus_style: Option<Style>,
    /// Custom track style.
    pub track_style: Option<Style>,
}

impl ScrollbarConfig {
    /// Create a new `ScrollbarConfig` with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set scrollbar rendering variant.
    pub fn variant(mut self, variant: ScrollbarVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Reserve empty cells before a standalone scrollbar.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set custom thumb character.
    pub fn thumb(mut self, ch: char) -> Self {
        self.thumb = Some(ch);
        self
    }

    /// Set custom thumb style.
    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = Some(style);
        self
    }

    /// Set custom thumb style when the widget is focused.
    pub fn thumb_focus_style(mut self, style: Style) -> Self {
        self.thumb_focus_style = Some(style);
        self
    }

    /// Set custom track style.
    pub fn track_style(mut self, style: Style) -> Self {
        self.track_style = Some(style);
        self
    }
}

/// Palette of semantic colors used for file icons.
///
/// These categories match the semantic grouping used by `mini.icons` in Neovim.
#[derive(Clone, Debug, PartialEq)]
pub struct FileIconPalette {
    /// Light blue (e.g. Markdown, documentation)
    pub azure: Color,
    /// Standard blue (e.g. Directories, CSS)
    pub blue: Color,
    /// Bright cyan (e.g. TypeScript, Docker)
    pub cyan: Color,
    /// Standard green (e.g. Go, Shell scripts)
    pub green: Color,
    /// Neutral grey (e.g. Lock files, logs)
    pub grey: Color,
    /// Vibrant orange (e.g. Java, HTML)
    pub orange: Color,
    /// Rich purple (e.g. C++, images)
    pub purple: Color,
    /// Standard red (e.g. Rust, Git files)
    pub red: Color,
    /// Bright yellow (e.g. JavaScript, Python)
    pub yellow: Color,
}

impl Default for FileIconPalette {
    fn default() -> Self {
        Self {
            // One Dark inspired palette (vibrant and elastic)
            azure: Color::hex_u24(0x61AFEF),  // Light Blue
            blue: Color::hex_u24(0x4175E6),   // Standard Blue
            cyan: Color::hex_u24(0x56B6C2),   // Cyan
            green: Color::hex_u24(0x98C379),  // Green
            grey: Color::hex_u24(0xABB2BF),   // Grey
            orange: Color::hex_u24(0xD19A66), // Orange
            purple: Color::hex_u24(0xC678DD), // Purple
            red: Color::hex_u24(0xE06C75),    // Red
            yellow: Color::hex_u24(0xE5C07B), // Yellow
        }
    }
}

/// Palette of colors for git status indicators.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GitStatusPalette {
    /// Tracked file modified.
    pub modified: Color,
    /// New tracked file.
    pub added: Color,
    /// File deleted.
    pub deleted: Color,
    /// File renamed.
    pub renamed: Color,
    /// Untracked file.
    pub untracked: Color,
    /// Merge conflict.
    pub conflicted: Color,
}

impl Default for GitStatusPalette {
    fn default() -> Self {
        Self {
            modified: Color::hex_u24(0xE5B767),
            added: Color::hex_u24(0x7EC699),
            deleted: Color::hex_u24(0xE57E7E),
            renamed: Color::hex_u24(0x76C5E5),
            untracked: Color::hex_u24(0xC59AE5),
            conflicted: Color::hex_u24(0xE57E7E),
        }
    }
}

/// Palette of colors for scrollbars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarPalette {
    /// Track color (the background of the scrollbar).
    pub track: Option<Color>,
    /// Thumb color (the draggable part).
    pub thumb: Color,
    /// Thumb color when focused.
    pub thumb_focus: Option<Color>,
}

impl Default for ScrollbarPalette {
    fn default() -> Self {
        Self {
            track: None,
            thumb: Color::DarkGray,
            thumb_focus: Some(Color::Gray),
        }
    }
}

/// Palette of colors for splitter interaction states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitterPalette {
    /// Handle/seam color when hovered.
    pub hover: Color,
    /// Handle/seam color while actively dragging.
    pub active: Color,
}

impl Default for SplitterPalette {
    fn default() -> Self {
        Self {
            hover: Color::hex_u24(0x2563EB),
            active: Color::hex_u24(0x22D3EE),
        }
    }
}

/// Surface colors used for layered UI chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfacePalette {
    /// Base panel surface.
    pub panel: Color,
    /// Nested element/input surface.
    pub element: Color,
    /// Menu/popover surface.
    pub menu: Color,
    /// Backdrop/base underlay surface.
    pub backdrop: Color,
}

/// Semantic status colors used across widgets and apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StatusPalette {
    /// Success state color.
    pub success: Color,
    /// Warning state color.
    pub warning: Color,
    /// Error state color.
    pub error: Color,
    /// Informational state color.
    pub info: Color,
}

/// Semantic style palette for diff rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DiffPalette {
    /// Style for unchanged/context lines.
    pub context: Style,
    /// Style for added lines.
    pub added: Style,
    /// Style for removed lines.
    pub removed: Style,
    /// Style for filler/empty lines in split diff layouts.
    pub empty: Style,
    /// Style for added word-level segments.
    pub added_word: Style,
    /// Style for removed word-level segments.
    pub removed_word: Style,
    /// Style for the added marker in the prefix gutter.
    pub added_marker: Style,
    /// Style for the removed marker in the prefix gutter.
    pub removed_marker: Style,
    /// Style for the line-number segment of unchanged/context lines in the gutter.
    pub context_line_number: Style,
    /// Style for the line-number segment of added lines in the gutter.
    pub added_line_number: Style,
    /// Style for the line-number segment of removed lines in the gutter.
    pub removed_line_number: Style,
    /// Style for context-collapse separator lines.
    pub context_separator_style: Style,
    /// Style for unified-diff `diff --git …` metadata (and inline file headers in multi-file patches).
    pub patch_header: Style,
}

/// Semantic style palette for formatted documents and markdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocumentPalette {
    /// Heading styles (h1 through h6).
    pub heading_styles: [Style; 6],
    /// Inline code style.
    pub code_inline: Style,
    /// Code block style.
    pub code_block: Style,
    /// Emphasis style.
    pub emphasis: Style,
    /// Strong style.
    pub strong: Style,
    /// Strikethrough style.
    pub strikethrough: Style,
    /// Link style.
    pub link: Style,
    /// Blockquote bar style.
    pub blockquote_bar: Style,
    /// Table border style.
    pub table_border: Style,
    /// Table header style.
    pub table_header: Style,
    /// Horizontal-rule style.
    pub hr: Style,
    /// Bullet point style for unordered lists.
    pub list_item: Style,
    /// Enumeration number style for ordered lists.
    pub list_enumeration: Style,
    /// Diagram node fill style.
    pub diagram_node_fill_style: Style,
    /// Diagram node border style.
    pub diagram_node_border_style: Style,
    /// Diagram node label style.
    pub diagram_node_label_style: Style,
    /// Diagram edge style.
    pub diagram_edge_style: Style,
    /// Diagram muted style for auxiliary glyphs (sequence lifelines, etc.).
    pub diagram_muted_style: Style,
}

impl Default for DocumentPalette {
    fn default() -> Self {
        Self {
            heading_styles: [
                Style::new().bold().fg(Color::LightBlue),
                Style::new().bold().fg(Color::LightBlue),
                Style::new().bold().fg(Color::LightBlue),
                Style::new().bold(),
                Style::new().bold(),
                Style::new().bold().dim(),
            ],
            code_inline: Style::new().fg(Color::Green),
            code_block: Style::default(),
            emphasis: Style::new().italic(),
            strong: Style::new().bold(),
            strikethrough: Style::new().strikethrough(),
            link: Style::new().fg(Color::LightBlue).underline(),
            blockquote_bar: Style::new().fg(Color::DarkGray).dim(),
            table_border: Style::new().fg(Color::DarkGray).dim(),
            table_header: Style::new().bold(),
            hr: Style::new().fg(Color::DarkGray).dim(),
            list_item: Style::new().fg(Color::LightBlue).bold(),
            list_enumeration: Style::new().fg(Color::LightBlue).bold(),
            diagram_node_fill_style: Style::default(),
            diagram_node_border_style: Style::default(),
            diagram_node_label_style: Style::default(),
            diagram_edge_style: Style::default(),
            diagram_muted_style: Style::default(),
        }
    }
}

/// Semantic style palette for syntax highlighting overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyntaxPalette {
    /// Style for comments and documentation text.
    pub comment: Style,
    /// Style for keywords and language control words.
    pub keyword: Style,
    /// Style for string literals.
    pub string: Style,
    /// Style for numeric literals.
    pub number: Style,
    /// Style for named constants, booleans, null-like values, and character literals.
    pub constant: Style,
    /// Style for function and method identifiers.
    pub function: Style,
    /// Style for built-in functions, types, classes, and constants.
    pub builtin: Style,
    /// Style for type names and class-like identifiers.
    pub type_name: Style,
    /// Style for regular identifiers/variables.
    pub variable: Style,
    /// Style for function parameters and argument-like bindings.
    pub parameter: Style,
    /// Style for operators and punctuation-like emphasis.
    pub operator: Style,
}

impl Default for SyntaxPalette {
    fn default() -> Self {
        let number = Style::new().fg(Color::Yellow);
        let function = Style::new().fg(Color::Cyan);
        let variable = Style::new().fg(Color::White);

        Self {
            comment: Style::new().fg(Color::DarkGray).italic().dim(),
            keyword: Style::new().fg(Color::LightMagenta),
            string: Style::new().fg(Color::Green),
            number,
            constant: number.lighten_by(0.12),
            function,
            builtin: function.italic(),
            type_name: Style::new().fg(Color::LightBlue),
            variable,
            parameter: variable.italic(),
            operator: Style::new().fg(Color::LightRed),
        }
    }
}

/// Semantic interaction palette for single-line text inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InputPalette {
    /// Style applied to focused input content when the theme opts in.
    ///
    /// This is intentionally empty by default so focused inputs keep their base
    /// text color unless the theme author explicitly requests otherwise.
    pub focus: Style,
}

/// Semantic interaction palette for multi-line text editors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextAreaPalette {
    /// Style applied to focused text-area content when the theme opts in.
    ///
    /// This is intentionally empty by default so focused editors keep their
    /// base text color unless the theme author explicitly requests otherwise.
    pub focus: Style,
}

/// Semantic interaction palette for read-only document surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DocumentViewPalette {
    /// Style applied to focused document content when the theme opts in.
    ///
    /// This is intentionally empty by default so focused documents keep their
    /// base text color unless the theme author explicitly requests otherwise.
    pub focus: Style,
}

/// Semantic interaction palette for hex editors/viewers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HexAreaPalette {
    /// Style applied to focused hex content when the theme opts in.
    pub focus: Style,
    /// Style applied to the active hex cursor/caret.
    pub cursor: Style,
}

/// Semantic interaction palette for terminal surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TerminalPalette {
    /// Style applied to focused terminal content when the theme opts in.
    ///
    /// This is intentionally empty by default so focused terminals keep their
    /// base text color unless the theme author explicitly requests otherwise.
    pub focus: Style,
}

/// Theme palette for common widget defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Primary style (e.g. text color).
    pub primary: Style,
    /// Style for interactive emphasis.
    ///
    /// Used for control hover, cursors, active glyphs, and other
    /// foreground-only emphasis that should not imply selection ownership.
    pub accent: Style,
    /// Style for selected/current items.
    pub selection: Style,
    /// Style for selected text or byte ranges.
    pub text_selection: Style,
    /// Style for focused widget chrome and focus affordances.
    ///
    /// Resolved at render time by widgets whose focus slot inherits or extends
    /// the theme role. Keep this empty to suppress theme-provided focus visuals
    /// while still allowing explicit widget-level `focus_style(...)` overrides.
    pub focus: Style,
    /// Whether theme-provided focus decoration is enabled.
    ///
    /// Disabling this suppresses focus roles derived from the theme, including
    /// focused-content palettes and focused scrollbar thumbs. Explicit widget
    /// focus styles still apply.
    pub focus_decoration: bool,
    /// Style for hovered items.
    pub hover: Style,
    /// Style for borders and frames.
    ///
    /// When set, frame borders and dividers use this `fg` instead of
    /// `primary.fg`, letting you dim borders independently of text.
    pub border: Style,
    /// Style for muted/secondary content.
    ///
    /// Applied to placeholders, disabled widgets, line numbers, scroll
    /// indicators, and empty-state text.
    pub muted: Style,
    /// Derived layered UI surfaces.
    pub surface: SurfacePalette,
    /// Semantic status colors.
    pub status: StatusPalette,
    /// Active/emphasized border color.
    pub border_active: Color,
    /// Color palette for file icons.
    pub file_icons: FileIconPalette,
    /// Colors for git status.
    pub git_status: GitStatusPalette,
    /// Semantic styles for diff views.
    pub diff: DiffPalette,
    /// Semantic styles for formatted documents and markdown.
    pub document: DocumentPalette,
    /// Semantic styles for syntax token recoloring.
    pub syntax: SyntaxPalette,
    /// Semantic interaction styles for single-line inputs.
    pub input: InputPalette,
    /// Semantic interaction styles for multi-line text editors.
    pub text_area: TextAreaPalette,
    /// Semantic interaction styles for read-only document surfaces.
    pub document_view: DocumentViewPalette,
    /// Semantic interaction styles for hex editors/viewers.
    pub hex_area: HexAreaPalette,
    /// Semantic interaction styles for terminal surfaces.
    pub terminal: TerminalPalette,
    /// Colors for scrollbars.
    pub scrollbar: ScrollbarPalette,
    /// Colors for splitter interaction states.
    pub splitter: SplitterPalette,
    /// Typed app-specific theme data stored alongside the framework palettes.
    #[doc(hidden)]
    pub extensions: ThemeExtensions,
}

impl Theme {
    /// Build a deterministic theme from probed host terminal colors.
    pub fn from_host_colors(colors: HostTerminalColors) -> Self {
        ThemePalette::new(colors.fg, colors.bg, colors.ansi[4])
            .success(colors.ansi[2])
            .warning(colors.ansi[3])
            .error(colors.ansi[1])
            .info(colors.ansi[4])
            .into_theme()
    }

    /// Return the style associated with a semantic theme role.
    pub fn role(&self, role: ThemeRole) -> Style {
        match role {
            ThemeRole::Base => self.primary,
            ThemeRole::Accent => {
                let mut style = self.accent;
                if style.fg.is_none() {
                    style.fg = self.primary.fg;
                }
                if style.fg_transform.is_none() {
                    style.fg_transform = self.primary.fg_transform;
                }
                style
            }
            ThemeRole::Selection | ThemeRole::UnfocusedSelection => self.selection,
            ThemeRole::TextSelection => self.text_selection,
            ThemeRole::Hover
            | ThemeRole::DragSource
            | ThemeRole::DropTarget
            | ThemeRole::DropTargetActive
            | ThemeRole::ItemHover => self.hover,
            ThemeRole::Focus if self.focus_decoration => self.focus,
            ThemeRole::Focus => Style::default(),
            ThemeRole::Active => self.selection,
            ThemeRole::Border => self.primary.patch(self.border),
            ThemeRole::Disabled | ThemeRole::Muted => self.primary.patch(self.muted),
            ThemeRole::Error => Style::new().fg(self.status.error),
            ThemeRole::InputFocusContent if self.focus_decoration => self.input.focus,
            ThemeRole::TextAreaFocusContent if self.focus_decoration => self.text_area.focus,
            ThemeRole::DocumentViewFocusContent if self.focus_decoration => {
                self.document_view.focus
            }
            ThemeRole::HexAreaFocusContent if self.focus_decoration => self.hex_area.focus,
            ThemeRole::HexAreaCursor if self.focus_decoration => self.hex_area.cursor,
            ThemeRole::TerminalFocusContent if self.focus_decoration => self.terminal.focus,
            ThemeRole::InputFocusContent
            | ThemeRole::TextAreaFocusContent
            | ThemeRole::DocumentViewFocusContent
            | ThemeRole::HexAreaFocusContent
            | ThemeRole::HexAreaCursor
            | ThemeRole::TerminalFocusContent => Style::default(),
            ThemeRole::ScrollbarThumb => Style::new().bg(self.scrollbar.thumb),
            ThemeRole::ScrollbarThumbFocus if self.focus_decoration => self
                .scrollbar
                .thumb_focus
                .map(|color| Style::new().bg(color))
                .unwrap_or_default(),
            ThemeRole::ScrollbarThumbFocus => Style::default(),
            ThemeRole::ScrollbarTrack => self
                .scrollbar
                .track
                .map(|color| Style::new().bg(color))
                .unwrap_or_default(),
            ThemeRole::SplitterHover => Style::new().fg(self.splitter.hover),
            ThemeRole::SplitterActive => Style::new().fg(self.splitter.active),
        }
    }

    /// Create a theme from the three core colors most apps care about.
    ///
    /// - `primary_fg`: default text/foreground color
    /// - `primary_bg`: base surface/background color
    /// - `accent`: interactive accent used for control emphasis, selection,
    ///   text selection, splitters, and focused scrollbar thumbs
    pub fn custom(primary_fg: Color, primary_bg: Color, accent: Color) -> Self {
        let success = Color::Green;
        let warning = Color::Yellow;
        let error = Color::Red;
        let info = accent;
        let muted = primary_fg.blend_toward(primary_bg, 0.42);
        let border_active = accent.lighten_by(0.08);
        Self {
            primary: Style::new().fg(primary_fg).bg(primary_bg),
            accent: Style::new().fg(accent),
            selection: Style::new()
                .fg(accent)
                .bg(primary_bg.blend_toward(accent, 0.22)),
            text_selection: Style::new()
                .fg(accent)
                .bg(primary_bg.blend_toward(accent, 0.22)),
            focus: Style::new().fg(border_active),
            focus_decoration: true,
            hover: Style::default(),
            border: Style::new().fg(primary_fg.blend_toward(primary_bg, 0.40)),
            muted: Style::new().fg(muted),
            surface: SurfacePalette {
                panel: primary_bg.elevate(0.07),
                element: primary_bg.elevate(0.04),
                menu: primary_bg.elevate(0.12),
                backdrop: primary_bg,
            },
            status: StatusPalette {
                success,
                warning,
                error,
                info,
            },
            border_active,
            file_icons: FileIconPalette::default(),
            git_status: GitStatusPalette::default(),
            diff: DiffPalette {
                context: Style::default(),
                added: Style::new().bg(primary_bg.blend_toward(success, 0.14)),
                removed: Style::new().bg(primary_bg.blend_toward(error, 0.16)),
                empty: Style::new().dim(),
                added_word: Style::new().bg(primary_bg.blend_toward(success, 0.24)),
                removed_word: Style::new().bg(primary_bg.blend_toward(error, 0.28)),
                added_marker: Style::new().fg(success),
                removed_marker: Style::new().fg(error),
                context_line_number: Style::new().fg(primary_fg.blend_toward(primary_bg, 0.50)),
                added_line_number: Style::default(),
                removed_line_number: Style::default(),
                context_separator_style: Style::new()
                    .fg(primary_fg.blend_toward(primary_bg, 0.40))
                    .dim(),
                patch_header: Style::new()
                    .fg(accent.blend_toward(primary_fg, 0.35))
                    .bold(),
            },
            document: DocumentPalette {
                heading_styles: [
                    Style::new().bold().fg(accent.lighten_by(0.20)),
                    Style::new().bold().fg(accent.lighten_by(0.12)),
                    Style::new().bold().fg(accent),
                    Style::new().bold().fg(primary_fg),
                    Style::new().bold().fg(primary_fg),
                    Style::new().bold().fg(primary_fg).dim(),
                ],
                code_inline: Style::new().fg(success),
                code_block: Style::default(),
                emphasis: Style::new().italic(),
                strong: Style::new().bold(),
                strikethrough: Style::new().strikethrough(),
                link: Style::new().fg(accent).underline(),
                blockquote_bar: Style::new().fg(muted).dim(),
                table_border: Style::new()
                    .fg(primary_fg.blend_toward(primary_bg, 0.40))
                    .dim(),
                table_header: Style::new().bold(),
                hr: Style::new()
                    .fg(primary_fg.blend_toward(primary_bg, 0.40))
                    .dim(),
                list_item: Style::new().fg(accent).bold(),
                list_enumeration: Style::new().fg(accent).bold(),
                diagram_node_fill_style: Style::new().bg(primary_bg.blend_toward(accent, 0.10)),
                diagram_node_border_style: Style::new().fg(accent.lighten_by(0.08)),
                diagram_node_label_style: Style::new().fg(primary_fg),
                diagram_edge_style: Style::new().fg(accent.blend_toward(primary_fg, 0.20)),
                diagram_muted_style: Style::new().fg(muted).dim(),
            },
            syntax: SyntaxPalette {
                comment: Style::new().fg(muted).italic().dim(),
                keyword: Style::new().fg(accent),
                string: Style::new().fg(accent.blend_toward(success, 0.55)),
                number: Style::new().fg(accent.blend_toward(Color::Yellow, 0.60)),
                constant: Style::new()
                    .fg(accent.blend_toward(Color::Yellow, 0.52).lighten_by(0.10)),
                function: Style::new().fg(info.blend_toward(accent, 0.12)),
                builtin: Style::new().fg(info.blend_toward(accent, 0.28)).italic(),
                type_name: Style::new().fg(accent.blend_toward(info, 0.32)),
                variable: Style::new().fg(primary_fg),
                parameter: Style::new()
                    .fg(primary_fg.blend_toward(accent, 0.12))
                    .italic(),
                operator: Style::new().fg(accent.blend_toward(error, 0.45)),
            },
            input: InputPalette::default(),
            text_area: TextAreaPalette::default(),
            document_view: DocumentViewPalette::default(),
            hex_area: HexAreaPalette {
                focus: Style::default(),
                cursor: Style::new().fg(accent),
            },
            terminal: TerminalPalette::default(),
            scrollbar: ScrollbarPalette {
                track: Some(primary_bg.elevate(0.05)),
                thumb: primary_bg.elevate(0.20),
                thumb_focus: Some(accent.lighten_by(0.08)),
            },
            splitter: SplitterPalette {
                hover: accent.lighten_by(0.08),
                active: accent.lighten_by(0.18),
            },
            extensions: ThemeExtensions::default(),
        }
    }

    /// Set the primary text/foreground style.
    ///
    /// This is the base color for labels, text, and borders across all widgets.
    /// Start from a preset or `Theme::default()` and override only what you need:
    ///
    /// ```rust
    /// use tui_lipan::{Theme, Style, Color};
    ///
    /// let theme = Theme::default()
    ///     .primary(Style::new().fg(Color::hex_u24(0xE0E0E0)))
    ///     .selection(Style::new().fg(Color::hex_u24(0xFF8000)));
    /// ```
    pub fn primary(mut self, style: Style) -> Self {
        self.primary = style;
        self
    }

    /// Set the interactive accent style.
    ///
    /// Used for button hover, cursors, matches, active glyphs, and other
    /// non-selection emphasis. Unlike [`Theme::selection`], this should usually
    /// avoid painting a selection background.
    pub fn accent(mut self, style: Style) -> Self {
        self.accent = style;
        self
    }

    /// Set the focused-widget chrome style.
    ///
    /// `ThemeProvider` applies this to widget `focus_style` defaults. Keep it
    /// empty when you want the theme itself to stay visually quiet on focus
    /// while still allowing widgets to opt into explicit focus styles.
    pub fn focus(mut self, style: Style) -> Self {
        self.focus = style;
        self
    }

    /// Enable or disable focus decoration supplied by this theme.
    ///
    /// Explicit widget focus styles remain active when this is disabled.
    pub fn focus_decoration(mut self, focus_decoration: bool) -> Self {
        self.focus_decoration = focus_decoration;
        self
    }

    /// Attach typed app-specific theme data to this theme.
    ///
    /// This keeps app semantic tokens inside the framework theme tree so app
    /// code can read them through `Context::theme_extension::<T>()` while still
    /// relying on `ThemeProvider` as the single source of truth.
    pub fn with_extension<T>(mut self, extension: T) -> Self
    where
        T: ThemeExtension,
    {
        self.extensions.insert(extension);
        self
    }

    /// Remove a previously attached typed theme extension.
    pub fn without_extension<T>(mut self) -> Self
    where
        T: ThemeExtension,
    {
        self.extensions.remove::<T>();
        self
    }

    /// Return a typed app-specific theme extension if present.
    pub fn extension<T>(&self) -> Option<&T>
    where
        T: ThemeExtension,
    {
        self.extensions.get::<T>()
    }

    /// Return a cloned typed app-specific theme extension if present.
    pub fn extension_cloned<T>(&self) -> Option<T>
    where
        T: ThemeExtension,
    {
        self.extension::<T>().cloned()
    }

    /// Set the selected/current item style.
    ///
    /// Used for selected/current items in lists, tables, trees, and active tabs.
    /// Text/range selections use [`Theme::text_selection`] instead.
    pub fn selection(mut self, style: Style) -> Self {
        self.selection = style;
        self
    }

    /// Set the text/range selection style.
    ///
    /// Used for selected ranges in `Input`, `TextArea`, `DocumentView`,
    /// `Terminal`, and `HexArea`.
    pub fn text_selection(mut self, style: Style) -> Self {
        self.text_selection = style;
        self
    }

    /// Set the hover style.
    ///
    /// Hover is disabled by default for non-button widgets. Set this when you
    /// want row/surface hover feedback in lists, tables, trees, inputs, and
    /// similar widgets.
    pub fn hover(mut self, style: Style) -> Self {
        self.hover = style;
        self
    }

    /// Set the border/frame style.
    ///
    /// Controls the foreground color used for frame borders and dividers.
    /// When set, borders are decoupled from the primary text color.
    pub fn border(mut self, style: Style) -> Self {
        self.border = style;
        self
    }

    /// Set the muted/secondary style.
    ///
    /// Used for placeholders, disabled widgets, line numbers, scroll
    /// indicators, and empty-state text.
    pub fn muted(mut self, style: Style) -> Self {
        self.muted = style;
        self
    }

    /// Set the scrollbar color palette.
    pub fn scrollbar(mut self, palette: ScrollbarPalette) -> Self {
        self.scrollbar = palette;
        self
    }

    /// Set the splitter color palette.
    pub fn splitter(mut self, palette: SplitterPalette) -> Self {
        self.splitter = palette;
        self
    }

    /// Set the file icon color palette.
    pub fn file_icons(mut self, palette: FileIconPalette) -> Self {
        self.file_icons = palette;
        self
    }

    /// Set the git status color palette.
    pub fn git_status(mut self, palette: GitStatusPalette) -> Self {
        self.git_status = palette;
        self
    }

    /// Set the semantic diff style palette.
    pub fn diff(mut self, palette: DiffPalette) -> Self {
        self.diff = palette;
        self
    }

    /// Set the semantic document/markdown style palette.
    pub fn document(mut self, palette: DocumentPalette) -> Self {
        self.document = palette;
        self
    }

    /// Set the semantic syntax highlighting palette.
    pub fn syntax(mut self, palette: SyntaxPalette) -> Self {
        self.syntax = palette;
        self
    }

    /// Set the semantic interaction palette for single-line inputs.
    pub fn input(mut self, palette: InputPalette) -> Self {
        self.input = palette;
        self
    }

    /// Set the semantic interaction palette for multi-line text editors.
    pub fn text_area(mut self, palette: TextAreaPalette) -> Self {
        self.text_area = palette;
        self
    }

    /// Set the semantic interaction palette for read-only document surfaces.
    pub fn document_view(mut self, palette: DocumentViewPalette) -> Self {
        self.document_view = palette;
        self
    }

    /// Set the semantic interaction palette for hex editors/viewers.
    pub fn hex_area(mut self, palette: HexAreaPalette) -> Self {
        self.hex_area = palette;
        self
    }

    /// Set the semantic interaction palette for terminal surfaces.
    pub fn terminal(mut self, palette: TerminalPalette) -> Self {
        self.terminal = palette;
        self
    }
}

/// A minimal color palette that derives a complete [`Theme`].
///
/// Set 3 required colors (text, background, accent) and optionally override
/// a few more. Everything else - accent, selection, text selection, border, muted, scrollbar,
/// splitter, toast, diff, document, syntax, text-surface interaction, file-icon,
/// and git-status palettes - is derived automatically so every widget in the
/// tree shares a coherent look. Focus chrome is derived separately from the
/// accent token so apps can mute or restyle focus without affecting hover,
/// cursors, or other accent-driven states. Generic hover is intentionally
/// disabled by default and can be opted into later with [`Theme::hover`].
///
/// # Quick start
///
/// ```rust
/// use tui_lipan::{ThemePalette, Color};
///
/// // Three colors → full theme
/// let theme = ThemePalette::new(
///     Color::hex_u24(0xCDD6F4),  // text
///     Color::hex_u24(0x1E1E2E),  // background
///     Color::hex_u24(0xCBA6F7),  // accent
/// ).into_theme();
///
/// // Override just what you need
/// let theme = ThemePalette::new(
///     Color::hex_u24(0xCDD6F4),
///     Color::hex_u24(0x1E1E2E),
///     Color::hex_u24(0xCBA6F7),
/// )
/// .border(Color::hex_u24(0x585B70))
/// .selection(Color::hex_u24(0xCBA6F7))
/// .text_selection(Color::hex_u24(0x89B4FA))
/// .success(Color::hex_u24(0xA6E3A1))
/// .error(Color::hex_u24(0xF38BA8))
/// .into_theme();
/// ```
#[derive(Clone, Debug)]
pub struct ThemePalette {
    /// Main text/foreground color. Applied to all text and labels.
    pub text: Color,
    /// Primary background color.
    pub background: Color,
    /// Accent color used to derive interactive emphasis and default selection styles.
    pub accent: Color,
    /// Color used to derive selected/current item styles. Default: accent.
    pub selection: Option<Color>,
    /// Color used to derive text/range selection styles. Default: accent.
    pub text_selection: Option<Color>,
    /// Border/frame color. Default: text blended 40 % toward background.
    pub border: Option<Color>,
    /// Muted/secondary text color (placeholders, disabled, line numbers).
    /// Default: text blended 50 % toward background.
    pub muted: Option<Color>,
    /// Scrollbar thumb color. Default: background lightened 16 %.
    pub scrollbar: Option<Color>,
    /// Success/green semantic color. Default: `#34D399`.
    pub success: Option<Color>,
    /// Warning/yellow semantic color. Default: `#FBBF24`.
    pub warning: Option<Color>,
    /// Error/red semantic color. Default: `#F43F5E`.
    pub error: Option<Color>,
    /// Info/blue semantic color. Default: accent.
    pub info: Option<Color>,
}

impl ThemePalette {
    /// Create a palette from the three essential colors.
    pub fn new(text: Color, background: Color, accent: Color) -> Self {
        Self {
            text,
            background,
            accent,
            selection: None,
            text_selection: None,
            border: None,
            muted: None,
            scrollbar: None,
            success: None,
            warning: None,
            error: None,
            info: None,
        }
    }

    /// Override the selected/current item color.
    pub fn selection(mut self, color: Color) -> Self {
        self.selection = Some(color);
        self
    }

    /// Override the text/range selection color.
    pub fn text_selection(mut self, color: Color) -> Self {
        self.text_selection = Some(color);
        self
    }

    /// Override the border/frame color.
    pub fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }

    /// Override the muted/secondary text color.
    pub fn muted(mut self, color: Color) -> Self {
        self.muted = Some(color);
        self
    }

    /// Override the scrollbar thumb color.
    pub fn scrollbar(mut self, color: Color) -> Self {
        self.scrollbar = Some(color);
        self
    }

    /// Override the success semantic color.
    pub fn success(mut self, color: Color) -> Self {
        self.success = Some(color);
        self
    }

    /// Override the warning semantic color.
    pub fn warning(mut self, color: Color) -> Self {
        self.warning = Some(color);
        self
    }

    /// Override the error semantic color.
    pub fn error(mut self, color: Color) -> Self {
        self.error = Some(color);
        self
    }

    /// Override the info semantic color.
    pub fn info(mut self, color: Color) -> Self {
        self.info = Some(color);
        self
    }

    /// Convert this palette into a fully-derived [`Theme`].
    pub fn into_theme(self) -> Theme {
        Theme::from(self)
    }
}

impl From<ThemePalette> for Theme {
    fn from(p: ThemePalette) -> Self {
        let border_color = p
            .border
            .unwrap_or_else(|| p.text.blend_toward(p.background, 0.40));
        let muted_color = p
            .muted
            .unwrap_or_else(|| p.text.blend_toward(p.background, 0.42));
        let scrollbar_thumb = p.scrollbar.unwrap_or_else(|| p.background.elevate(0.20));

        let success = p.success.unwrap_or(Color::hex_u24(0x34D399));
        let warning = p.warning.unwrap_or(Color::hex_u24(0xFBBF24));
        let error = p.error.unwrap_or(Color::hex_u24(0xF43F5E));
        let info = p.info.unwrap_or(p.accent);
        let border_active = p.border.unwrap_or(p.accent).lighten_by(0.08);
        let selection = p.selection.unwrap_or(p.accent);
        let text_selection = p.text_selection.unwrap_or(p.accent);

        Theme {
            primary: Style::new().fg(p.text).bg(p.background),
            accent: Style::new().fg(p.accent),
            selection: Style::new()
                .fg(selection)
                .bg(p.background.blend_toward(selection, 0.22)),
            text_selection: Style::new()
                .fg(text_selection)
                .bg(p.background.blend_toward(text_selection, 0.22)),
            focus: Style::new().fg(border_active),
            focus_decoration: true,
            hover: Style::default(),
            border: Style::new().fg(border_color),
            muted: Style::new().fg(muted_color),
            surface: SurfacePalette {
                panel: p.background.elevate(0.07),
                element: p.background.elevate(0.04),
                menu: p.background.elevate(0.12),
                backdrop: p.background,
            },
            status: StatusPalette {
                success,
                warning,
                error,
                info,
            },
            border_active,
            file_icons: FileIconPalette {
                green: success,
                red: error,
                yellow: warning,
                azure: info,
                blue: p.accent,
                cyan: info.lighten_by(0.10),
                grey: muted_color,
                orange: warning.blend_toward(error, 0.40),
                purple: p.accent.blend_toward(error, 0.30),
            },
            git_status: GitStatusPalette {
                modified: warning,
                added: success,
                deleted: error,
                renamed: info,
                untracked: p.accent.blend_toward(error, 0.30),
                conflicted: error,
            },
            diff: DiffPalette {
                context: Style::default(),
                added: Style::new().bg(p.background.blend_toward(success, 0.14)),
                removed: Style::new().bg(p.background.blend_toward(error, 0.16)),
                empty: Style::new().dim(),
                added_word: Style::new().bg(p.background.blend_toward(success, 0.24)),
                removed_word: Style::new().bg(p.background.blend_toward(error, 0.28)),
                added_marker: Style::new().fg(success),
                removed_marker: Style::new().fg(error),
                context_line_number: Style::new().fg(p.text.blend_toward(p.background, 0.50)),
                added_line_number: Style::default(),
                removed_line_number: Style::default(),
                context_separator_style: Style::new()
                    .fg(p.text.blend_toward(p.background, 0.40))
                    .dim(),
                patch_header: Style::new().fg(p.accent.blend_toward(p.text, 0.25)).bold(),
            },
            document: DocumentPalette {
                heading_styles: [
                    Style::new().bold().fg(p.accent.lighten_by(0.20)),
                    Style::new().bold().fg(p.accent.lighten_by(0.12)),
                    Style::new().bold().fg(p.accent),
                    Style::new().bold().fg(p.text),
                    Style::new().bold().fg(p.text),
                    Style::new().bold().fg(p.text).dim(),
                ],
                code_inline: Style::new().fg(success),
                code_block: Style::default(),
                emphasis: Style::new().italic(),
                strong: Style::new().bold(),
                strikethrough: Style::new().strikethrough(),
                link: Style::new().fg(p.accent).underline(),
                blockquote_bar: Style::new().fg(muted_color).dim(),
                table_border: Style::new().fg(border_color).dim(),
                table_header: Style::new().bold(),
                hr: Style::new().fg(border_color).dim(),
                list_item: Style::new().fg(p.accent).bold(),
                list_enumeration: Style::new().fg(p.accent).bold(),
                diagram_node_fill_style: Style::new().bg(p.background.blend_toward(p.accent, 0.10)),
                diagram_node_border_style: Style::new().fg(p.accent.lighten_by(0.08)),
                diagram_node_label_style: Style::new().fg(p.text),
                diagram_edge_style: Style::new().fg(p.accent.blend_toward(p.text, 0.20)),
                diagram_muted_style: Style::new().fg(muted_color).dim(),
            },
            syntax: SyntaxPalette {
                comment: Style::new().fg(muted_color).italic().dim(),
                keyword: Style::new().fg(p.accent),
                string: Style::new().fg(success.blend_toward(p.accent, 0.15)),
                number: Style::new().fg(warning.blend_toward(p.accent, 0.20)),
                constant: Style::new().fg(warning.blend_toward(p.text, 0.18)),
                function: Style::new().fg(info.blend_toward(p.accent, 0.10)),
                builtin: Style::new()
                    .fg(info.blend_toward(muted_color, 0.22))
                    .italic(),
                type_name: Style::new().fg(p.accent.blend_toward(info, 0.35)),
                variable: Style::new().fg(p.text),
                parameter: Style::new().fg(p.text).italic(),
                operator: Style::new().fg(error.blend_toward(p.accent, 0.45)),
            },
            input: InputPalette::default(),
            text_area: TextAreaPalette::default(),
            document_view: DocumentViewPalette::default(),
            hex_area: HexAreaPalette {
                focus: Style::default(),
                cursor: Style::new().fg(p.accent),
            },
            terminal: TerminalPalette::default(),
            scrollbar: ScrollbarPalette {
                track: Some(p.background.elevate(0.05)),
                thumb: scrollbar_thumb,
                thumb_focus: Some(p.accent.lighten_by(0.08)),
            },
            splitter: SplitterPalette {
                hover: p.accent.lighten_by(0.08),
                active: p.accent.lighten_by(0.18),
            },
            extensions: ThemeExtensions::default(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xE2E8F0),
            Color::hex_u24(0x0B121F),
            Color::hex_u24(0x7DCFFF),
        )
        .success(Color::hex_u24(0x34D399))
        .warning(Color::hex_u24(0xFBBF24))
        .error(Color::hex_u24(0xF43F5E))
        .info(Color::hex_u24(0x38BDF8))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x7DCFFF),
            blue: Color::hex_u24(0x60A5FA),
            cyan: Color::hex_u24(0x2DD4BF),
            green: Color::hex_u24(0x4ADE80),
            grey: Color::hex_u24(0x94A3B8),
            orange: Color::hex_u24(0xFB923C),
            purple: Color::hex_u24(0xC4B5FD),
            red: Color::hex_u24(0xF87171),
            yellow: Color::hex_u24(0xFBBF24),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xFBBF24),
            added: Color::hex_u24(0x34D399),
            deleted: Color::hex_u24(0xFB7171),
            renamed: Color::hex_u24(0x38BDF8),
            untracked: Color::hex_u24(0xA78BFA),
            conflicted: Color::hex_u24(0xF43F5E),
        };

        theme
    }
}
