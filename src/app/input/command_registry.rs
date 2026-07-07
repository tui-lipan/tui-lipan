//! Runtime command registry used by command palette style UIs.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::input::key_dispatch::CommandConflictPolicy;
use crate::app::input::keymap::{Action, Keymap};
use crate::callback::Callback;
use crate::callback::ScopeId;
use crate::core::event::KeyEvent;
use crate::input::{ChordMatcher, ChordResult, KeyBinding, KeyBindings};

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
    /// Executable keyboard shortcuts for this command.
    pub shortcuts: KeyBindings,
    /// Relative priority when resolving shortcut conflicts.
    pub priority: i32,
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
            .field("shortcuts", &self.shortcuts)
            .field("priority", &self.priority)
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
    shortcut_bindings: Vec<KeyBinding>,
    priority: i32,
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
            shortcut_bindings: Vec::new(),
            priority: 0,
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

    /// Set optional keybinding hint text for display only.
    pub fn keybinding_hint(mut self, hint: impl Into<Arc<str>>) -> Self {
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

    /// Add an executable keyboard shortcut.
    pub fn shortcut(mut self, binding: KeyBinding) -> Self {
        self.shortcut_bindings.push(binding);
        self
    }

    /// Add multiple executable keyboard shortcuts.
    pub fn shortcuts(mut self, bindings: KeyBindings) -> Self {
        self.shortcut_bindings.extend(bindings.iter().cloned());
        self
    }

    /// Set relative shortcut conflict priority.
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
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
            shortcuts: KeyBindings::from_bindings(self.shortcut_bindings),
            priority: self.priority,
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
    order: Rc<RefCell<Vec<CommandId>>>,
    generation: Rc<Cell<u64>>,
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command (replaces same id while preserving registration order).
    pub fn register(&self, entry: CommandEntry) {
        let id = entry.id.clone();
        let mut entries = self.entries.borrow_mut();
        let mut order = self.order.borrow_mut();
        let replacing = entries.contains_key(&id);
        entries.insert(id.clone(), entry);
        if !replacing {
            order.push(id);
        }
        drop(entries);
        drop(order);
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
        let id = id.into();
        let removed = self.entries.borrow_mut().remove(&id);
        if removed.is_some() {
            self.order.borrow_mut().retain(|existing| existing != &id);
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

    /// Resolve enabled commands whose shortcuts match a single key event.
    pub fn matching_enabled_shortcuts(
        &self,
        key: KeyEvent,
        policy: CommandConflictPolicy,
    ) -> Vec<CommandId> {
        let entries = self.entries.borrow();
        let order = self.order.borrow();
        let mut matches = Vec::new();

        for id in order.iter() {
            let Some(entry) = entries.get(id) else {
                continue;
            };
            if !entry.enabled {
                continue;
            }
            if entry
                .shortcuts
                .iter()
                .any(|binding| binding.step_count() == 1 && binding.matches_sequence(&[key]))
            {
                matches.push(id.clone());
            }
        }

        resolve_shortcut_conflicts(matches, &entries, policy)
    }

    pub(crate) fn shortcut_entries(&self) -> Vec<(KeyBinding, CommandId)> {
        let entries = self.entries.borrow();
        let order = self.order.borrow();
        let mut out = Vec::new();
        for id in order.iter() {
            let Some(entry) = entries.get(id) else {
                continue;
            };
            if !entry.enabled {
                continue;
            }
            for binding in entry.shortcuts.iter().cloned() {
                out.push((binding, id.clone()));
            }
        }
        out
    }

    pub(crate) fn ordered_entries(&self) -> Vec<CommandEntry> {
        let entries = self.entries.borrow();
        self.order
            .borrow()
            .iter()
            .filter_map(|id| entries.get(id).cloned())
            .collect()
    }

    /// Snapshot all registered entries.
    pub fn entries(&self) -> Vec<CommandEntry> {
        self.ordered_entries()
    }

    pub(crate) fn unregister_scope(&self, scope: ScopeId) {
        let before = self.entries.borrow().len();
        self.entries
            .borrow_mut()
            .retain(|_, entry| entry.scope != Some(scope));
        self.order
            .borrow_mut()
            .retain(|id| self.entries.borrow().contains_key(id));
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

fn resolve_shortcut_conflicts(
    matches: Vec<CommandId>,
    entries: &HashMap<CommandId, CommandEntry>,
    policy: CommandConflictPolicy,
) -> Vec<CommandId> {
    if matches.len() <= 1 {
        return matches;
    }

    match policy {
        CommandConflictPolicy::FirstRegistered => vec![matches[0].clone()],
        CommandConflictPolicy::HighestPriority => {
            let winner = matches.into_iter().max_by(|left, right| {
                let left_priority = entries.get(left).map(|entry| entry.priority).unwrap_or(0);
                let right_priority = entries.get(right).map(|entry| entry.priority).unwrap_or(0);
                left_priority.cmp(&right_priority)
            });
            winner.into_iter().collect()
        }
    }
}

/// Result of feeding a key into [`CommandShortcutRuntime`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandShortcutResult {
    None,
    Pending,
    Matched(CommandId),
    Mismatch,
}

/// Stateful matcher for executable app command shortcuts.
pub(crate) struct CommandShortcutRuntime {
    matcher: ChordMatcher<CommandId>,
    registry_generation: u64,
    conflict_policy: CommandConflictPolicy,
}

impl CommandShortcutRuntime {
    pub(crate) fn new(registry: &CommandRegistry, conflict_policy: CommandConflictPolicy) -> Self {
        Self {
            matcher: ChordMatcher::new(registry.shortcut_entries()),
            registry_generation: registry.generation(),
            conflict_policy,
        }
    }

    pub(crate) fn sync_registry(&mut self, registry: &CommandRegistry) {
        let generation = registry.generation();
        if self.registry_generation == generation {
            return;
        }
        self.matcher = ChordMatcher::new(registry.shortcut_entries());
        self.registry_generation = generation;
        self.matcher.reset();
    }

    pub(crate) fn reset(&mut self) {
        self.matcher.reset();
    }

    pub(crate) fn feed(
        &mut self,
        key: KeyEvent,
        registry: &CommandRegistry,
    ) -> CommandShortcutResult {
        self.sync_registry(registry);
        let was_pending = self.matcher.is_pending();
        match self.matcher.feed(&key) {
            ChordResult::None => {
                if was_pending {
                    CommandShortcutResult::Mismatch
                } else {
                    CommandShortcutResult::None
                }
            }
            ChordResult::Pending => CommandShortcutResult::Pending,
            ChordResult::Matched(id) => {
                if !was_pending {
                    let resolved = registry.matching_enabled_shortcuts(key, self.conflict_policy);
                    return resolved
                        .into_iter()
                        .next()
                        .map(CommandShortcutResult::Matched)
                        .unwrap_or(CommandShortcutResult::None);
                }
                let resolved = resolve_shortcut_conflicts(
                    vec![(*id).clone()],
                    &registry.entries.borrow(),
                    self.conflict_policy,
                );
                if let Some(winner) = resolved.into_iter().next() {
                    CommandShortcutResult::Matched(winner)
                } else {
                    CommandShortcutResult::None
                }
            }
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.matcher.is_pending()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::str::FromStr;

    use super::{
        CommandBuilder, CommandId, CommandRegistry, CommandShortcutResult, CommandShortcutRuntime,
    };
    use crate::app::input::key_dispatch::CommandConflictPolicy;
    use crate::callback::Callback;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::input::KeyBinding;

    fn ctrl_key_for_test(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn key_event_for_test(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            mods: KeyMods::default(),
        }
    }

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

    #[test]
    fn command_shortcut_conflict_is_stable_first_registered_by_default() {
        let registry = CommandRegistry::new();
        registry.register(
            CommandBuilder::new("first")
                .shortcut(KeyBinding::from_str("ctrl-k").unwrap())
                .build(),
        );
        registry.register(
            CommandBuilder::new("second")
                .shortcut(KeyBinding::from_str("ctrl-k").unwrap())
                .build(),
        );

        let matches = registry.matching_enabled_shortcuts(
            ctrl_key_for_test('k'),
            CommandConflictPolicy::FirstRegistered,
        );
        assert_eq!(matches, vec![CommandId::from("first")]);
    }

    #[test]
    fn command_shortcut_conflict_can_use_highest_priority() {
        let registry = CommandRegistry::new();
        registry.register(
            CommandBuilder::new("low")
                .priority(0)
                .shortcut(KeyBinding::from_str("ctrl-k").unwrap())
                .build(),
        );
        registry.register(
            CommandBuilder::new("high")
                .priority(10)
                .shortcut(KeyBinding::from_str("ctrl-k").unwrap())
                .build(),
        );

        let matches = registry.matching_enabled_shortcuts(
            ctrl_key_for_test('k'),
            CommandConflictPolicy::HighestPriority,
        );
        assert_eq!(matches, vec![CommandId::from("high")]);
    }

    #[test]
    fn command_shortcut_runtime_supports_chords_and_resets_on_generation_change() {
        let registry = CommandRegistry::new();
        registry.register(
            CommandBuilder::new("mux.detach")
                .shortcut(KeyBinding::from_str("ctrl-a d").unwrap())
                .build(),
        );
        let mut runtime =
            CommandShortcutRuntime::new(&registry, CommandConflictPolicy::FirstRegistered);
        assert!(matches!(
            runtime.feed(ctrl_key_for_test('a'), &registry),
            CommandShortcutResult::Pending
        ));
        registry.unregister("mux.detach");
        assert!(matches!(
            runtime.feed(key_event_for_test('d'), &registry),
            CommandShortcutResult::None
        ));
    }
}
