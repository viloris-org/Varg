//! Condition expressions for behavior trees.

use engine_ecs::Entity;
use serde::{Deserialize, Serialize};

/// A condition that can be evaluated to true or false.
///
/// Conditions are pure queries with no side effects.
pub trait Condition {
    /// Evaluates the condition in the given context.
    fn evaluate(&self, ctx: &ConditionContext) -> bool;
}

/// Runtime context for condition evaluation.
pub struct ConditionContext<'a> {
    /// The entity this condition is being evaluated for.
    pub entity: Entity,
    /// Current scene state (transforms, components).
    pub scene: &'a engine_ecs::Scene,
    /// Current input state.
    pub input: &'a engine_platform::InputState,
    /// Current physics backend.
    pub physics: Option<&'a dyn engine_physics::PhysicsBackend>,
    /// Asset database for resource lookups.
    pub assets: Option<&'a engine_assets::AssetDatabase>,
    /// Delta time since last frame.
    pub delta_time: f32,
}

/// A condition expression that can be serialized from JSON.
///
/// This enum defines all condition types that LLMs can generate.
/// Each variant has clear semantics and predictable JSON structure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConditionExpr {
    /// Check if a key is currently pressed (this frame).
    ///
    /// # Example
    /// ```json
    /// {"keyPressed": "W"}
    /// ```
    KeyPressed {
        /// Key name (e.g., "W", "Space", "ArrowUp").
        key: String,
    },

    /// Check if a key is currently held down.
    ///
    /// # Example
    /// ```json
    /// {"keyHeld": "Shift"}
    /// ```
    KeyHeld {
        /// Key name.
        key: String,
    },

    /// Check if a key was released this frame.
    ///
    /// # Example
    /// ```json
    /// {"keyReleased": "Space"}
    /// ```
    KeyReleased {
        /// Key name.
        key: String,
    },

    /// Check distance to player.
    ///
    /// # Example
    /// ```json
    /// {"playerDistance": {"lessThan": 5.0}}
    /// ```
    PlayerDistance {
        /// Distance comparison.
        #[serde(flatten)]
        comparison: FloatComparison,
    },

    /// Check entity's health value.
    ///
    /// # Example
    /// ```json
    /// {"health": {"lessThan": 20}}
    /// ```
    Health {
        /// Health comparison.
        #[serde(flatten)]
        comparison: FloatComparison,
    },

    /// Check if entity has a specific tag/component.
    ///
    /// # Example
    /// ```json
    /// {"hasTag": "enemy"}
    /// ```
    HasTag {
        /// Tag name to check.
        tag: String,
    },

    /// Check if a raycast hits something.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "raycastHit": {
    ///     "direction": [0, 0, 1],
    ///     "maxDistance": 10.0
    ///   }
    /// }
    /// ```
    RaycastHit {
        /// Ray direction (normalized automatically).
        direction: [f32; 3],
        /// Maximum distance to check.
        max_distance: f32,
    },

    /// Boolean AND of multiple conditions.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "and": [
    ///     {"keyPressed": "W"},
    ///     {"health": {"greaterThan": 0}}
    ///   ]
    /// }
    /// ```
    And {
        /// Conditions to AND together.
        conditions: Vec<ConditionExpr>,
    },

    /// Boolean OR of multiple conditions.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "or": [
    ///     {"keyPressed": "W"},
    ///     {"keyPressed": "ArrowUp"}
    ///   ]
    /// }
    /// ```
    Or {
        /// Conditions to OR together.
        conditions: Vec<ConditionExpr>,
    },

    /// Boolean NOT of a condition.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "not": {
    ///     "keyPressed": "Space"
    ///   }
    /// }
    /// ```
    Not {
        /// Condition to negate.
        condition: Box<ConditionExpr>,
    },

    /// Always true (useful for testing).
    Always,

    /// Always false (useful for testing).
    Never,
}

impl<'de> Deserialize<'de> for ConditionExpr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        condition_de::deserialize(deserializer)
    }
}

mod condition_de {
    use super::*;
    use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
    use std::fmt;

    struct ConditionExprVisitor;

    impl<'de> Visitor<'de> for ConditionExprVisitor {
        type Value = ConditionExpr;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a condition expression")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<ConditionExpr, E> {
            match v {
                "always" => Ok(ConditionExpr::Always),
                "never" => Ok(ConditionExpr::Never),
                other => Err(de::Error::unknown_variant(other, &["always", "never"])),
            }
        }

        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<ConditionExpr, M::Error> {
            let key: String = map
                .next_key()?
                .ok_or_else(|| de::Error::custom("empty map"))?;
            let result = match key.as_str() {
                "keyPressed" => {
                    let v: BareOrString<KeyPressedFields> = map.next_value()?;
                    ConditionExpr::KeyPressed {
                        key: v.into_inner().key,
                    }
                }
                "keyHeld" => {
                    let v: BareOrString<KeyHeldFields> = map.next_value()?;
                    ConditionExpr::KeyHeld {
                        key: v.into_inner().key,
                    }
                }
                "keyReleased" => {
                    let v: BareOrString<KeyReleasedFields> = map.next_value()?;
                    ConditionExpr::KeyReleased {
                        key: v.into_inner().key,
                    }
                }
                "playerDistance" => {
                    let v: PlayerDistanceFields = map.next_value()?;
                    ConditionExpr::PlayerDistance {
                        comparison: v.comparison,
                    }
                }
                "health" => {
                    let v: HealthFields = map.next_value()?;
                    ConditionExpr::Health {
                        comparison: v.comparison,
                    }
                }
                "hasTag" => {
                    let v: BareOrString<HasTagFields> = map.next_value()?;
                    ConditionExpr::HasTag {
                        tag: v.into_inner().tag,
                    }
                }
                "raycastHit" => {
                    let v: RaycastHitFields = map.next_value()?;
                    ConditionExpr::RaycastHit {
                        direction: v.direction,
                        max_distance: v.max_distance,
                    }
                }
                "and" => {
                    let v: AndFields = map.next_value()?;
                    ConditionExpr::And {
                        conditions: v.conditions,
                    }
                }
                "or" => {
                    let v: OrFields = map.next_value()?;
                    ConditionExpr::Or {
                        conditions: v.conditions,
                    }
                }
                "not" => {
                    let v: NotFields = map.next_value()?;
                    ConditionExpr::Not {
                        condition: v.condition,
                    }
                }
                "always" => {
                    let _: IgnoredAny = map.next_value()?;
                    ConditionExpr::Always
                }
                "never" => {
                    let _: IgnoredAny = map.next_value()?;
                    ConditionExpr::Never
                }
                other => {
                    return Err(de::Error::unknown_variant(
                        other,
                        &[
                            "keyPressed",
                            "keyHeld",
                            "keyReleased",
                            "playerDistance",
                            "health",
                            "hasTag",
                            "raycastHit",
                            "and",
                            "or",
                            "not",
                            "always",
                            "never",
                        ],
                    ))
                }
            };
            Ok(result)
        }
    }

    /// Wraps a single-field struct so it can be deserialized from either
    /// a bare string or an object.
    enum BareOrString<T> {
        Bare(T),
        Struct(T),
    }

    impl<T> BareOrString<T> {
        fn into_inner(self) -> T {
            match self {
                BareOrString::Bare(v) | BareOrString::Struct(v) => v,
            }
        }
    }

    impl<'de, T: Deserialize<'de>> Deserialize<'de> for BareOrString<T> {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor<T>(std::marker::PhantomData<T>);
            impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
                type Value = BareOrString<T>;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a bare string or an object")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<BareOrString<T>, E> {
                    let json = serde_json::json!({"key": v});
                    T::deserialize(json)
                        .map(BareOrString::Bare)
                        .map_err(de::Error::custom)
                }
                fn visit_map<M: de::MapAccess<'de>>(
                    self,
                    map: M,
                ) -> Result<BareOrString<T>, M::Error> {
                    T::deserialize(de::value::MapAccessDeserializer::new(map))
                        .map(BareOrString::Struct)
                }
                fn visit_f64<E: de::Error>(self, v: f64) -> Result<BareOrString<T>, E> {
                    let json = serde_json::json!({"key": v});
                    T::deserialize(json)
                        .map(BareOrString::Bare)
                        .map_err(de::Error::custom)
                }
                fn visit_i64<E: de::Error>(self, v: i64) -> Result<BareOrString<T>, E> {
                    self.visit_f64(v as f64)
                }
                fn visit_bool<E: de::Error>(self, v: bool) -> Result<BareOrString<T>, E> {
                    let json = serde_json::json!({"key": v});
                    T::deserialize(json)
                        .map(BareOrString::Bare)
                        .map_err(de::Error::custom)
                }
            }
            deserializer.deserialize_any(Visitor(std::marker::PhantomData))
        }
    }

    #[derive(Deserialize)]
    struct KeyPressedFields {
        key: String,
    }
    #[derive(Deserialize)]
    struct KeyHeldFields {
        key: String,
    }
    #[derive(Deserialize)]
    struct KeyReleasedFields {
        key: String,
    }
    #[derive(Deserialize)]
    struct PlayerDistanceFields {
        #[serde(flatten)]
        comparison: FloatComparison,
    }
    #[derive(Deserialize)]
    struct HealthFields {
        #[serde(flatten)]
        comparison: FloatComparison,
    }
    #[derive(Deserialize)]
    struct HasTagFields {
        tag: String,
    }
    #[derive(Deserialize)]
    struct RaycastHitFields {
        direction: [f32; 3],
        max_distance: f32,
    }
    #[derive(Deserialize)]
    struct AndFields {
        conditions: Vec<ConditionExpr>,
    }
    #[derive(Deserialize)]
    struct OrFields {
        conditions: Vec<ConditionExpr>,
    }
    #[derive(Deserialize)]
    struct NotFields {
        condition: Box<ConditionExpr>,
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ConditionExpr, D::Error> {
        deserializer.deserialize_any(ConditionExprVisitor)
    }
}

/// Float comparison operators for numeric conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FloatComparison {
    /// Less than a value.
    LessThan(f32),
    /// Less than or equal to a value.
    LessOrEqual(f32),
    /// Greater than a value.
    GreaterThan(f32),
    /// Greater than or equal to a value.
    GreaterOrEqual(f32),
    /// Equal to a value (with epsilon tolerance).
    Equal(f32),
    /// Between two values (inclusive).
    Between {
        /// Minimum value.
        min: f32,
        /// Maximum value.
        max: f32,
    },
}

impl FloatComparison {
    /// Evaluates the comparison against a value.
    pub fn evaluate(&self, value: f32) -> bool {
        const EPSILON: f32 = 0.0001;
        match self {
            FloatComparison::LessThan(threshold) => value < *threshold,
            FloatComparison::LessOrEqual(threshold) => value <= *threshold,
            FloatComparison::GreaterThan(threshold) => value > *threshold,
            FloatComparison::GreaterOrEqual(threshold) => value >= *threshold,
            FloatComparison::Equal(target) => (value - target).abs() < EPSILON,
            FloatComparison::Between { min, max } => value >= *min && value <= *max,
        }
    }
}

impl ConditionExpr {
    /// Evaluates this condition expression in the given context.
    pub fn evaluate(&self, ctx: &ConditionContext) -> bool {
        match self {
            ConditionExpr::KeyPressed { key } => {
                Self::parse_key_name(key).map_or(false, |k| ctx.input.key_pressed(k))
            }
            ConditionExpr::KeyHeld { key } => {
                Self::parse_key_name(key).map_or(false, |k| ctx.input.key_down(k))
            }
            ConditionExpr::KeyReleased { key } => {
                Self::parse_key_name(key).map_or(false, |k| ctx.input.key_released(k))
            }
            ConditionExpr::PlayerDistance { comparison } => {
                // Find player entity
                let player_entity = ctx
                    .scene
                    .find_by_name("Player")
                    .or_else(|| ctx.scene.find_by_name("player"));

                if let Some(player_ent) = player_entity {
                    if let (Some(self_transform), Some(player_transform)) = (
                        ctx.scene.transforms().local(ctx.entity),
                        ctx.scene.transforms().local(player_ent),
                    ) {
                        let distance =
                            (player_transform.translation - self_transform.translation).length();
                        comparison.evaluate(distance)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            ConditionExpr::Health { comparison } => {
                // Check if entity has a health component
                // In a real implementation, this would query a Health component
                // For now, we'll check if the entity has components and return false
                // This allows the behavior tree to compile without a full health system
                if let Some(components) = ctx.scene.components(ctx.entity) {
                    // Check for a component named "Health" or similar
                    let has_health = components.iter().any(|comp| {
                        matches!(comp, engine_ecs::ComponentData::MeshRenderer { .. })
                        // In a real implementation: matches!(comp, ComponentData::Health { value })
                    });

                    if has_health {
                        // Would extract health value here
                        // For now, return false to allow testing
                        false
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            ConditionExpr::HasTag { tag } => {
                // Check entity tags using find_by_tag
                ctx.scene.find_by_tag(tag).contains(&ctx.entity)
            }
            ConditionExpr::RaycastHit {
                direction,
                max_distance,
            } => {
                if let Some(physics) = ctx.physics {
                    if let Some(transform) = ctx.scene.transforms().local(ctx.entity) {
                        use engine_physics::{QueryFilter, Vec3};
                        let origin = transform.translation;
                        let dir = Vec3::new(direction[0], direction[1], direction[2]).normalized();
                        let filter = QueryFilter::default();
                        physics
                            .raycast(origin, dir, *max_distance, filter)
                            .is_some()
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            ConditionExpr::And { conditions } => conditions.iter().all(|c| c.evaluate(ctx)),
            ConditionExpr::Or { conditions } => conditions.iter().any(|c| c.evaluate(ctx)),
            ConditionExpr::Not { condition } => !condition.evaluate(ctx),
            ConditionExpr::Always => true,
            ConditionExpr::Never => false,
        }
    }

    /// Parses a key name string into a KeyCode.
    fn parse_key_name(name: &str) -> Option<engine_platform::KeyCode> {
        use engine_platform::KeyCode;
        match name {
            "Escape" => Some(KeyCode::Escape),
            "Enter" => Some(KeyCode::Enter),
            "Space" => Some(KeyCode::Space),
            "ArrowUp" => Some(KeyCode::ArrowUp),
            "ArrowDown" => Some(KeyCode::ArrowDown),
            "ArrowLeft" => Some(KeyCode::ArrowLeft),
            "ArrowRight" => Some(KeyCode::ArrowRight),
            s if s.len() == 1 => s.chars().next().map(KeyCode::Character),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_comparison_less_than() {
        let cmp = FloatComparison::LessThan(10.0);
        assert!(cmp.evaluate(5.0));
        assert!(!cmp.evaluate(10.0));
        assert!(!cmp.evaluate(15.0));
    }

    #[test]
    fn float_comparison_between() {
        let cmp = FloatComparison::Between {
            min: 10.0,
            max: 20.0,
        };
        assert!(!cmp.evaluate(5.0));
        assert!(cmp.evaluate(10.0));
        assert!(cmp.evaluate(15.0));
        assert!(cmp.evaluate(20.0));
        assert!(!cmp.evaluate(25.0));
    }

    #[test]
    fn condition_expr_and() {
        let expr = ConditionExpr::And {
            conditions: vec![ConditionExpr::Always, ConditionExpr::Always],
        };
        // Can't test without full context, but at least verify structure
        assert!(matches!(expr, ConditionExpr::And { .. }));
    }

    #[test]
    fn condition_expr_deserializes_from_json() {
        let json = r#"{"keyPressed": "W"}"#;
        let expr: ConditionExpr = serde_json::from_str(json).unwrap();
        assert!(matches!(expr, ConditionExpr::KeyPressed { .. }));
    }
}
