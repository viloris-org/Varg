//! Context-sensitive action maps with priority ordering.

use std::collections::HashMap;

/// A named input context with priority.
///
/// Multiple contexts can be active simultaneously. When the same action
/// is defined in multiple contexts, the highest-priority context wins.
#[derive(Clone, Debug)]
pub struct ActionContext {
    /// Context name.
    pub name: String,
    /// Priority (higher = takes precedence).
    pub priority: i32,
    /// Whether this context is currently enabled.
    pub enabled: bool,
    /// Action name to input binding mappings.
    pub actions: HashMap<String, super::input_map::InputBinding>,
}

impl ActionContext {
    /// Creates a new action context.
    pub fn new(name: impl Into<String>, priority: i32) -> Self {
        Self {
            name: name.into(),
            priority,
            enabled: true,
            actions: HashMap::new(),
        }
    }

    /// Binds an action to a key.
    pub fn bind_key(
        &mut self,
        action: impl Into<String>,
        key: crate::input::KeyCode,
    ) {
        let entry = self.actions.entry(action.into()).or_default();
        entry.positive_keys.push(key);
    }

    /// Binds an action to a mouse button.
    pub fn bind_mouse(
        &mut self,
        action: impl Into<String>,
        button: crate::input::MouseButton,
    ) {
        let entry = self.actions.entry(action.into()).or_default();
        entry.positive_mouse.push(button);
    }
}

/// Manages a stack of action contexts, resolving input by priority.
#[derive(Clone, Debug, Default)]
pub struct ActionContextManager {
    contexts: Vec<ActionContext>,
}

impl ActionContextManager {
    /// Pushes a context onto the stack. Higher priority contexts take precedence.
    pub fn push_context(&mut self, context: ActionContext) {
        self.contexts.push(context);
        self.contexts
            .sort_by_key(|ctx| std::cmp::Reverse(ctx.priority));
    }

    /// Removes a context by name.
    pub fn remove_context(&mut self, name: &str) {
        self.contexts.retain(|ctx| ctx.name != name);
    }

    /// Enables or disables a context by name.
    pub fn set_context_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(ctx) = self.contexts.iter_mut().find(|c| c.name == name) {
            ctx.enabled = enabled;
        }
    }

    /// Evaluates all enabled contexts and returns resolved action values.
    ///
    /// When the same action is defined in multiple contexts, the highest
    /// priority context wins.
    pub fn evaluate(
        &self,
        input: &crate::input::InputState,
    ) -> HashMap<String, f32> {
        let mut resolved: HashMap<String, f32> = HashMap::new();
        let mut seen: HashMap<String, i32> = HashMap::new();

        for ctx in &self.contexts {
            if !ctx.enabled {
                continue;
            }
            for (name, binding) in &ctx.actions {
                let entry = seen.entry(name.clone()).or_insert(ctx.priority);
                if ctx.priority >= *entry {
                    *entry = ctx.priority;
                    let value = evaluate_single(binding, input);
                    resolved.insert(name.clone(), value);
                }
            }
        }

        resolved
    }

    /// Returns whether a specific context exists.
    pub fn has_context(&self, name: &str) -> bool {
        self.contexts.iter().any(|ctx| ctx.name == name)
    }

    /// Returns the number of registered contexts.
    pub fn context_count(&self) -> usize {
        self.contexts.len()
    }
}

fn evaluate_single(
    binding: &super::input_map::InputBinding,
    input: &crate::input::InputState,
) -> f32 {
    let deadzone = binding.dead_zone.unwrap_or_default();

    let positive = binding
        .positive_keys
        .iter()
        .any(|k| input.key_down(*k));
    let negative = binding
        .negative_keys
        .iter()
        .any(|k| input.key_down(*k));

    let raw: f32 = match (negative, positive) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => 0.0,
    };

    if !binding.chord_keys.is_empty()
        && !binding
            .chord_keys
            .iter()
            .all(|k| input.key_down(*k))
    {
        return 0.0;
    }

    let abs = raw.abs();
    if abs <= deadzone.inner {
        return 0.0;
    }
    raw.signum()
}
