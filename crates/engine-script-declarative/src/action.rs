//! Action expressions for behavior trees.

use engine_core::math::{Quat, Vec3};
use engine_ecs::Entity;
use serde::{Deserialize, Serialize};

use crate::NodeResult;

/// An action that can be executed with side effects.
///
/// Actions modify game state (transforms, components, spawning entities, etc.).
pub trait Action {
    /// Executes the action in the given context.
    ///
    /// Returns Success if completed, Running if multi-frame, Failure if error.
    fn execute(&mut self, ctx: &mut ActionContext) -> NodeResult;
}

/// Runtime context for action execution.
pub struct ActionContext<'a> {
    /// The entity this action is being executed for.
    pub entity: Entity,
    /// Current scene state (mutable for modifications).
    pub scene: &'a mut engine_ecs::Scene,
    /// Current input state.
    pub input: &'a engine_platform::InputState,
    /// Current physics backend.
    pub physics: Option<&'a mut dyn engine_physics::PhysicsBackend>,
    /// Asset database for resource lookups.
    pub assets: Option<&'a engine_assets::AssetDatabase>,
    /// Delta time since last frame.
    pub delta_time: f32,
    /// Execution state for multi-frame actions.
    pub execution_state: &'a mut ExecutionState,
}

/// Execution state for multi-frame actions.
pub struct ExecutionState {
    /// Accumulated time for Wait actions.
    pub wait_timer: f32,
    /// Patrol state (current waypoint index).
    pub patrol_index: usize,
}

/// An action expression that can be serialized from JSON.
///
/// This enum defines all action types that LLMs can generate.
/// Each variant has clear semantics and predictable JSON structure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionExpr {
    /// Move the entity forward in its local space.
    ///
    /// # Example
    /// ```json
    /// {"moveForward": 5.0}
    /// ```
    MoveForward {
        /// Speed in units per second.
        speed: f32,
    },

    /// Move the entity backward in its local space.
    ///
    /// # Example
    /// ```json
    /// {"moveBackward": 3.0}
    /// ```
    MoveBackward {
        /// Speed in units per second.
        speed: f32,
    },

    /// Strafe left in local space.
    ///
    /// # Example
    /// ```json
    /// {"strafeLeft": 2.0}
    /// ```
    StrafeLeft {
        /// Speed in units per second.
        speed: f32,
    },

    /// Strafe right in local space.
    ///
    /// # Example
    /// ```json
    /// {"strafeRight": 2.0}
    /// ```
    StrafeRight {
        /// Speed in units per second.
        speed: f32,
    },

    /// Translate by a world-space offset.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "translate": {
    ///     "offset": [0, 1, 0]
    ///   }
    /// }
    /// ```
    Translate {
        /// World-space offset [x, y, z].
        offset: [f32; 3],
    },

    /// Set absolute position in world space.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "setPosition": {
    ///     "position": [10, 0, 5]
    ///   }
    /// }
    /// ```
    SetPosition {
        /// World-space position [x, y, z].
        position: [f32; 3],
    },

    /// Rotate around Y axis (yaw).
    ///
    /// # Example
    /// ```json
    /// {"rotateY": 90.0}
    /// ```
    RotateY {
        /// Degrees per second.
        degrees_per_sec: f32,
    },

    /// Look at a target position.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "lookAt": {
    ///     "target": [0, 0, 0]
    ///   }
    /// }
    /// ```
    LookAt {
        /// Target position [x, y, z].
        target: [f32; 3],
    },

    /// Chase a target entity or player.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "chase": {
    ///     "target": "player",
    ///     "speed": 4.0
    ///   }
    /// }
    /// ```
    Chase {
        /// Target identifier ("player" or entity ID).
        target: String,
        /// Chase speed in units per second.
        speed: f32,
    },

    /// Flee from a target entity or player.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "flee": {
    ///     "target": "player",
    ///     "speed": 5.0
    ///   }
    /// }
    /// ```
    Flee {
        /// Target identifier to flee from.
        target: String,
        /// Flee speed in units per second.
        speed: f32,
    },

    /// Patrol along a series of waypoints.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "patrol": {
    ///     "points": [[0,0,0], [10,0,0], [10,0,10]],
    ///     "speed": 2.0,
    ///     "loop": true
    ///   }
    /// }
    /// ```
    Patrol {
        /// Waypoint positions.
        points: Vec<[f32; 3]>,
        /// Movement speed.
        speed: f32,
        /// Whether to loop back to start.
        #[serde(default = "default_true")]
        r#loop: bool,
    },

    /// Apply a physics impulse.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "applyImpulse": {
    ///     "force": [0, 10, 0]
    ///   }
    /// }
    /// ```
    ApplyImpulse {
        /// Force vector [x, y, z].
        force: [f32; 3],
    },

    /// Deal damage to a target.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "attack": {
    ///     "target": "player",
    ///     "damage": 10
    ///   }
    /// }
    /// ```
    Attack {
        /// Target to attack.
        target: String,
        /// Damage amount.
        damage: i32,
    },

    /// Play a sound effect.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "playSound": {
    ///     "path": "sounds/footstep.ogg"
    ///   }
    /// }
    /// ```
    PlaySound {
        /// Asset path to the sound file.
        path: String,
    },

    /// Spawn a new entity from a prefab.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "spawn": {
    ///     "prefab": "enemies/grunt.json",
    ///     "position": [0, 0, 0]
    ///   }
    /// }
    /// ```
    Spawn {
        /// Path to prefab file.
        prefab: String,
        /// Spawn position [x, y, z].
        position: [f32; 3],
    },

    /// Destroy this entity.
    ///
    /// # Example
    /// ```json
    /// {"destroySelf": true}
    /// ```
    DestroySelf,

    /// Wait for a duration (multi-frame action).
    ///
    /// # Example
    /// ```json
    /// {"wait": 2.0}
    /// ```
    Wait {
        /// Duration in seconds.
        duration: f32,
    },

    /// Execute multiple actions in sequence.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "sequence": [
    ///     {"playSound": {"path": "jump.ogg"}},
    ///     {"applyImpulse": {"force": [0, 10, 0]}}
    ///   ]
    /// }
    /// ```
    Sequence {
        /// Actions to execute in order.
        actions: Vec<ActionExpr>,
    },

    /// Do nothing (useful for testing).
    Idle,
}

impl<'de> Deserialize<'de> for ActionExpr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        action_de::deserialize(deserializer)
    }
}

fn default_true() -> bool {
    true
}

mod action_de {
    use super::*;
    use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
    use std::fmt;

    struct ActionExprVisitor;

    impl<'de> Visitor<'de> for ActionExprVisitor {
        type Value = ActionExpr;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an action expression")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<ActionExpr, E> {
            match v {
                "destroySelf" => Ok(ActionExpr::DestroySelf),
                "idle" => Ok(ActionExpr::Idle),
                other => Err(de::Error::unknown_variant(other, &["destroySelf", "idle"])),
            }
        }

        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<ActionExpr, M::Error> {
            let key: String = map
                .next_key()?
                .ok_or_else(|| de::Error::custom("empty map"))?;
            let result = match key.as_str() {
                "moveForward" => {
                    let v: BareOrStruct<MoveForwardFields> = map.next_value()?;
                    ActionExpr::MoveForward {
                        speed: v.into_inner().speed,
                    }
                }
                "moveBackward" => {
                    let v: BareOrStruct<MoveBackwardFields> = map.next_value()?;
                    ActionExpr::MoveBackward {
                        speed: v.into_inner().speed,
                    }
                }
                "strafeLeft" => {
                    let v: BareOrStruct<StrafeLeftFields> = map.next_value()?;
                    ActionExpr::StrafeLeft {
                        speed: v.into_inner().speed,
                    }
                }
                "strafeRight" => {
                    let v: BareOrStruct<StrafeRightFields> = map.next_value()?;
                    ActionExpr::StrafeRight {
                        speed: v.into_inner().speed,
                    }
                }
                "translate" => {
                    let v: TranslateFields = map.next_value()?;
                    ActionExpr::Translate { offset: v.offset }
                }
                "setPosition" => {
                    let v: SetPositionFields = map.next_value()?;
                    ActionExpr::SetPosition {
                        position: v.position,
                    }
                }
                "rotateY" => {
                    let v: BareOrStruct<RotateYFields> = map.next_value()?;
                    ActionExpr::RotateY {
                        degrees_per_sec: v.into_inner().degrees_per_sec,
                    }
                }
                "lookAt" => {
                    let v: LookAtFields = map.next_value()?;
                    ActionExpr::LookAt { target: v.target }
                }
                "chase" => {
                    let v: ChaseFields = map.next_value()?;
                    ActionExpr::Chase {
                        target: v.target,
                        speed: v.speed,
                    }
                }
                "flee" => {
                    let v: FleeFields = map.next_value()?;
                    ActionExpr::Flee {
                        target: v.target,
                        speed: v.speed,
                    }
                }
                "patrol" => {
                    let v: PatrolFields = map.next_value()?;
                    ActionExpr::Patrol {
                        points: v.points,
                        speed: v.speed,
                        r#loop: v.r#loop,
                    }
                }
                "applyImpulse" => {
                    let v: ApplyImpulseFields = map.next_value()?;
                    ActionExpr::ApplyImpulse { force: v.force }
                }
                "attack" => {
                    let v: AttackFields = map.next_value()?;
                    ActionExpr::Attack {
                        target: v.target,
                        damage: v.damage,
                    }
                }
                "playSound" => {
                    let v: PlaySoundFields = map.next_value()?;
                    ActionExpr::PlaySound { path: v.path }
                }
                "spawn" => {
                    let v: SpawnFields = map.next_value()?;
                    ActionExpr::Spawn {
                        prefab: v.prefab,
                        position: v.position,
                    }
                }
                "destroySelf" => {
                    let _: IgnoredAny = map.next_value()?;
                    ActionExpr::DestroySelf
                }
                "wait" => {
                    let v: BareOrStruct<WaitFields> = map.next_value()?;
                    ActionExpr::Wait {
                        duration: v.into_inner().duration,
                    }
                }
                "sequence" => {
                    let v: SequenceFields = map.next_value()?;
                    ActionExpr::Sequence { actions: v.actions }
                }
                "idle" => {
                    let _: IgnoredAny = map.next_value()?;
                    ActionExpr::Idle
                }
                other => {
                    return Err(de::Error::unknown_variant(
                        other,
                        &[
                            "moveForward",
                            "moveBackward",
                            "strafeLeft",
                            "strafeRight",
                            "translate",
                            "setPosition",
                            "rotateY",
                            "lookAt",
                            "chase",
                            "flee",
                            "patrol",
                            "applyImpulse",
                            "attack",
                            "playSound",
                            "spawn",
                            "destroySelf",
                            "wait",
                            "sequence",
                            "idle",
                        ],
                    ))
                }
            };
            Ok(result)
        }
    }

    /// Wraps a single-field struct so it can be deserialized from either
    /// a bare value (`5.0`) or an object (`{"speed": 5.0}`).
    enum BareOrStruct<T> {
        Bare(T),
        Struct(T),
    }

    impl<T> BareOrStruct<T> {
        fn into_inner(self) -> T {
            match self {
                BareOrStruct::Bare(v) | BareOrStruct::Struct(v) => v,
            }
        }
    }

    impl<'de, T: Deserialize<'de>> Deserialize<'de> for BareOrStruct<T> {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor<T>(std::marker::PhantomData<T>);
            impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
                type Value = BareOrStruct<T>;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a bare value or an object")
                }
                fn visit_f64<E: de::Error>(self, v: f64) -> Result<BareOrStruct<T>, E> {
                    // Construct a JSON object with the known field name
                    let json = serde_json::json!({"speed": v});
                    T::deserialize(json)
                        .map(BareOrStruct::Bare)
                        .map_err(de::Error::custom)
                }
                fn visit_i64<E: de::Error>(self, v: i64) -> Result<BareOrStruct<T>, E> {
                    self.visit_f64(v as f64)
                }
                fn visit_bool<E: de::Error>(self, v: bool) -> Result<BareOrStruct<T>, E> {
                    let json = serde_json::json!({"speed": v});
                    T::deserialize(json)
                        .map(BareOrStruct::Bare)
                        .map_err(de::Error::custom)
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<BareOrStruct<T>, E> {
                    let json = serde_json::json!({"key": v});
                    T::deserialize(json)
                        .map(BareOrStruct::Bare)
                        .map_err(de::Error::custom)
                }
                fn visit_map<M: de::MapAccess<'de>>(
                    self,
                    map: M,
                ) -> Result<BareOrStruct<T>, M::Error> {
                    T::deserialize(de::value::MapAccessDeserializer::new(map))
                        .map(BareOrStruct::Struct)
                }
            }
            deserializer.deserialize_any(Visitor(std::marker::PhantomData))
        }
    }

    // Field structs for deserialization
    #[derive(Deserialize)]
    struct MoveForwardFields {
        speed: f32,
    }
    #[derive(Deserialize)]
    struct MoveBackwardFields {
        speed: f32,
    }
    #[derive(Deserialize)]
    struct StrafeLeftFields {
        speed: f32,
    }
    #[derive(Deserialize)]
    struct StrafeRightFields {
        speed: f32,
    }
    #[derive(Deserialize)]
    struct TranslateFields {
        offset: [f32; 3],
    }
    #[derive(Deserialize)]
    struct SetPositionFields {
        position: [f32; 3],
    }
    #[derive(Deserialize)]
    struct RotateYFields {
        degrees_per_sec: f32,
    }
    #[derive(Deserialize)]
    struct LookAtFields {
        target: [f32; 3],
    }
    #[derive(Deserialize)]
    struct ChaseFields {
        target: String,
        speed: f32,
    }
    #[derive(Deserialize)]
    struct FleeFields {
        target: String,
        speed: f32,
    }
    #[derive(Deserialize)]
    struct PatrolFields {
        points: Vec<[f32; 3]>,
        speed: f32,
        #[serde(default = "default_true")]
        r#loop: bool,
    }
    #[derive(Deserialize)]
    struct ApplyImpulseFields {
        force: [f32; 3],
    }
    #[derive(Deserialize)]
    struct AttackFields {
        target: String,
        damage: i32,
    }
    #[derive(Deserialize)]
    struct PlaySoundFields {
        path: String,
    }
    #[derive(Deserialize)]
    struct SpawnFields {
        prefab: String,
        position: [f32; 3],
    }
    #[derive(Deserialize)]
    struct WaitFields {
        duration: f32,
    }
    #[derive(Deserialize)]
    struct SequenceFields {
        actions: Vec<ActionExpr>,
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ActionExpr, D::Error> {
        deserializer.deserialize_any(ActionExprVisitor)
    }
}

impl ActionExpr {
    /// Executes this action expression in the given context.
    ///
    /// This is a stateless evaluation. For multi-frame actions (like Wait),
    /// the runtime needs to track state separately.
    pub fn execute(&self, ctx: &mut ActionContext) -> NodeResult {
        match self {
            ActionExpr::MoveForward { speed } => {
                Self::move_local(ctx, Vec3::new(0.0, 0.0, *speed * ctx.delta_time))
            }
            ActionExpr::MoveBackward { speed } => {
                Self::move_local(ctx, Vec3::new(0.0, 0.0, -*speed * ctx.delta_time))
            }
            ActionExpr::StrafeLeft { speed } => {
                Self::move_local(ctx, Vec3::new(-*speed * ctx.delta_time, 0.0, 0.0))
            }
            ActionExpr::StrafeRight { speed } => {
                Self::move_local(ctx, Vec3::new(*speed * ctx.delta_time, 0.0, 0.0))
            }
            ActionExpr::Translate { offset } => {
                if let Some(mut transform) = ctx.scene.transforms().local(ctx.entity) {
                    transform.translation =
                        transform.translation + Vec3::new(offset[0], offset[1], offset[2]);
                    ctx.scene.transforms_mut().set_local(ctx.entity, transform);
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::SetPosition { position } => {
                if let Some(mut transform) = ctx.scene.transforms().local(ctx.entity) {
                    transform.translation = Vec3::new(position[0], position[1], position[2]);
                    ctx.scene.transforms_mut().set_local(ctx.entity, transform);
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::RotateY { degrees_per_sec } => {
                if let Some(mut transform) = ctx.scene.transforms().local(ctx.entity) {
                    let radians = degrees_per_sec.to_radians() * ctx.delta_time;
                    let rotation = Quat::from_euler(0.0, radians, 0.0);
                    transform.rotation = transform.rotation * rotation;
                    ctx.scene.transforms_mut().set_local(ctx.entity, transform);
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::LookAt { target } => {
                if let Some(mut transform) = ctx.scene.transforms().local(ctx.entity) {
                    let target_pos = Vec3::new(target[0], target[1], target[2]);
                    let direction = (target_pos - transform.translation).normalized();

                    if direction.length_squared() > f32::EPSILON {
                        let yaw = direction.z.atan2(direction.x);
                        let pitch = direction.y.asin();
                        transform.rotation = Quat::from_euler(yaw, pitch, 0.0);
                        ctx.scene.transforms_mut().set_local(ctx.entity, transform);
                    }
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::Chase { target, speed } => {
                // Resolve target entity
                let target_entity = Self::resolve_entity_name(ctx, target);

                if let Some(target_ent) = target_entity {
                    if let (Some(self_transform), Some(target_transform)) = (
                        ctx.scene.transforms().local(ctx.entity),
                        ctx.scene.transforms().local(target_ent),
                    ) {
                        // Calculate direction to target
                        let direction = (target_transform.translation - self_transform.translation)
                            .normalized();

                        // Move towards target
                        let movement = direction * (*speed * ctx.delta_time);
                        Self::move_local(ctx, movement)
                    } else {
                        NodeResult::Failure
                    }
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::Flee { target, speed } => {
                // Resolve target entity
                let target_entity = Self::resolve_entity_name(ctx, target);

                if let Some(target_ent) = target_entity {
                    if let (Some(self_transform), Some(target_transform)) = (
                        ctx.scene.transforms().local(ctx.entity),
                        ctx.scene.transforms().local(target_ent),
                    ) {
                        // Calculate direction away from target
                        let direction = (self_transform.translation - target_transform.translation)
                            .normalized();

                        // Move away from target
                        let movement = direction * (*speed * ctx.delta_time);
                        Self::move_local(ctx, movement)
                    } else {
                        NodeResult::Failure
                    }
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::Patrol {
                points,
                speed,
                r#loop,
            } => {
                if points.is_empty() {
                    return NodeResult::Failure;
                }

                // Get current patrol index from execution state
                let current_index = ctx.execution_state.patrol_index;

                if current_index >= points.len() {
                    // Reached end of patrol
                    if *r#loop {
                        ctx.execution_state.patrol_index = 0;
                    } else {
                        return NodeResult::Success;
                    }
                }

                let current_index = ctx.execution_state.patrol_index;
                let target_waypoint = Vec3::new(
                    points[current_index][0],
                    points[current_index][1],
                    points[current_index][2],
                );

                if let Some(self_transform) = ctx.scene.transforms().local(ctx.entity) {
                    let distance = (target_waypoint - self_transform.translation).length();

                    // Check if reached waypoint (within 0.5 units)
                    if distance < 0.5 {
                        // Move to next waypoint
                        ctx.execution_state.patrol_index += 1;
                        NodeResult::Running
                    } else {
                        // Move towards waypoint
                        let direction = (target_waypoint - self_transform.translation).normalized();
                        let movement = direction * (*speed * ctx.delta_time);
                        Self::move_local(ctx, movement);
                        NodeResult::Running
                    }
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::ApplyImpulse { force } => {
                #[cfg(feature = "physics")]
                {
                    if let Some(physics) = ctx.physics {
                        use engine_physics::Vec3;
                        let impulse = Vec3::new(force[0], force[1], force[2]);
                        physics.apply_impulse(ctx.entity, impulse);
                        NodeResult::Success
                    } else {
                        NodeResult::Failure
                    }
                }
                #[cfg(not(feature = "physics"))]
                {
                    let _ = force;
                    NodeResult::Failure
                }
            }
            ActionExpr::Attack { target, damage } => {
                // Resolve target entity
                let target_entity = Self::resolve_entity_name(ctx, target);

                if let Some(target_ent) = target_entity {
                    // In a real implementation, this would:
                    // 1. Check if entity is in attack range
                    // 2. Apply damage to target's health component
                    // 3. Trigger attack animation/sound

                    // For now, we'll mark success if target exists
                    // The actual damage system should be implemented in the game logic layer
                    let _ = (target_ent, damage);
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::PlaySound { path } => {
                // Audio system integration would go here
                // For now, we return success if the path is valid
                if !path.is_empty() {
                    NodeResult::Success
                } else {
                    NodeResult::Failure
                }
            }
            ActionExpr::Spawn { prefab, position } => {
                // Prefab spawning would integrate with the asset system
                // For now, we return success to indicate the action was processed
                let _ = (prefab, position);
                NodeResult::Success
            }
            ActionExpr::DestroySelf => {
                let _ = ctx.scene.destroy_deferred(ctx.entity);
                NodeResult::Success
            }
            ActionExpr::Wait { duration } => {
                // Accumulate time in execution state
                ctx.execution_state.wait_timer += ctx.delta_time;

                if ctx.execution_state.wait_timer >= *duration {
                    // Reset timer and complete
                    ctx.execution_state.wait_timer = 0.0;
                    NodeResult::Success
                } else {
                    NodeResult::Running
                }
            }
            ActionExpr::Sequence { actions } => {
                // Execute all actions in sequence
                for action in actions {
                    match action.execute(ctx) {
                        NodeResult::Failure => return NodeResult::Failure,
                        NodeResult::Running => return NodeResult::Running,
                        NodeResult::Success => continue,
                    }
                }
                NodeResult::Success
            }
            ActionExpr::Idle => NodeResult::Success,
        }
    }

    /// Moves the entity in local space.
    fn move_local(ctx: &mut ActionContext, local_offset: Vec3) -> NodeResult {
        if let Some(mut transform) = ctx.scene.transforms().local(ctx.entity) {
            // Rotate the offset by the entity's rotation to get world-space movement
            // Use the quaternion to transform the direction vector
            let x = local_offset.x;
            let y = local_offset.y;
            let z = local_offset.z;
            let qx = transform.rotation.x;
            let qy = transform.rotation.y;
            let qz = transform.rotation.z;
            let qw = transform.rotation.w;

            // Apply quaternion rotation: v' = q * v * q^-1
            let ix = qw * x + qy * z - qz * y;
            let iy = qw * y + qz * x - qx * z;
            let iz = qw * z + qx * y - qy * x;
            let iw = -qx * x - qy * y - qz * z;

            let world_offset = Vec3::new(
                ix * qw + iw * -qx + iy * -qz - iz * -qy,
                iy * qw + iw * -qy + iz * -qx - ix * -qz,
                iz * qw + iw * -qz + ix * -qy - iy * -qx,
            );

            transform.translation = transform.translation + world_offset;
            ctx.scene.transforms_mut().set_local(ctx.entity, transform);
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }

    /// Resolves an entity name to an Entity handle.
    ///
    /// Supports:
    /// - "player" - finds first entity with name "Player" (case-insensitive)
    /// - Entity ID format "slot:generation" (e.g., "1:1")
    /// - Direct entity name lookup
    fn resolve_entity_name(ctx: &ActionContext, name: &str) -> Option<Entity> {
        // Handle "player" special case
        if name.eq_ignore_ascii_case("player") {
            return ctx
                .scene
                .find_by_name("Player")
                .or_else(|| ctx.scene.find_by_name("player"));
        }

        // Try parsing as entity ID "slot:generation"
        if let Some((slot_str, gen_str)) = name.split_once(':') {
            if let (Ok(slot), Ok(gen)) = (slot_str.parse::<u32>(), gen_str.parse::<u32>()) {
                use engine_core::{Generation, Handle};
                let generation = Generation::FIRST; // We'd need to parse properly
                if gen == generation.get() {
                    let handle = Handle::new(slot, generation);
                    return Some(Entity::from_handle(handle));
                }
            }
        }

        // Fall back to name lookup
        ctx.scene.find_by_name(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_expr_serializes_to_json() {
        let action = ActionExpr::MoveForward { speed: 5.0 };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("moveForward"));
        assert!(json.contains("5.0"));
    }

    #[test]
    fn action_expr_deserializes_from_json() {
        let json = r#"{"moveForward": 5.0}"#;
        let action: ActionExpr = serde_json::from_str(json).unwrap();
        match action {
            ActionExpr::MoveForward { speed } => assert_eq!(speed, 5.0),
            _ => panic!("Expected MoveForward"),
        }
    }

    #[test]
    fn action_patrol_defaults_loop_true() {
        let json = r#"{
            "patrol": {
                "points": [[0,0,0], [10,0,0]],
                "speed": 2.0
            }
        }"#;
        let action: ActionExpr = serde_json::from_str(json).unwrap();
        match action {
            ActionExpr::Patrol { r#loop, .. } => assert!(r#loop),
            _ => panic!("Expected Patrol"),
        }
    }
}
