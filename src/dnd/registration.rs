//! What a program has registered with the terminal it runs in.

use std::sync::Arc;

use super::MachineId;

/// What the program in a terminal has registered: whether it accepts drops and offers drags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct ChildRegistration {
    /// Set while the program accepts drops.
    pub drops: Option<DropRegistration>,
    /// Set while the program offers drags.
    pub drags: Option<DragRegistration>,
}

impl ChildRegistration {
    /// Whether the program registered for anything.
    pub fn is_empty(&self) -> bool {
        self.drops.is_none() && self.drags.is_none()
    }
}

/// A program's registration for drops.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct DropRegistration {
    /// The `i` key the program registered with; every event for it carries the same.
    pub client_id: Option<u32>,
    /// Extra MIME types it asked to receive.
    pub mimes: Vec<Arc<str>>,
    /// The machine it runs on, when it said.
    pub machine_id: Option<Arc<str>>,
}

impl DropRegistration {
    /// The program's machine id, when it sent one.
    pub fn machine(&self) -> Option<MachineId> {
        self.machine_id.clone().map(MachineId::from_wire)
    }
}

/// A program's registration to offer drags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct DragRegistration {
    /// The `i` key the program registered with.
    pub client_id: Option<u32>,
    /// The machine it runs on, when it said.
    pub machine_id: Option<Arc<str>>,
}
