//! What a successful selection copy looks like.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::style::Style;

/// How a copied selection is highlighted after a successful copy.
///
/// Every effect is drawn with [`CopyFeedback::style`] over the widget's selection style and lasts
/// [`CopyFeedback::duration`]. Effects that change over time repaint on their own; the app does
/// not need a timer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CopyFeedbackEffect {
    /// No highlight.
    Off,
    /// The whole selection lights up at once and goes back when the time is up.
    #[default]
    Flash,
    /// The whole selection lights up at once, then eases back to the plain selection.
    Fade,
    /// A bright band runs through the selection in reading order, leaving a short trail.
    ///
    /// Terminals sweep cell by cell. Widgets that wrap or reflow text, such as inputs, text areas,
    /// and document views, fade instead.
    Scan,
    /// The selection lights up twice.
    Pulse,
}

impl CopyFeedbackEffect {
    /// Every effect, in the order a settings picker would list them.
    pub const ALL: &'static [Self] = &[Self::Off, Self::Flash, Self::Fade, Self::Scan, Self::Pulse];

    /// The stable lowercase name of this effect, as [`FromStr`] accepts it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Flash => "flash",
            Self::Fade => "fade",
            Self::Scan => "scan",
            Self::Pulse => "pulse",
        }
    }

    /// How long this effect runs unless the app asks for something else.
    ///
    /// A flash only has to register, while a scan has to travel the selection slowly enough to be
    /// followed.
    pub const fn default_duration(self) -> Duration {
        match self {
            Self::Off => Duration::ZERO,
            Self::Flash => Duration::from_millis(150),
            Self::Fade => Duration::from_millis(300),
            Self::Scan => Duration::from_millis(350),
            Self::Pulse => Duration::from_millis(420),
        }
    }

    /// Whether the highlight changes while it runs, so the renderer must repaint it every frame.
    pub const fn is_animated(self) -> bool {
        matches!(self, Self::Fade | Self::Scan | Self::Pulse)
    }
}

impl fmt::Display for CopyFeedbackEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error returned when a string names no [`CopyFeedbackEffect`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownCopyFeedbackEffect(String);

impl fmt::Display for UnknownCopyFeedbackEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown copy feedback effect {:?} (expected one of: off, flash, fade, scan, pulse)",
            self.0
        )
    }
}

impl std::error::Error for UnknownCopyFeedbackEffect {}

impl FromStr for CopyFeedbackEffect {
    type Err = UnknownCopyFeedbackEffect;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|effect| effect.as_str().eq_ignore_ascii_case(value.trim()))
            .ok_or_else(|| UnknownCopyFeedbackEffect(value.to_owned()))
    }
}

/// The feedback a selection copy gives, and what happens to the selection afterwards.
///
/// ```
/// use std::time::Duration;
/// use tui_lipan::prelude::*;
///
/// let feedback = CopyFeedback::new(CopyFeedbackEffect::Scan)
///     .duration(Duration::from_millis(400))
///     .on_select(true)
///     .clear_selection(true);
/// assert_eq!(feedback.effect, CopyFeedbackEffect::Scan);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CopyFeedback {
    /// How the copied selection is highlighted.
    pub effect: CopyFeedbackEffect,
    /// How long the highlight runs. Zero turns it off, as [`CopyFeedbackEffect::Off`] does.
    pub duration: Duration,
    /// The strongest highlight, merged onto the selection style. Effects that fade or sweep scale
    /// its color transforms and tint toward nothing; absolute colors and text modifiers stay on
    /// for the brighter half of the effect.
    pub style: Style,
    /// Also highlight a selection copied automatically when a mouse drag ends
    /// ([`CopyOnSelect`](crate::clipboard::CopyOnSelect)). Off by default, because that copy runs
    /// on every completed drag rather than when the user asks for one.
    pub on_select: bool,
    /// Clear a copied selection once its highlight ends, or straight away when there is no
    /// highlight. A selection the user has changed since the copy is left alone.
    pub clear_selection: bool,
}

impl CopyFeedback {
    /// The default style: a lighten strong enough to read on any selection color.
    pub fn default_style() -> Style {
        Style::new().lighten_by(0.35)
    }

    /// Feedback with `effect` at its [default duration](CopyFeedbackEffect::default_duration).
    pub fn new(effect: CopyFeedbackEffect) -> Self {
        Self {
            effect,
            duration: effect.default_duration(),
            style: Self::default_style(),
            on_select: false,
            clear_selection: false,
        }
    }

    /// No highlight. The selection is still cleared if [`Self::clear_selection`] asks for it.
    pub fn off() -> Self {
        Self::new(CopyFeedbackEffect::Off)
    }

    /// Set how long the highlight runs.
    #[must_use]
    pub const fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set the strongest highlight style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set whether copy-on-select highlights too.
    #[must_use]
    pub const fn on_select(mut self, on_select: bool) -> Self {
        self.on_select = on_select;
        self
    }

    /// Set whether a copied selection is cleared once its highlight ends.
    #[must_use]
    pub const fn clear_selection(mut self, clear_selection: bool) -> Self {
        self.clear_selection = clear_selection;
        self
    }

    /// Whether a copy shows any highlight.
    pub fn is_visible(&self) -> bool {
        self.effect != CopyFeedbackEffect::Off && !self.duration.is_zero()
    }
}

impl Default for CopyFeedback {
    fn default() -> Self {
        Self::new(CopyFeedbackEffect::Flash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_round_trip() {
        for effect in CopyFeedbackEffect::ALL {
            assert_eq!(effect.as_str().parse::<CopyFeedbackEffect>(), Ok(*effect));
        }
        assert_eq!(
            " Scan ".parse::<CopyFeedbackEffect>(),
            Ok(CopyFeedbackEffect::Scan)
        );
        assert!("sparkle".parse::<CopyFeedbackEffect>().is_err());
    }

    #[test]
    fn default_matches_the_historical_flash() {
        let feedback = CopyFeedback::default();
        assert_eq!(feedback.effect, CopyFeedbackEffect::Flash);
        assert_eq!(feedback.duration, Duration::from_millis(150));
        assert!(!feedback.on_select);
        assert!(!feedback.clear_selection);
        assert!(feedback.is_visible());
    }

    #[test]
    fn off_or_zero_duration_is_invisible() {
        assert!(!CopyFeedback::off().is_visible());
        assert!(
            !CopyFeedback::new(CopyFeedbackEffect::Scan)
                .duration(Duration::ZERO)
                .is_visible()
        );
    }
}
