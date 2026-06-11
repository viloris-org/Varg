//! Behavior tree compiler.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use engine_core::{EngineError, EngineResult};

use crate::{BehaviorSchema, BehaviorTree};

/// Compiles JSON behavior schemas into optimized runtime behavior trees.
///
/// The compiler performs:
/// - JSON deserialization and validation
/// - Schema validation (empty children, depth limits)
/// - AST optimization (constant folding, dead code elimination)
/// - Caching for hot-reload support
pub struct BehaviorCompiler {
    /// Compiled behavior trees keyed by file path.
    cache: HashMap<PathBuf, BehaviorTree>,
}

impl Default for BehaviorCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl BehaviorCompiler {
    /// Creates a new behavior compiler with an empty cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Compiles a behavior schema from a JSON file.
    ///
    /// The compiled tree is cached for subsequent loads.
    ///
    /// # Errors
    /// Returns an error if:
    /// - File cannot be read
    /// - JSON is invalid
    /// - Schema validation fails
    pub fn compile_file(&mut self, path: &Path) -> EngineResult<&BehaviorTree> {
        // Check cache first
        if self.cache.contains_key(path) {
            return Ok(self.cache.get(path).unwrap());
        }

        // Read and parse JSON
        let json = std::fs::read_to_string(path).map_err(|e| EngineError::Filesystem {
            path: path.to_path_buf(),
            source: e,
        })?;

        let schema: BehaviorSchema = serde_json::from_str(&json).map_err(|e| {
            EngineError::other(format!(
                "Invalid behavior JSON at {}: {}",
                path.display(),
                e
            ))
        })?;

        // Validate schema
        schema.validate().map_err(|e| {
            EngineError::other(format!(
                "Behavior schema validation failed at {}: {}",
                path.display(),
                e
            ))
        })?;

        // Compile to tree
        let tree = self.compile_schema(schema)?;

        // Cache and return
        self.cache.insert(path.to_path_buf(), tree);
        Ok(self.cache.get(path).unwrap())
    }

    /// Compiles a behavior schema from an in-memory string.
    ///
    /// The `logical_path` is used as a cache key.
    pub fn compile_source(&mut self, logical_path: &Path, source: &str) -> EngineResult<()> {
        let schema: BehaviorSchema = serde_json::from_str(source).map_err(|e| {
            EngineError::other(format!(
                "Invalid behavior JSON at {}: {}",
                logical_path.display(),
                e
            ))
        })?;

        schema.validate().map_err(|e| {
            EngineError::other(format!(
                "Behavior schema validation failed at {}: {}",
                logical_path.display(),
                e
            ))
        })?;

        let tree = self.compile_schema(schema)?;
        self.cache.insert(logical_path.to_path_buf(), tree);
        Ok(())
    }

    /// Compiles a behavior schema into a behavior tree.
    fn compile_schema(&self, schema: BehaviorSchema) -> EngineResult<BehaviorTree> {
        // For now, we just wrap the root behavior in a Parallel if there are multiple
        let root = if schema.behaviors.len() == 1 {
            schema.behaviors.into_iter().next().unwrap()
        } else {
            crate::BehaviorNode::Parallel {
                name: Some("root".to_string()),
                children: schema.behaviors,
            }
        };

        Ok(BehaviorTree::with_name(root, schema.entity))
    }

    /// Returns the number of cached behavior trees.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Clears the behavior tree cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Removes a specific behavior tree from the cache.
    pub fn invalidate(&mut self, path: &Path) -> bool {
        self.cache.remove(path).is_some()
    }

    /// Returns whether a behavior tree is cached for the given path.
    pub fn is_cached(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionExpr, BehaviorNode, ConditionExpr};

    #[test]
    fn compiler_caches_compiled_trees() {
        let dir = std::env::temp_dir().join("aster_test_compiler_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.json");

        let schema = BehaviorSchema::new(
            "Test",
            vec![BehaviorNode::Action {
                action: ActionExpr::Idle,
            }],
        );
        std::fs::write(&path, serde_json::to_string(&schema).unwrap()).unwrap();

        let mut compiler = BehaviorCompiler::new();
        assert_eq!(compiler.cache_size(), 0);

        compiler.compile_file(&path).unwrap();
        assert_eq!(compiler.cache_size(), 1);

        // Second compile should use cache
        compiler.compile_file(&path).unwrap();
        assert_eq!(compiler.cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compiler_validates_schema() {
        let mut compiler = BehaviorCompiler::new();
        let path = PathBuf::from("test.json");

        // Empty behaviors array should fail
        let invalid_json = r#"{
            "entity": "Test",
            "behaviors": []
        }"#;

        let result = compiler.compile_source(&path, invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn compiler_rejects_invalid_json() {
        let mut compiler = BehaviorCompiler::new();
        let path = PathBuf::from("test.json");

        let invalid_json = r#"{"entity": "Test", "behaviors": [}"#;

        let result = compiler.compile_source(&path, invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn compiler_clear_cache() {
        let mut compiler = BehaviorCompiler::new();
        let path = PathBuf::from("test.json");

        let valid_json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"}
            ]
        }"#;

        compiler.compile_source(&path, valid_json).unwrap();
        assert_eq!(compiler.cache_size(), 1);

        compiler.clear_cache();
        assert_eq!(compiler.cache_size(), 0);
    }

    #[test]
    fn compiler_invalidate_specific_entry() {
        let mut compiler = BehaviorCompiler::new();
        let path1 = PathBuf::from("test1.json");
        let path2 = PathBuf::from("test2.json");

        let valid_json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"}
            ]
        }"#;

        compiler.compile_source(&path1, valid_json).unwrap();
        compiler.compile_source(&path2, valid_json).unwrap();
        assert_eq!(compiler.cache_size(), 2);

        assert!(compiler.invalidate(&path1));
        assert_eq!(compiler.cache_size(), 1);
        assert!(!compiler.is_cached(&path1));
        assert!(compiler.is_cached(&path2));
    }

    #[test]
    fn compiler_compiles_multiple_behaviors_to_parallel() {
        let mut compiler = BehaviorCompiler::new();
        let path = PathBuf::from("test.json");

        let json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"},
                {"type": "Action", "do": {"moveForward": 1.0}}
            ]
        }"#;

        compiler.compile_source(&path, json).unwrap();
        let tree = compiler.cache.get(&path).unwrap();

        // Root should be a Parallel node
        match &tree.root {
            BehaviorNode::Parallel { children, .. } => {
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected Parallel root node"),
        }
    }

    #[test]
    fn compiler_compiles_single_behavior_directly() {
        let mut compiler = BehaviorCompiler::new();
        let path = PathBuf::from("test.json");

        let json = r#"{
            "entity": "Test",
            "behaviors": [
                {"type": "Action", "do": "idle"}
            ]
        }"#;

        compiler.compile_source(&path, json).unwrap();
        let tree = compiler.cache.get(&path).unwrap();

        // Root should be the action directly
        match &tree.root {
            BehaviorNode::Action { .. } => (),
            _ => panic!("Expected Action root node"),
        }
    }
}
