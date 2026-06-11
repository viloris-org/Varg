//! JSON schema definitions for behavior trees.

use serde::{Deserialize, Serialize};

use crate::BehaviorNode;

/// Top-level schema for an entity's declarative behavior.
///
/// This is what LLMs should generate as JSON output.
///
/// # Example
/// ```json
/// {
///   "entity": "Enemy",
///   "description": "Basic patrol and chase AI",
///   "behaviors": [
///     {
///       "type": "Selector",
///       "children": [
///         {
///           "type": "Sequence",
///           "children": [
///             {"type": "Condition", "check": {"playerDistance": {"lessThan": 5.0}}},
///             {"type": "Action", "do": {"chase": {"target": "player", "speed": 4.0}}}
///           ]
///         },
///         {
///           "type": "Action",
///           "do": {"patrol": {"points": [[0,0,0], [10,0,0]], "speed": 2.0}}
///         }
///       ]
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorSchema {
    /// Entity name for debugging.
    pub entity: String,

    /// Optional human-readable description of the behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Root behavior nodes (typically one, but can have multiple for parallel top-level behaviors).
    pub behaviors: Vec<BehaviorNode>,
}

impl BehaviorSchema {
    /// Creates a new behavior schema.
    pub fn new(entity: impl Into<String>, behaviors: Vec<BehaviorNode>) -> Self {
        Self {
            entity: entity.into(),
            description: None,
            behaviors,
        }
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Validates the schema structure.
    ///
    /// Checks for common errors that LLMs might make:
    /// - Empty behavior list
    /// - Sequences/Selectors with no children
    /// - Deeply nested trees (> 10 levels)
    pub fn validate(&self) -> Result<(), String> {
        if self.behaviors.is_empty() {
            return Err(
                "Behavior schema must have at least one behavior.\n\
                 Suggestion: Add a root Selector or Sequence node with children.\n\
                 Example: {\"type\": \"Sequence\", \"children\": [{\"type\": \"Action\", \"do\": {\"idle\": true}}]}"
                    .to_string(),
            );
        }

        for (i, behavior) in self.behaviors.iter().enumerate() {
            self.validate_node(behavior, 0)
                .map_err(|e| format!("Behavior {}: {}", i, e))?;
        }

        Ok(())
    }

    fn validate_node(&self, node: &BehaviorNode, depth: usize) -> Result<(), String> {
        const MAX_DEPTH: usize = 10;

        if depth > MAX_DEPTH {
            return Err(format!(
                "Behavior tree too deep (max {} levels). Current depth: {}.\n\
                 Suggestion: Split into multiple behaviors or use behavior presets like 'patrol_and_chase'.\n\
                 Deeply nested trees are hard to debug and maintain.",
                MAX_DEPTH, depth
            ));
        }

        match node {
            BehaviorNode::Sequence { children, .. }
            | BehaviorNode::Selector { children, .. }
            | BehaviorNode::Parallel { children, .. } => {
                if children.is_empty() {
                    let node_type = match node {
                        BehaviorNode::Sequence { .. } => "Sequence",
                        BehaviorNode::Selector { .. } => "Selector",
                        BehaviorNode::Parallel { .. } => "Parallel",
                        _ => "Control",
                    };
                    return Err(format!(
                        "{} node must have at least one child.\n\
                         Suggestion: Add leaf nodes (Condition or Action) as children.\n\
                         Example: {{\"type\": \"{}\", \"children\": [{{\"type\": \"Action\", \"do\": {{\"idle\": true}}}}]}}",
                        node_type, node_type
                    ));
                }
                for child in children {
                    self.validate_node(child, depth + 1)?;
                }
            }
            BehaviorNode::Invert { child }
            | BehaviorNode::Succeed { child }
            | BehaviorNode::Repeat { child, .. } => {
                self.validate_node(child, depth + 1)?;
            }
            BehaviorNode::Condition { .. } | BehaviorNode::Action { .. } => {
                // Leaf nodes are always valid
            }
        }

        Ok(())
    }
}

/// Configuration for an entity's behavior attachment.
///
/// This is used in the ECS to attach declarative behaviors to entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityBehaviorConfig {
    /// Path to the behavior JSON file.
    pub behavior_path: String,

    /// Whether this behavior is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Optional priority (higher runs first if multiple behaviors).
    #[serde(default)]
    pub priority: i32,
}

fn default_true() -> bool {
    true
}

/// JSON Schema (schema.org) for validation by external tools.
///
/// This can be exported to a .json file for LLM tool use schemas.
pub fn generate_json_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "AsterBehaviorSchema",
        "description": "Declarative behavior tree schema for Aster game engine",
        "type": "object",
        "required": ["entity", "behaviors"],
        "properties": {
            "entity": {
                "type": "string",
                "description": "Entity name for debugging"
            },
            "description": {
                "type": "string",
                "description": "Human-readable description of the behavior"
            },
            "behaviors": {
                "type": "array",
                "description": "Root behavior nodes",
                "minItems": 1,
                "items": {
                    "$ref": "#/definitions/BehaviorNode"
                }
            }
        },
        "definitions": {
            "BehaviorNode": {
                "oneOf": [
                    {"$ref": "#/definitions/Sequence"},
                    {"$ref": "#/definitions/Selector"},
                    {"$ref": "#/definitions/Parallel"},
                    {"$ref": "#/definitions/Condition"},
                    {"$ref": "#/definitions/Action"}
                ]
            },
            "Sequence": {
                "type": "object",
                "required": ["type", "children"],
                "properties": {
                    "type": {"const": "Sequence"},
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/BehaviorNode"}
                    }
                }
            },
            "Selector": {
                "type": "object",
                "required": ["type", "children"],
                "properties": {
                    "type": {"const": "Selector"},
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/BehaviorNode"}
                    }
                }
            },
            "Parallel": {
                "type": "object",
                "required": ["type", "children"],
                "properties": {
                    "type": {"const": "Parallel"},
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/BehaviorNode"}
                    }
                }
            },
            "Condition": {
                "type": "object",
                "required": ["type", "check"],
                "properties": {
                    "type": {"const": "Condition"},
                    "check": {"$ref": "#/definitions/ConditionExpr"}
                }
            },
            "Action": {
                "type": "object",
                "required": ["type", "do"],
                "properties": {
                    "type": {"const": "Action"},
                    "do": {"$ref": "#/definitions/ActionExpr"}
                }
            },
            "ConditionExpr": {
                "type": "object",
                "description": "Condition expression (keyPressed, playerDistance, etc.)"
            },
            "ActionExpr": {
                "type": "object",
                "description": "Action expression (moveForward, chase, patrol, etc.)"
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionExpr, ConditionExpr};

    #[test]
    fn behavior_schema_validates_empty_behaviors() {
        let schema = BehaviorSchema {
            entity: "Test".to_string(),
            description: None,
            behaviors: vec![],
        };

        assert!(schema.validate().is_err());
    }

    #[test]
    fn behavior_schema_validates_valid_tree() {
        let schema = BehaviorSchema::new(
            "Test",
            vec![BehaviorNode::Sequence {
                name: None,
                children: vec![
                    BehaviorNode::Condition {
                        check: ConditionExpr::KeyPressed {
                            key: "W".to_string(),
                        },
                    },
                    BehaviorNode::Action {
                        action: ActionExpr::MoveForward { speed: 1.0 },
                    },
                ],
            }],
        );

        assert!(schema.validate().is_ok());
    }

    #[test]
    fn behavior_schema_rejects_empty_sequence() {
        let schema = BehaviorSchema::new(
            "Test",
            vec![BehaviorNode::Sequence {
                name: None,
                children: vec![],
            }],
        );

        assert!(schema.validate().is_err());
    }

    #[test]
    fn behavior_schema_serializes_to_json() {
        let schema = BehaviorSchema::new(
            "Enemy",
            vec![BehaviorNode::Action {
                action: ActionExpr::Idle,
            }],
        )
        .with_description("Simple idle behavior");

        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("Enemy"));
        assert!(json.contains("Simple idle behavior"));
    }

    #[test]
    fn json_schema_generation() {
        let schema = generate_json_schema();
        assert_eq!(schema["title"], "AsterBehaviorSchema");
        assert!(schema["definitions"]["BehaviorNode"].is_object());
    }
}
