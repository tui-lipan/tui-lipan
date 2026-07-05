//! Runtime command registry used by command palette style UIs.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::input::keymap::{Action, Keymap};
use crate::callback::Callback;
use crate::callback::ScopeId;

/// Stable identifier for a registered command.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandId(Arc<str>);

impl CommandId {
    /// Borrow this id as `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CommandId {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<String> for CommandId {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl From<Arc<str>> for CommandId {
    fn from(value: Arc<str>) -> Self {
        Self(value)
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One command entry in the runtime registry.
#[derive(Clone)]
pub struct CommandEntry {
    /// Stable command id.
    pub id: CommandId,
    /// Label shown in command palettes.
    pub label: Arc<str>,
    /// Optional longer description.
    pub description: Option<Arc<str>>,
    /// Optional category/group label.
    pub category: Option<Arc<str>>,
    /// Optional keybinding hint shown on the right.
    pub keybinding_hint: Option<Arc<str>>,
    /// Whether this command is currently actionable.
    pub enabled: bool,
    pub(crate) scope: Option<ScopeId>,
    /// Callback executed when command is run.
    pub handler: Callback<()>,
}

impl fmt::Debug for CommandEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandEntry")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("description", &self.description)
            .field("category", &self.category)
            .field("keybinding_hint", &self.keybinding_hint)
            .field("enabled", &self.enabled)
            .field("scope", &self.scope)
            .finish()
    }
}

impl CommandEntry {
    /// Create a command entry builder.
    pub fn builder(id: impl Into<CommandId>) -> CommandBuilder {
        CommandBuilder::new(id)
    }
}

/// Builder for [`CommandEntry`].
#[derive(Clone)]
pub struct CommandBuilder {
    id: CommandId,
    label: Arc<str>,
    description: Option<Arc<str>>,
    category: Option<Arc<str>>,
    keybinding_hint: Option<Arc<str>>,
    enabled: bool,
    scope: Option<ScopeId>,
    handler: Callback<()>,
}

impl CommandBuilder {
    /// Create a builder with required id.
    pub fn new(id: impl Into<CommandId>) -> Self {
        Self {
            id: id.into(),
            label: Arc::from(""),
            description: None,
            category: None,
            keybinding_hint: None,
            enabled: true,
            scope: None,
            handler: Callback::new(|_| {}),
        }
    }

    /// Set command label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }

    /// Set optional description.
    pub fn description(mut self, description: impl Into<Arc<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set optional category.
    pub fn category(mut self, category: impl Into<Arc<str>>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Set optional keybinding hint text.
    pub fn keybinding(mut self, hint: impl Into<Arc<str>>) -> Self {
        self.keybinding_hint = Some(hint.into());
        self
    }

    /// Set optional keybinding hint text directly.
    pub fn keybinding_hint_opt(mut self, hint: Option<Arc<str>>) -> Self {
        self.keybinding_hint = hint;
        self
    }

    /// Set keybinding hint derived from the configured keymap for an action.
    pub fn keybinding_from_keymap(mut self, keymap: &Keymap, action: Action) -> Self {
        self.keybinding_hint = keymap
            .binding_for_action(action)
            .map(|binding| Arc::<str>::from(binding.canonical_lowercase()));
        self
    }

    /// Set initial enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set command handler callback.
    pub fn handler(mut self, handler: Callback<()>) -> Self {
        self.handler = handler;
        self
    }

    /// Build the final command entry.
    pub fn build(self) -> CommandEntry {
        CommandEntry {
            id: self.id,
            label: self.label,
            description: self.description,
            category: self.category,
            keybinding_hint: self.keybinding_hint,
            enabled: self.enabled,
            scope: self.scope,
            handler: self.handler,
        }
    }
}

/// Shared runtime command registry.
#[derive(Clone, Default)]
pub struct CommandRegistry {
    entries: Rc<RefCell<HashMap<CommandId, CommandEntry>>>,
    generation: Rc<Cell<u64>>,
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command (replaces same id).
    pub fn register(&self, entry: CommandEntry) {
        self.entries.borrow_mut().insert(entry.id.clone(), entry);
        self.bump_generation();
    }

    pub(crate) fn register_for_scope(&self, scope: ScopeId, entry: CommandEntry) {
        self.register(CommandEntry {
            scope: Some(scope),
            ..entry
        });
    }

    /// Remove a command by id.
    pub fn unregister(&self, id: impl Into<CommandId>) -> Option<CommandEntry> {
        let removed = self.entries.borrow_mut().remove(&id.into());
        if removed.is_some() {
            self.bump_generation();
        }
        removed
    }

    /// Enable/disable a registered command.
    pub fn set_enabled(&self, id: impl Into<CommandId>, enabled: bool) -> bool {
        let id = id.into();
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.get_mut(&id) else {
            return false;
        };
        if entry.enabled != enabled {
            entry.enabled = enabled;
            self.bump_generation();
        }
        true
    }

    /// Execute a command by id when present and enabled.
    pub fn execute(&self, id: impl Into<CommandId>) -> bool {
        let id = id.into();
        let to_run = {
            let entries = self.entries.borrow();
            let Some(entry) = entries.get(&id) else {
                return false;
            };
            if !entry.enabled {
                return false;
            }
            entry.handler.clone()
        };
        to_run.emit(());
        true
    }

    /// Snapshot all registered entries.
    pub fn entries(&self) -> Vec<CommandEntry> {
        self.entries.borrow().values().cloned().collect()
    }

    pub(crate) fn unregister_scope(&self, scope: ScopeId) {
        let before = self.entries.borrow().len();
        self.entries
            .borrow_mut()
            .retain(|_, entry| entry.scope != Some(scope));
        if self.entries.borrow().len() != before {
            self.bump_generation();
        }
    }

    /// Monotonic generation used by listeners to detect updates.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn bump_generation(&self) {
        self.generation
            .set(self.generation.get().wrapping_add(1).max(1));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::{CommandBuilder, CommandRegistry};
    use crate::callback::Callback;

    #[test]
    fn register_replaces_existing_entry_by_id() {
        let registry = CommandRegistry::new();
        let hit_a = Rc::new(Cell::new(false));
        let hit_b = Rc::new(Cell::new(false));

        let hit_a_cb = Rc::clone(&hit_a);
        registry.register(
            CommandBuilder::new("app.test")
                .label("A")
                .handler(Callback::new(move |_| hit_a_cb.set(true)))
                .build(),
        );

        let hit_b_cb = Rc::clone(&hit_b);
        registry.register(
            CommandBuilder::new("app.test")
                .label("B")
                .handler(Callback::new(move |_| hit_b_cb.set(true)))
                .build(),
        );

        assert_eq!(registry.entries().len(), 1);
        assert!(registry.execute("app.test"));
        assert!(!hit_a.get());
        assert!(hit_b.get());
    }

    #[test]
    fn generation_increments_on_mutations() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.generation(), 0);

        registry.register(CommandBuilder::new("a").label("A").build());
        let g1 = registry.generation();
        assert!(g1 > 0);

        assert!(registry.set_enabled("a", false));
        let g2 = registry.generation();
        assert!(g2 > g1);

        assert!(registry.unregister("a").is_some());
        let g3 = registry.generation();
        assert!(g3 > g2);
    }

    #[test]
    fn execute_runs_handler_when_enabled() {
        let registry = CommandRegistry::new();
        let called = Rc::new(Cell::new(false));
        let called_cb = Rc::clone(&called);

        registry.register(
            CommandBuilder::new("run")
                .label("Run")
                .handler(Callback::new(move |_| called_cb.set(true)))
                .build(),
        );

        assert!(registry.execute("run"));
        assert!(called.get());
    }

    #[test]
    fn disabled_command_does_not_execute_until_enabled() {
        let registry = CommandRegistry::new();
        let called = Rc::new(Cell::new(false));
        let called_cb = Rc::clone(&called);

        registry.register(
            CommandBuilder::new("toggle")
                .label("Toggle")
                .enabled(false)
                .handler(Callback::new(move |_| called_cb.set(true)))
                .build(),
        );

        assert!(!registry.execute("toggle"));
        assert!(!called.get());

        assert!(registry.set_enabled("toggle", true));
        assert!(registry.execute("toggle"));
        assert!(called.get());
    }
}
