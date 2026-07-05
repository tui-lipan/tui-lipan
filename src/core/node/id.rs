use crate::utils::arena::ArenaId;

/// Runtime node identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl NodeId {
    /// An invalid node id.
    pub const INVALID: Self = Self {
        index: u32::MAX,
        generation: 0,
    };

    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) fn is_invalid(self) -> bool {
        self.index == u32::MAX
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::INVALID
    }
}

impl ArenaId for NodeId {
    const INVALID: Self = Self::INVALID;

    fn from_parts(index: u32, generation: u32) -> Self {
        Self::new(index, generation)
    }

    fn index(self) -> usize {
        self.index()
    }

    fn generation(self) -> u32 {
        self.generation
    }
}
