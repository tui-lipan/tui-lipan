//! Shared ASCII art constants for the tui-lipan showcase examples.

use tui_lipan::EffectAxis;
use tui_lipan::prelude::*;

#[allow(dead_code)]
pub const TAB_HERO: usize = 0;
#[allow(dead_code)]
pub const TAB_VOID: usize = 1;
#[allow(dead_code)]
pub const TAB_NEON: usize = 2;
#[allow(dead_code)]
pub const TAB_CRT_AMBER: usize = 3;
#[allow(dead_code)]
pub const TAB_QUAD: usize = 4;

pub const TAB_LABELS: &[&str] = &["Hero", "Void", "Neon", "CRT amber", "Quad"];

/// Braille-style mark (centered in the stage below).
pub const LOGO: &str = r"   ⢰⣦⣄ ⣠⣾⣷⣄ ⣠⣴⡆
   ⠈⣿⣿⣷⡙⠟⠻⢋⣾⣿⣿⠁
   ⢸⣿⣿⡟⣱⣿⣿⣎⢻⣿⣿⡇
   ⢸⣿⡟⣼⡟⢿⣿⣿⣧⢻⣿⡇
   ⢸⣿⢡⣿⡟⣠⣿⣿⣿⡌⣿⡇
   ⠈⢿⡆⣿⣿⣿⣷⣾⣿⢰⡿⠁
⢤⣤⣤⣄⣀⠙⠘⠻⠿⠿⠟⠃⠋⣀⣠⣤⣤⡤
⠘⣿⣿⣿⡻⣷⣦ ⢰⡆ ⣴⣾⢟⣿⣿⣿⠃
 ⠙⣿⣿⣿⣮⡻⣧⢸⡇⣼⢟⣵⣿⣿⣿⠋
  ⠈⠻⢿⣿⣷⡜⢸⡇⢣⣾⣿⡿⠟⠁
     ⠈⠉⠉⢸⡇⠉⠉⠁";

pub const TEXT: &str = r"
⣴⣶          ⣴⣦     ⣴⣶ ⣴⣦
⣿⣿⣤⣤ ⣤⣤  ⣤⣤ ⣬⣥     ⣿⣿ ⣬⣥  ⣠⣤⣤⣄   ⣠⣤⣤⣄   ⣠⣤⣤⣄
⣿⣿⠉⠉ ⣿⣿  ⣿⣿ ⣿⣿ ⣤⣤⣤ ⣿⣿ ⣿⣿ ⣾⣿⠋⠙⣿⣷ ⣾⣿⠋⠙⣿⣷ ⣾⣿⠋⠙⣿⣷
⣿⣿⣄⡀ ⣿⣿⡀⢀⣿⣿ ⣿⣿ ⠉⠉⠉ ⣿⣿ ⣿⣿ ⣿⣿⢀⣠⣿⣿ ⣿⣿⣄⡀⣿⣿ ⣿⣿  ⣿⣿
⠈⠻⠿⠿ ⠈⠻⠿⠿⠟⠁ ⠿⠿     ⠿⠿ ⠿⠿ ⣿⣿⠸⠿⠟⠁ ⠈⠻⠿⠇⠿⠿ ⠿⠿  ⠿⠿
                         ⣿⣿                  ";

/// Where the optional Braille **tui-lipan** banner sits relative to the AsciiCanvas stage.
#[derive(Clone, Copy, Default)]
pub enum BannerPlacement {
    Off,
    #[default]
    Above,
    Below,
    Left,
    Right,
}

impl BannerPlacement {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Above,
            Self::Above => Self::Below,
            Self::Below => Self::Left,
            Self::Left => Self::Right,
            Self::Right => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Above => "above",
            Self::Below => "below",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Brand gradient on Hero (horizontal sweep) vs other tabs (vertical), scope-local via [`VisualEffect::Gradient`].
pub fn brand_gradient_scope_effects(tab: usize) -> Vec<VisualEffect> {
    if tab == TAB_HERO {
        vec![VisualEffect::Gradient {
            gradient: ColorGradient::new(Color::Rgb(126, 58, 199), Color::Rgb(244, 114, 182))
                .with_center(Color::Rgb(185, 86, 205)),
            blend: 0.9,
            frequency: 1.15,
            speed: 0.35,
            axis: EffectAxis::Diagonal,
        }]
    } else {
        vec![VisualEffect::Gradient {
            gradient: ColorGradient::new(Color::Rgb(70, 40, 160), Color::Rgb(20, 160, 190))
                .with_center(Color::Rgb(50, 100, 220)),
            blend: 0.78,
            frequency: 0.8,
            speed: 0.1,
            axis: EffectAxis::Vertical,
        }]
    }
}
