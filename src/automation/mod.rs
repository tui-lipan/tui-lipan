//! Typed automation contracts and in-process sessions.

mod checkpoint;
mod identity;
mod operation;
mod selector;
mod semantic;
mod session;

pub use checkpoint::{
    Checkpoint, CheckpointArtifact, CheckpointBaseline, CheckpointFormat, CheckpointSink,
};
pub use identity::{AutomationId, AutomationIdError};
pub use operation::{
    AutomationError, AutomationScrollDirection, AutomationStep, AutomationStepResult, ClockMode,
    FocusDirection, IdleReport, WaitCondition,
};
pub use selector::{Selector, SelectorMatch};
pub use semantic::{
    SemanticAction, SemanticChecked, SemanticIter, SemanticNode, SemanticRole, SemanticTree,
    SemanticValue, ValueSensitivity,
};
pub use session::{AutomationOptions, AutomationSession, AutomationSnapshot};

#[cfg(feature = "ui-snapshot-json")]
pub(crate) use checkpoint::semantic_json;
pub(crate) use checkpoint::{semantic_markdown, validate_checkpoint_name};
pub(crate) use operation::AutomationStepKind;
pub(crate) use selector::resolve as resolve_selector;
pub(crate) use semantic::project_semantic_tree;
pub(crate) use session::evaluate_wait_condition;
