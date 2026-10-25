//! Mouse pointer shapes from the OSC 22 protocol.

/// A system-independent mouse pointer shape. Host terminals may approximate shapes they lack.
///
/// This controls the mouse pointer, independently of [`super::CaretShape`]. Use
/// [`crate::Element::pointer_shape`] to override a widget's automatic pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerShape {
    /// The `alias` pointer.
    Alias,
    /// The `cell` pointer.
    Cell,
    /// The `copy` pointer.
    Copy,
    /// The `crosshair` pointer.
    Crosshair,
    #[default]
    /// The `default` pointer.
    Default,
    /// The `e-resize` pointer.
    EResize,
    /// The `ew-resize` pointer.
    EwResize,
    /// The `grab` pointer.
    Grab,
    /// The `grabbing` pointer.
    Grabbing,
    /// The `help` pointer.
    Help,
    /// The `move` pointer.
    Move,
    /// The `n-resize` pointer.
    NResize,
    /// The `ne-resize` pointer.
    NeResize,
    /// The `nesw-resize` pointer.
    NeswResize,
    /// The `no-drop` pointer.
    NoDrop,
    /// The `not-allowed` pointer.
    NotAllowed,
    /// The `ns-resize` pointer.
    NsResize,
    /// The `nw-resize` pointer.
    NwResize,
    /// The `nwse-resize` pointer.
    NwseResize,
    /// The `pointer` pointer.
    Pointer,
    /// The `progress` pointer.
    Progress,
    /// The `s-resize` pointer.
    SResize,
    /// The `se-resize` pointer.
    SeResize,
    /// The `sw-resize` pointer.
    SwResize,
    /// The `text` pointer.
    Text,
    /// The `vertical-text` pointer.
    VerticalText,
    /// The `w-resize` pointer.
    WResize,
    /// The `wait` pointer.
    Wait,
    /// The `zoom-in` pointer.
    ZoomIn,
    /// The `zoom-out` pointer.
    ZoomOut,
}

impl PointerShape {
    const NAMES: &'static [(Self, &'static str)] = &[
        (Self::Alias, "alias"),
        (Self::Cell, "cell"),
        (Self::Copy, "copy"),
        (Self::Crosshair, "crosshair"),
        (Self::Default, "default"),
        (Self::EResize, "e-resize"),
        (Self::EwResize, "ew-resize"),
        (Self::Grab, "grab"),
        (Self::Grabbing, "grabbing"),
        (Self::Help, "help"),
        (Self::Move, "move"),
        (Self::NResize, "n-resize"),
        (Self::NeResize, "ne-resize"),
        (Self::NeswResize, "nesw-resize"),
        (Self::NoDrop, "no-drop"),
        (Self::NotAllowed, "not-allowed"),
        (Self::NsResize, "ns-resize"),
        (Self::NwResize, "nw-resize"),
        (Self::NwseResize, "nwse-resize"),
        (Self::Pointer, "pointer"),
        (Self::Progress, "progress"),
        (Self::SResize, "s-resize"),
        (Self::SeResize, "se-resize"),
        (Self::SwResize, "sw-resize"),
        (Self::Text, "text"),
        (Self::VerticalText, "vertical-text"),
        (Self::WResize, "w-resize"),
        (Self::Wait, "wait"),
        (Self::ZoomIn, "zoom-in"),
        (Self::ZoomOut, "zoom-out"),
    ];

    /// The OSC 22 / CSS shape name.
    pub fn as_str(self) -> &'static str {
        Self::NAMES[self as usize].1
    }

    /// Parse a standard OSC 22 shape name. Unknown names are ignored.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find_map(|&(shape, value)| (name == value).then_some(shape))
    }
}
