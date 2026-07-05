#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardCommand {
    Copy,
    Cut,
    Paste,
    PasteFromSelection,
    CopyImage,
    PasteImage,
}

impl ClipboardCommand {
    pub fn is_copy_or_cut(self) -> bool {
        matches!(self, Self::Copy | Self::Cut)
    }

    pub(crate) fn is_image(self) -> bool {
        matches!(self, Self::CopyImage | Self::PasteImage)
    }
}
