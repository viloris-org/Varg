//! Core behavior tree node types.

use serde::{Deserialize, Serialize};

use crate::{ActionExpr, ConditionExpr};

/// The result of executing a behavior tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeResult {
    /// The node succeeded.
    Success,
    /// The node failed.
    Failure,
    /// The node is still running (for multi-frame actions).
    Running,
}

/// A node in the behavior tree.
///
/// Behavior trees are composed of control flow nodes (Sequence, Selector, Parallel)
/// and leaf nodes (Condition, Action). This design is particularly LLM-friendly
/// because:
/// 1. The structure is hierarchical and matches JSON naturally
/// 2. Each node type has clear, predictable semantics
/// 3. Composition is explicit through nesting
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum BehaviorNode {
    /// Execute children in order until one fails.
    ///
    /// Semantics: Returns Success if all children succeed, Failure if any fails.
    /// Short-circuits on first failure.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Sequence",
    ///   "children": [
    ///     {"type": "Condition", "check": {"keyPressed": "W"}},
    ///     {"type": "Action", "do": {"moveForward": 1.0}}
    ///   ]
    /// }
    /// ```
    Sequence {
        /// Optional name for debugging.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Child nodes to execute in sequence.
        children: Vec<BehaviorNode>,
    },

    /// Execute children in order until one succeeds.
    ///
    /// Semantics: Returns Success if any child succeeds, Failure if all fail.
    /// Short-circuits on first success.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Selector",
    ///   "children": [
    ///     {"type": "Condition", "check": {"healthBelow": 20}},
    ///     {"type": "Action", "do": {"flee": true}}
    ///   ]
    /// }
    /// ```
    Selector {
        /// Optional name for debugging.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Child nodes to try in order.
        children: Vec<BehaviorNode>,
    },

    /// Execute all children concurrently.
    ///
    /// Semantics: Returns Success when all children succeed, Failure when any fails,
    /// Running while any child is still running.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Parallel",
    ///   "children": [
    ///     {"type": "Action", "do": {"patrol": {...}}},
    ///     {"type": "Action", "do": {"playIdleAnimation": true}}
    ///   ]
    /// }
    /// ```
    Parallel {
        /// Optional name for debugging.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Child nodes to execute in parallel.
        children: Vec<BehaviorNode>,
    },

    /// Check a condition without side effects.
    ///
    /// Semantics: Returns Success if condition is true, Failure otherwise.
    /// Always completes in one frame.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Condition",
    ///   "check": {
    ///     "playerDistance": {"lessThan": 5.0}
    ///   }
    /// }
    /// ```
    Condition {
        /// The condition expression to evaluate.
        check: ConditionExpr,
    },

    /// Perform an action with side effects.
    ///
    /// Semantics: Returns Success if action completes, Running if multi-frame,
    /// Failure if action fails.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Action",
    ///   "do": {
    ///     "chase": {
    ///       "target": "player",
    ///       "speed": 4.0
    ///     }
    ///   }
    /// }
    /// ```
    Action {
        /// The action to perform.
        #[serde(rename = "do")]
        action: ActionExpr,
    },

    /// Invert the result of the child node.
    ///
    /// Semantics: Success → Failure, Failure → Success, Running → Running.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Invert",
    ///   "child": {
    ///     "type": "Condition",
    ///     "check": {"keyPressed": "Space"}
    ///   }
    /// }
    /// ```
    Invert {
        /// The child node to invert.
        child: Box<BehaviorNode>,
    },

    /// Always succeed.
    ///
    /// Useful for forcing a branch to always return Success.
    Succeed {
        /// The child node to execute (result is ignored).
        child: Box<BehaviorNode>,
    },

    /// Repeat the child node N times or until it fails.
    ///
    /// # Example
    /// ```json
    /// {
    ///   "type": "Repeat",
    ///   "count": 3,
    ///   "child": {"type": "Action", "do": {"jump": true}}
    /// }
    /// ```
    Repeat {
        /// Number of times to repeat (None = infinite).
        #[serde(skip_serializing_if = "Option::is_none")]
        count: Option<u32>,
        /// The child node to repeat.
        child: Box<BehaviorNode>,
    },
}

/// A complete behavior tree for an entity.
///
/// This is the top-level structure that gets compiled from JSON and executed
/// per-frame for each entity.
#[derive(Debug, Clone)]
pub struct BehaviorTree {
    /// The root node of the tree.
    pub root: BehaviorNode,
    /// Optional name for debugging.
    pub name: Option<String>,
}

impl BehaviorTree {
    /// Creates a new behavior tree with the given root node.
    pub fn new(root: BehaviorNode) -> Self {
        Self { root, name: None }
    }

    /// Creates a new behavior tree with a name.
    pub fn with_name(root: BehaviorNode, name: impl Into<String>) -> Self {
        Self {
            root,
            name: Some(name.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_node_serializes_to_json() {
        let node = BehaviorNode::Sequence {
            name: Some("test".to_string()),
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
        };

        let json = serde_json::to_string_pretty(&node).unwrap();
        assert!(json.contains("Sequence"));
        assert!(json.contains("Condition"));
        assert!(json.contains("Action"));
    }

    #[test]
    fn behavior_node_deserializes_from_json() {
        let json = r#"{
            "type": "Sequence",
            "name": "test",
            "children": [
                {
                    "type": "Condition",
                    "check": {
                        "keyPressed": "W"
                    }
                },
                {
                    "type": "Action",
                    "do": {
                        "moveForward": 1.0
                    }
                }
            ]
        }"#;

        let node: BehaviorNode = serde_json::from_str(json).unwrap();
        match node {
            BehaviorNode::Sequence { name, children } => {
                assert_eq!(name, Some("test".to_string()));
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected Sequence node"),
        }
    }
}
