//! Runtime execution engine for declarative behaviors.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use engine_core::EngineResult;
use engine_ecs::Entity;

use crate::{
    ActionContext, BehaviorCompiler, BehaviorNode, BehaviorTree, ConditionContext, NodeResult,
};

/// Declarative script backend for runtime execution of behavior trees.
///
/// This is the main entry point for executing declarative behaviors.
/// It manages:
/// - Behavior tree compilation and caching
/// - Per-entity execution state
/// - Scene/input/physics integration
pub struct DeclarativeScriptBackend {
    /// Behavior compiler (handles loading and caching).
    compiler: BehaviorCompiler,

    /// Per-entity runtime state (for multi-frame actions like Wait, Patrol).
    entity_states: HashMap<(Entity, PathBuf), EntityExecutionState>,

    /// Shared input state for condition evaluation.
    input_state: Option<engine_platform::InputState>,

    /// Shared scene reference.
    scene: Option<engine_ecs::Scene>,

    /// Shared physics backend reference.
    physics_backend: Option<Box<dyn engine_physics::PhysicsBackend>>,

    /// Shared asset database reference.
    asset_database: Option<engine_assets::AssetDatabase>,
}

/// Runtime execution state for a single entity's behavior tree.
#[derive(Debug, Clone)]
struct EntityExecutionState {
    /// Current node being executed (for multi-frame actions).
    current_node_index: usize,

    /// Accumulated time for Wait actions.
    wait_timer: f32,

    /// Patrol state (current waypoint index).
    patrol_index: usize,

    /// Repeat node state (current iteration count).
    repeat_count: u32,

    /// Blackboard for inter-node shared state.
    blackboard: HashMap<String, BlackboardValue>,
}

/// Value types that can be stored in the blackboard.
#[derive(Debug, Clone)]
enum BlackboardValue {
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i32),
    /// Float value.
    Float(f32),
    /// String value.
    String(String),
    /// Entity reference.
    Entity(Entity),
    /// Vec3 position.
    Vec3([f32; 3]),
}

impl Default for EntityExecutionState {
    fn default() -> Self {
        Self {
            current_node_index: 0,
            wait_timer: 0.0,
            patrol_index: 0,
            repeat_count: 0,
            blackboard: HashMap::new(),
        }
    }
}

impl Default for DeclarativeScriptBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DeclarativeScriptBackend {
    /// Creates a new declarative script backend.
    pub fn new() -> Self {
        Self {
            compiler: BehaviorCompiler::new(),
            entity_states: HashMap::new(),
            input_state: None,
            scene: None,
            physics_backend: None,
            asset_database: None,
        }
    }

    /// Loads and compiles a behavior tree from a JSON file.
    pub fn load_behavior(&mut self, path: &Path) -> EngineResult<()> {
        self.compiler.compile_file(path)?;
        Ok(())
    }

    /// Compiles a behavior tree from an in-memory JSON string.
    pub fn compile_source(&mut self, logical_path: &Path, source: &str) -> EngineResult<()> {
        self.compiler.compile_source(logical_path, source)
    }

    /// Sets the input state for condition evaluation.
    pub fn set_input_state(&mut self, input: engine_platform::InputState) {
        self.input_state = Some(input);
    }

    /// Sets the scene reference.
    pub fn set_scene(&mut self, scene: engine_ecs::Scene) {
        self.scene = Some(scene);
    }

    /// Takes the scene back after execution.
    pub fn take_scene(&mut self) -> Option<engine_ecs::Scene> {
        self.scene.take()
    }

    /// Sets the physics backend reference.
    pub fn set_physics_backend(&mut self, backend: Box<dyn engine_physics::PhysicsBackend>) {
        self.physics_backend = Some(backend);
    }

    /// Takes the physics backend back.
    pub fn take_physics_backend(&mut self) -> Option<Box<dyn engine_physics::PhysicsBackend>> {
        self.physics_backend.take()
    }

    /// Sets the asset database reference.
    pub fn set_asset_database(&mut self, database: engine_assets::AssetDatabase) {
        self.asset_database = Some(database);
    }

    /// Takes the asset database back.
    pub fn take_asset_database(&mut self) -> Option<engine_assets::AssetDatabase> {
        self.asset_database.take()
    }

    /// Executes a behavior tree for an entity.
    ///
    /// This should be called once per frame for each entity with a declarative behavior.
    ///
    /// # Arguments
    /// * `entity` - The entity to execute for
    /// * `behavior_path` - Path to the compiled behavior tree
    /// * `delta_time` - Time since last frame in seconds
    ///
    /// # Returns
    /// The result of the behavior tree execution.
    pub fn execute(
        &mut self,
        entity: Entity,
        behavior_path: &Path,
        delta_time: f32,
    ) -> EngineResult<NodeResult> {
        // Get the compiled behavior tree
        let tree = self.compiler.compile_file(behavior_path)?.clone();

        // Get or create execution state for this entity
        let state_key = (entity, behavior_path.to_path_buf());
        let _state = self
            .entity_states
            .entry(state_key)
            .or_insert_with(EntityExecutionState::default);

        // Build execution contexts
        let input = self.input_state.as_ref().ok_or_else(|| {
            engine_core::EngineError::other("Input state not set on declarative backend")
        })?;

        let scene = self.scene.as_ref().ok_or_else(|| {
            engine_core::EngineError::other("Scene not set on declarative backend")
        })?;

        let condition_ctx = ConditionContext {
            entity,
            scene,
            input,
            physics: self.physics_backend.as_deref(),
            assets: self.asset_database.as_ref(),
            delta_time,
        };

        // Execute the tree (read-only for now)
        let result = self.evaluate_node(&tree.root, &condition_ctx);

        Ok(result)
    }

    /// Evaluates a behavior node recursively.
    fn evaluate_node(&self, node: &BehaviorNode, ctx: &ConditionContext) -> NodeResult {
        match node {
            BehaviorNode::Sequence { children, .. } => {
                for child in children {
                    match self.evaluate_node(child, ctx) {
                        NodeResult::Success => continue,
                        NodeResult::Failure => return NodeResult::Failure,
                        NodeResult::Running => return NodeResult::Running,
                    }
                }
                NodeResult::Success
            }

            BehaviorNode::Selector { children, .. } => {
                for child in children {
                    match self.evaluate_node(child, ctx) {
                        NodeResult::Success => return NodeResult::Success,
                        NodeResult::Failure => continue,
                        NodeResult::Running => return NodeResult::Running,
                    }
                }
                NodeResult::Failure
            }

            BehaviorNode::Parallel { children, .. } => {
                let mut any_running = false;
                for child in children {
                    match self.evaluate_node(child, ctx) {
                        NodeResult::Success => continue,
                        NodeResult::Failure => return NodeResult::Failure,
                        NodeResult::Running => any_running = true,
                    }
                }
                if any_running {
                    NodeResult::Running
                } else {
                    NodeResult::Success
                }
            }

            BehaviorNode::Condition { check } => {
                if check.evaluate(ctx) {
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }

            BehaviorNode::Action { action } => {
                // For read-only evaluation, we just return Success
                // Full execution with mutations requires mutable scene access
                NodeResult::Success
            }

            BehaviorNode::Invert { child } => match self.evaluate_node(child, ctx) {
                NodeResult::Success => NodeResult::Failure,
                NodeResult::Failure => NodeResult::Success,
                NodeResult::Running => NodeResult::Running,
            },

            BehaviorNode::Succeed { child } => {
                let _ = self.evaluate_node(child, ctx);
                NodeResult::Success
            }

            BehaviorNode::Repeat { child, count } => {
                // For now, just run once
                // Full implementation needs state tracking
                self.evaluate_node(child, ctx)
            }
        }
    }

    /// Executes a behavior tree with full mutation support.
    ///
    /// This version takes mutable scene access for actions that modify state.
    pub fn execute_mut(
        &mut self,
        entity: Entity,
        behavior_path: &Path,
        delta_time: f32,
    ) -> EngineResult<NodeResult> {
        // Get the compiled behavior tree
        let tree = self.compiler.compile_file(behavior_path)?.clone();

        // Get or create execution state for this entity
        let state_key = (entity, behavior_path.to_path_buf());
        let entity_state = self
            .entity_states
            .entry(state_key)
            .or_insert_with(EntityExecutionState::default);

        // Extract state values to avoid borrow conflicts
        let mut wait_timer = entity_state.wait_timer;
        let mut patrol_index = entity_state.patrol_index;
        let mut repeat_count = entity_state.repeat_count;

        // Build mutable action context
        let input = self
            .input_state
            .as_ref()
            .ok_or_else(|| engine_core::EngineError::other("Input state not set"))?
            .clone();

        let mut scene = self
            .scene
            .take()
            .ok_or_else(|| engine_core::EngineError::other("Scene not set"))?;

        let assets = self.asset_database.as_ref();

        // Create execution state for actions
        let mut exec_state = crate::action::ExecutionState {
            wait_timer,
            patrol_index,
        };

        // Simple approach: don't support physics mutations in this version
        let mut action_ctx = crate::action::ActionContext {
            entity,
            scene: &mut scene,
            input: &input,
            physics: None, // Simplified: no physics mutations for now
            assets,
            delta_time,
            execution_state: &mut exec_state,
        };

        // Execute with mutations
        let result = Self::execute_node_mut_static(&tree.root, &mut action_ctx, &mut repeat_count);

        // Save execution state back
        wait_timer = exec_state.wait_timer;
        patrol_index = exec_state.patrol_index;

        // Restore state
        self.scene = Some(scene);

        // Update entity state
        let state_key = (entity, behavior_path.to_path_buf());
        if let Some(state) = self.entity_states.get_mut(&state_key) {
            state.wait_timer = wait_timer;
            state.patrol_index = patrol_index;
            state.repeat_count = repeat_count;
        }

        Ok(result)
    }

    /// Executes a node with mutation support (static to avoid borrow conflicts).
    fn execute_node_mut_static(
        node: &BehaviorNode,
        ctx: &mut crate::action::ActionContext,
        repeat_count: &mut u32,
    ) -> NodeResult {
        match node {
            BehaviorNode::Sequence { children, .. } => {
                for child in children {
                    match Self::execute_node_mut_static(child, ctx, repeat_count) {
                        NodeResult::Success => continue,
                        result => return result,
                    }
                }
                NodeResult::Success
            }

            BehaviorNode::Selector { children, .. } => {
                for child in children {
                    match Self::execute_node_mut_static(child, ctx, repeat_count) {
                        NodeResult::Failure => continue,
                        result => return result,
                    }
                }
                NodeResult::Failure
            }

            BehaviorNode::Parallel { children, .. } => {
                let mut any_running = false;
                for child in children {
                    match Self::execute_node_mut_static(child, ctx, repeat_count) {
                        NodeResult::Failure => return NodeResult::Failure,
                        NodeResult::Running => any_running = true,
                        NodeResult::Success => continue,
                    }
                }
                if any_running {
                    NodeResult::Running
                } else {
                    NodeResult::Success
                }
            }

            BehaviorNode::Condition { check } => {
                // Create a read-only context for condition evaluation
                let condition_ctx = ConditionContext {
                    entity: ctx.entity,
                    scene: ctx.scene,
                    input: ctx.input,
                    physics: ctx.physics.as_deref(),
                    assets: ctx.assets,
                    delta_time: ctx.delta_time,
                };

                if check.evaluate(&condition_ctx) {
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }

            BehaviorNode::Action { action } => action.execute(ctx),

            BehaviorNode::Invert { child } => {
                match Self::execute_node_mut_static(child, ctx, repeat_count) {
                    NodeResult::Success => NodeResult::Failure,
                    NodeResult::Failure => NodeResult::Success,
                    NodeResult::Running => NodeResult::Running,
                }
            }

            BehaviorNode::Succeed { child } => {
                let _ = Self::execute_node_mut_static(child, ctx, repeat_count);
                NodeResult::Success
            }

            BehaviorNode::Repeat { child, count } => {
                // Handle repeat count
                if let Some(max_count) = count {
                    if *repeat_count >= *max_count {
                        // Reset and complete
                        *repeat_count = 0;
                        return NodeResult::Success;
                    }
                }

                // Execute child
                match Self::execute_node_mut_static(child, ctx, repeat_count) {
                    NodeResult::Success => {
                        *repeat_count += 1;
                        if let Some(max_count) = count {
                            if *repeat_count >= *max_count {
                                *repeat_count = 0;
                                NodeResult::Success
                            } else {
                                NodeResult::Running
                            }
                        } else {
                            // Infinite repeat
                            NodeResult::Running
                        }
                    }
                    NodeResult::Failure => {
                        *repeat_count = 0;
                        NodeResult::Failure
                    }
                    NodeResult::Running => NodeResult::Running,
                }
            }
        }
    }

    /// Returns the number of cached behavior trees.
    pub fn cache_size(&self) -> usize {
        self.compiler.cache_size()
    }

    /// Clears all cached behavior trees.
    pub fn clear_cache(&mut self) {
        self.compiler.clear_cache();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionExpr, BehaviorNode, BehaviorSchema, ConditionExpr};
    use engine_ecs::Scene;
    use engine_platform::InputState;

    #[test]
    fn backend_initializes() {
        let backend = DeclarativeScriptBackend::new();
        assert_eq!(backend.cache_size(), 0);
    }

    #[test]
    fn backend_compiles_and_caches() {
        let mut backend = DeclarativeScriptBackend::new();
        let path = PathBuf::from("test.json");

        let json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"}
            ]
        }"#;

        backend.compile_source(&path, json).unwrap();
        assert_eq!(backend.cache_size(), 1);
    }

    #[test]
    fn backend_requires_input_and_scene() {
        let mut backend = DeclarativeScriptBackend::new();
        let path = PathBuf::from("test.json");

        let json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"}
            ]
        }"#;

        backend.compile_source(&path, json).unwrap();

        let mut scene = Scene::new();
        let entity = scene.create_object("Test").unwrap();

        // Should fail without input/scene set
        let result = backend.execute(entity, &path, 0.016);
        assert!(result.is_err());

        // Set input and scene
        backend.set_input_state(InputState::default());
        backend.set_scene(scene);

        // Now should succeed
        let result = backend.execute(entity, &path, 0.016);
        assert!(result.is_ok());
    }
}
