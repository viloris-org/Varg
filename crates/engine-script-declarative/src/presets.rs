//! Behavior tree presets for common AI patterns.
//!
//! These presets provide high-level, LLM-friendly patterns that reduce the need
//! for LLMs to construct complex behavior trees from scratch.

use crate::{ActionExpr, BehaviorNode, BehaviorSchema, ConditionExpr, FloatComparison};

/// Creates a patrol-and-chase behavior preset.
///
/// The entity patrols between waypoints until a player comes within chase distance,
/// then pursues the player. If the player gets within attack distance, performs an attack.
///
/// # Parameters (JSON)
/// ```json
/// {
///   "patrol_points": [[0,0,0], [10,0,0], [10,0,10]],
///   "patrol_speed": 2.0,
///   "chase_distance": 8.0,
///   "chase_speed": 4.0,
///   "attack_distance": 1.5,
///   "attack_damage": 10
/// }
/// ```
pub fn patrol_and_chase(
    entity_name: impl Into<String>,
    patrol_points: Vec<[f32; 3]>,
    patrol_speed: f32,
    chase_distance: f32,
    chase_speed: f32,
    attack_distance: f32,
    attack_damage: i32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Selector {
        name: Some("patrol_or_chase".into()),
        children: vec![
            // Combat branch: if player is close, chase/attack
            BehaviorNode::Sequence {
                name: Some("combat".into()),
                children: vec![
                    BehaviorNode::Condition {
                        check: ConditionExpr::PlayerDistance {
                            comparison: FloatComparison::LessThan(chase_distance),
                        },
                    },
                    BehaviorNode::Selector {
                        name: None,
                        children: vec![
                            // Attack if very close
                            BehaviorNode::Sequence {
                                name: None,
                                children: vec![
                                    BehaviorNode::Condition {
                                        check: ConditionExpr::PlayerDistance {
                                            comparison: FloatComparison::LessThan(attack_distance),
                                        },
                                    },
                                    BehaviorNode::Action {
                                        action: ActionExpr::Attack {
                                            target: "player".into(),
                                            damage: attack_damage,
                                        },
                                    },
                                ],
                            },
                            // Otherwise chase
                            BehaviorNode::Action {
                                action: ActionExpr::Chase {
                                    target: "player".into(),
                                    speed: chase_speed,
                                },
                            },
                        ],
                    },
                ],
            },
            // Patrol branch: default behavior
            BehaviorNode::Action {
                action: ActionExpr::Patrol {
                    points: patrol_points,
                    speed: patrol_speed,
                    r#loop: true,
                },
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Patrols waypoints and chases player when close")
}

/// Creates a guard area behavior preset.
///
/// The entity stays at a guard position, rotating to scan for threats.
/// If a player enters the guard radius, the entity attacks.
///
/// # Parameters (JSON)
/// ```json
/// {
///   "guard_position": [5, 0, 5],
///   "guard_radius": 10.0,
///   "scan_speed": 30.0,
///   "attack_damage": 15
/// }
/// ```
pub fn guard_area(
    entity_name: impl Into<String>,
    _guard_position: [f32; 3],
    guard_radius: f32,
    scan_speed: f32,
    attack_damage: i32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Parallel {
        name: Some("guard".into()),
        children: vec![
            // Always rotate to scan
            BehaviorNode::Action {
                action: ActionExpr::RotateY {
                    degrees_per_sec: scan_speed,
                },
            },
            // Attack if player is in range
            BehaviorNode::Sequence {
                name: Some("attack_intruder".into()),
                children: vec![
                    BehaviorNode::Condition {
                        check: ConditionExpr::PlayerDistance {
                            comparison: FloatComparison::LessThan(guard_radius),
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::LookAt {
                            target: [0.0, 0.0, 0.0], // placeholder for player position
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::Attack {
                            target: "player".into(),
                            damage: attack_damage,
                        },
                    },
                ],
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Guards an area and attacks intruders")
}

/// Creates a collect items behavior preset.
///
/// The entity searches for collectible items and moves to collect them.
/// Once all items are collected, the entity idles.
///
/// # Parameters (JSON)
/// ```json
/// {
///   "search_radius": 15.0,
///   "move_speed": 3.0,
///   "collect_distance": 1.0
/// }
/// ```
pub fn collect_items(
    entity_name: impl Into<String>,
    _search_radius: f32,
    move_speed: f32,
    collect_distance: f32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Sequence {
        name: Some("collect".into()),
        children: vec![
            // Find nearest collectible (simplified - would need actual implementation)
            BehaviorNode::Condition {
                check: ConditionExpr::HasTag {
                    tag: "collectible".into(),
                },
            },
            // Move towards it
            BehaviorNode::Action {
                action: ActionExpr::Chase {
                    target: "nearest_collectible".into(),
                    speed: move_speed,
                },
            },
            // Wait until close
            BehaviorNode::Condition {
                check: ConditionExpr::PlayerDistance {
                    comparison: FloatComparison::LessThan(collect_distance),
                },
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Searches for and collects nearby items")
}

/// Creates a flee when damaged behavior preset.
///
/// The entity flees from the player when health drops below a threshold.
/// Otherwise, performs a default action (idle or custom).
///
/// # Parameters (JSON)
/// ```json
/// {
///   "health_threshold": 30,
///   "flee_speed": 6.0,
///   "flee_distance": 20.0
/// }
/// ```
pub fn flee_when_damaged(
    entity_name: impl Into<String>,
    health_threshold: i32,
    flee_speed: f32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Selector {
        name: Some("flee_or_idle".into()),
        children: vec![
            // Flee if damaged
            BehaviorNode::Sequence {
                name: Some("flee".into()),
                children: vec![
                    BehaviorNode::Condition {
                        check: ConditionExpr::Health {
                            comparison: FloatComparison::LessThan(health_threshold as f32),
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::Flee {
                            target: "player".into(),
                            speed: flee_speed,
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::PlaySound {
                            path: "sounds/scared.ogg".into(),
                        },
                    },
                ],
            },
            // Otherwise idle
            BehaviorNode::Action {
                action: ActionExpr::Idle,
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Flees from threats when health is low")
}

/// Creates a follow player behavior preset.
///
/// The entity follows the player when a key is pressed and stays within a certain distance.
///
/// # Parameters (JSON)
/// ```json
/// {
///   "follow_key": "E",
///   "follow_speed": 4.0,
///   "follow_distance": 2.0,
///   "activation_range": 3.0
/// }
/// ```
pub fn follow_player(
    entity_name: impl Into<String>,
    follow_key: impl Into<String>,
    follow_speed: f32,
    follow_distance: f32,
    activation_range: f32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Sequence {
        name: Some("follow".into()),
        children: vec![
            // Check if player pressed activation key nearby
            BehaviorNode::Condition {
                check: ConditionExpr::And {
                    conditions: vec![
                        ConditionExpr::KeyPressed {
                            key: follow_key.into(),
                        },
                        ConditionExpr::PlayerDistance {
                            comparison: FloatComparison::LessThan(activation_range),
                        },
                    ],
                },
            },
            // Follow but stop when close enough
            BehaviorNode::Selector {
                name: None,
                children: vec![
                    // Don't move if already close
                    BehaviorNode::Condition {
                        check: ConditionExpr::PlayerDistance {
                            comparison: FloatComparison::LessThan(follow_distance),
                        },
                    },
                    // Otherwise chase
                    BehaviorNode::Action {
                        action: ActionExpr::Chase {
                            target: "player".into(),
                            speed: follow_speed,
                        },
                    },
                ],
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Follows the player when activated")
}

/// Creates a turret behavior preset.
///
/// Stationary entity that rotates towards the player and shoots when in range.
///
/// # Parameters (JSON)
/// ```json
/// {
///   "detection_range": 15.0,
///   "shoot_range": 10.0,
///   "damage": 5,
///   "fire_rate": 1.0,
///   "scan_speed": 30.0
/// }
/// ```
pub fn turret(
    entity_name: impl Into<String>,
    detection_range: f32,
    shoot_range: f32,
    damage: i32,
    fire_rate: f32,
) -> BehaviorSchema {
    let behavior = BehaviorNode::Selector {
        name: Some("turret_ai".into()),
        children: vec![
            // Combat mode: player in range
            BehaviorNode::Sequence {
                name: Some("combat".into()),
                children: vec![
                    BehaviorNode::Condition {
                        check: ConditionExpr::PlayerDistance {
                            comparison: FloatComparison::LessThan(detection_range),
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::LookAt {
                            target: [0.0, 0.0, 0.0], // player position
                        },
                    },
                    // Shoot if in shoot range
                    BehaviorNode::Selector {
                        name: None,
                        children: vec![BehaviorNode::Sequence {
                            name: None,
                            children: vec![
                                BehaviorNode::Condition {
                                    check: ConditionExpr::PlayerDistance {
                                        comparison: FloatComparison::LessThan(shoot_range),
                                    },
                                },
                                BehaviorNode::Action {
                                    action: ActionExpr::Attack {
                                        target: "player".into(),
                                        damage,
                                    },
                                },
                                BehaviorNode::Action {
                                    action: ActionExpr::Wait {
                                        duration: fire_rate,
                                    },
                                },
                            ],
                        }],
                    },
                ],
            },
            // Idle mode: scan for targets
            BehaviorNode::Action {
                action: ActionExpr::RotateY {
                    degrees_per_sec: 30.0,
                },
            },
        ],
    };

    BehaviorSchema::new(entity_name, vec![behavior])
        .with_description("Stationary turret that shoots at player when in range")
}

/// Parses a preset name and parameters into a BehaviorSchema.
///
/// This is the main entry point for the AI agent to use presets.
pub fn parse_preset(
    preset_name: &str,
    entity_name: impl Into<String>,
    params: &serde_json::Value,
) -> Result<BehaviorSchema, String> {
    let entity = entity_name.into();

    match preset_name {
        "patrol_and_chase" => {
            let patrol_points = params
                .get("patrol_points")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 10.0]]);
            let patrol_speed = params
                .get("patrol_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(2.0) as f32;
            let chase_distance = params
                .get("chase_distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(8.0) as f32;
            let chase_speed = params
                .get("chase_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(4.0) as f32;
            let attack_distance = params
                .get("attack_distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.5) as f32;
            let attack_damage = params
                .get("attack_damage")
                .and_then(|v| v.as_i64())
                .unwrap_or(10) as i32;

            Ok(patrol_and_chase(
                entity,
                patrol_points,
                patrol_speed,
                chase_distance,
                chase_speed,
                attack_distance,
                attack_damage,
            ))
        }
        "guard_area" => {
            let guard_position = params
                .get("guard_position")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or([5.0, 0.0, 5.0]);
            let guard_radius = params
                .get("guard_radius")
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0) as f32;
            let scan_speed = params
                .get("scan_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(30.0) as f32;
            let attack_damage = params
                .get("attack_damage")
                .and_then(|v| v.as_i64())
                .unwrap_or(15) as i32;

            Ok(guard_area(
                entity,
                guard_position,
                guard_radius,
                scan_speed,
                attack_damage,
            ))
        }
        "collect_items" => {
            let search_radius = params
                .get("search_radius")
                .and_then(|v| v.as_f64())
                .unwrap_or(15.0) as f32;
            let move_speed = params
                .get("move_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0) as f32;
            let collect_distance = params
                .get("collect_distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            Ok(collect_items(
                entity,
                search_radius,
                move_speed,
                collect_distance,
            ))
        }
        "flee_when_damaged" => {
            let health_threshold = params
                .get("health_threshold")
                .and_then(|v| v.as_i64())
                .unwrap_or(30) as i32;
            let flee_speed = params
                .get("flee_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(6.0) as f32;

            Ok(flee_when_damaged(entity, health_threshold, flee_speed))
        }
        "follow_player" => {
            let follow_key = params
                .get("follow_key")
                .and_then(|v| v.as_str())
                .unwrap_or("E");
            let follow_speed = params
                .get("follow_speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(4.0) as f32;
            let follow_distance = params
                .get("follow_distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(2.0) as f32;
            let activation_range = params
                .get("activation_range")
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0) as f32;

            Ok(follow_player(
                entity,
                follow_key,
                follow_speed,
                follow_distance,
                activation_range,
            ))
        }
        "turret" => {
            let detection_range = params
                .get("detection_range")
                .and_then(|v| v.as_f64())
                .unwrap_or(15.0) as f32;
            let shoot_range = params
                .get("shoot_range")
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0) as f32;
            let damage = params
                .get("damage")
                .and_then(|v| v.as_i64())
                .unwrap_or(5) as i32;
            let fire_rate = params
                .get("fire_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            Ok(turret(
                entity,
                detection_range,
                shoot_range,
                damage,
                fire_rate,
            ))
        }
        _ => Err(format!(
            "Unknown preset: '{}'. Available presets: patrol_and_chase, guard_area, collect_items, flee_when_damaged, follow_player, turret",
            preset_name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patrol_and_chase_preset() {
        let schema = patrol_and_chase(
            "Enemy",
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            2.0,
            8.0,
            4.0,
            1.5,
            10,
        );

        assert_eq!(schema.entity, "Enemy");
        assert_eq!(schema.behaviors.len(), 1);
        schema.validate().expect("Schema should be valid");
    }

    #[test]
    fn test_parse_preset_with_defaults() {
        let params = serde_json::json!({});
        let schema = parse_preset("flee_when_damaged", "NPC", &params)
            .expect("Should parse with default params");

        assert_eq!(schema.entity, "NPC");
        schema.validate().expect("Schema should be valid");
    }

    #[test]
    fn test_parse_unknown_preset() {
        let params = serde_json::json!({});
        let result = parse_preset("unknown_preset", "Test", &params);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown preset"));
    }
}
