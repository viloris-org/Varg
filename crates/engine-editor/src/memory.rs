//! AI-first context services: project memory, user memory, and dependency graph.
//!
//! These services provide persistent context that enables the AI agent to
//! understand the project holistically, remember user preferences, and
//! reason about entity/component relationships.

use std::fs;
use std::path::{Path, PathBuf};

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

// ─── ProjectMemory ────────────────────────────────────────────────────────────

/// Persistent project-level context stored in `.aster/project.md`.
///
/// This file is AI-maintained (and human-editable) and contains:
/// - Project description and goals
/// - Current progress and milestones
/// - Technical decisions
/// - Known issues and TODOs
///
/// It should be committed to version control so all team members
/// share the same project understanding.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectMemory {
    /// Root directory of the project.
    project_root: PathBuf,
    /// Cached content of project.md.
    content: String,
    /// Whether the cached content has been modified since last save.
    dirty: bool,
}

impl ProjectMemory {
    /// Path to the project memory file relative to project root.
    const FILE_PATH: &'static str = ".aster/project.md";

    /// Creates a new ProjectMemory bound to a project root.
    ///
    /// Loads existing content if the file exists, otherwise starts empty.
    pub fn open(project_root: &Path) -> Self {
        let file_path = project_root.join(Self::FILE_PATH);
        let content = fs::read_to_string(&file_path).unwrap_or_default();
        Self {
            project_root: project_root.to_path_buf(),
            content,
            dirty: false,
        }
    }

    /// Returns the full path to the project.md file.
    pub fn file_path(&self) -> PathBuf {
        self.project_root.join(Self::FILE_PATH)
    }

    /// Returns the current content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns whether the content is non-empty.
    pub fn is_populated(&self) -> bool {
        !self.content.trim().is_empty()
    }

    /// Replaces the entire content.
    pub fn set_content(&mut self, content: String) {
        if self.content != content {
            self.content = content;
            self.dirty = true;
        }
    }

    /// Appends a section to the content.
    pub fn append_section(&mut self, heading: &str, body: &str) {
        if !self.content.is_empty() && !self.content.ends_with('\n') {
            self.content.push('\n');
        }
        self.content
            .push_str(&format!("\n## {heading}\n\n{body}\n"));
        self.dirty = true;
    }

    /// Persists the content to disk using atomic write.
    pub fn save(&mut self) -> EngineResult<()> {
        if !self.dirty {
            return Ok(());
        }
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let tmp_path = path.with_extension("md.tmp");
        fs::write(&tmp_path, &self.content).map_err(|source| EngineError::Filesystem {
            path: tmp_path.clone(),
            source,
        })?;
        fs::rename(&tmp_path, &path).map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
        self.dirty = false;
        Ok(())
    }

    /// Reloads content from disk, discarding unsaved changes.
    pub fn reload(&mut self) {
        let file_path = self.file_path();
        self.content = fs::read_to_string(&file_path).unwrap_or_default();
        self.dirty = false;
    }

    /// Serializes the project memory as a context value for AI injection.
    pub fn to_ai_context(&self) -> serde_json::Value {
        serde_json::json!({
            "project_memory": {
                "populated": self.is_populated(),
                "content": self.content,
            }
        })
    }
}

// ─── UserMemory ──────────────────────────────────────────────────────────────

/// User-specific habit and preference memory stored in `.aster/memory.md`.
///
/// This file is NOT committed to version control (should be in .gitignore).
/// The AI agent observes user patterns and updates this file to personalize
/// future interactions.
///
/// Tracked patterns include:
/// - Naming conventions (camelCase vs snake_case)
/// - Common component combinations
/// - Style preferences (lighting, materials)
/// - Interaction habits (frequent operations)
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserMemory {
    /// Root directory of the project.
    project_root: PathBuf,
    /// Cached content of memory.md.
    content: String,
    /// Structured entries parsed from the file.
    entries: Vec<MemoryEntry>,
    /// Whether modifications have been made since last save.
    dirty: bool,
}

/// A single memory entry with a key and observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Category key (e.g. "naming", "style", "workflow").
    pub key: String,
    /// The observed pattern or preference.
    pub value: String,
    /// Confidence level (0-100, higher = more observations).
    pub confidence: u8,
}

impl UserMemory {
    /// Path to the user memory file relative to project root.
    const FILE_PATH: &'static str = ".aster/memory.md";

    /// Creates a new UserMemory bound to a project root.
    pub fn open(project_root: &Path) -> Self {
        let file_path = project_root.join(Self::FILE_PATH);
        let content = fs::read_to_string(&file_path).unwrap_or_default();
        let entries = Self::parse_entries(&content);
        Self {
            project_root: project_root.to_path_buf(),
            content,
            entries,
            dirty: false,
        }
    }

    /// Returns the full path to the memory.md file.
    pub fn file_path(&self) -> PathBuf {
        self.project_root.join(Self::FILE_PATH)
    }

    /// Returns all memory entries.
    pub fn entries(&self) -> &[MemoryEntry] {
        &self.entries
    }

    /// Returns the raw markdown content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Looks up a memory entry by key.
    pub fn get(&self, key: &str) -> Option<&MemoryEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Upserts a memory entry. If the key exists, updates the value and bumps confidence.
    pub fn upsert(&mut self, key: &str, value: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.key == key) {
            entry.value = value.to_string();
            entry.confidence = entry.confidence.saturating_add(1).min(100);
        } else {
            self.entries.push(MemoryEntry {
                key: key.to_string(),
                value: value.to_string(),
                confidence: 1,
            });
        }
        self.rebuild_content();
        self.dirty = true;
    }

    /// Removes a memory entry by key.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        if self.entries.len() != before {
            self.rebuild_content();
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Persists the memory to disk using atomic write.
    pub fn save(&mut self) -> EngineResult<()> {
        if !self.dirty {
            return Ok(());
        }
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let tmp_path = path.with_extension("md.tmp");
        fs::write(&tmp_path, &self.content).map_err(|source| EngineError::Filesystem {
            path: tmp_path.clone(),
            source,
        })?;
        fs::rename(&tmp_path, &path).map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
        self.dirty = false;
        Ok(())
    }

    /// Reloads from disk, discarding unsaved changes.
    pub fn reload(&mut self) {
        let file_path = self.file_path();
        self.content = fs::read_to_string(&file_path).unwrap_or_default();
        self.entries = Self::parse_entries(&self.content);
        self.dirty = false;
    }

    /// Serializes user memory as a context value for AI injection.
    pub fn to_ai_context(&self) -> serde_json::Value {
        serde_json::json!({
            "user_memory": {
                "entries": self.entries,
            }
        })
    }

    /// Parses structured entries from markdown content.
    ///
    /// Expected format:
    /// ```markdown
    /// ## Memory
    ///
    /// - **key**: value (confidence: N)
    /// ```
    fn parse_entries(content: &str) -> Vec<MemoryEntry> {
        let mut entries = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            // Match lines like: - **key**: value (confidence: N)
            if let Some(rest) = trimmed.strip_prefix("- **") {
                if let Some(key_end) = rest.find("**:") {
                    let key = &rest[..key_end];
                    let after_key = &rest[key_end + 3..];
                    // Extract confidence if present
                    let (value, confidence) =
                        if let Some(conf_start) = after_key.rfind("(confidence:") {
                            let val = after_key[..conf_start].trim();
                            let conf_str = &after_key[conf_start + 12..];
                            let conf = conf_str
                                .trim_end_matches(')')
                                .trim()
                                .parse::<u8>()
                                .unwrap_or(1);
                            (val, conf)
                        } else {
                            (after_key.trim(), 1u8)
                        };
                    entries.push(MemoryEntry {
                        key: key.to_string(),
                        value: value.to_string(),
                        confidence,
                    });
                }
            }
        }
        entries
    }

    /// Rebuilds the markdown content from structured entries.
    fn rebuild_content(&mut self) {
        let mut out = String::from("# User Memory\n\nAI-observed patterns and preferences.\n\n");
        for entry in &self.entries {
            out.push_str(&format!(
                "- **{}**: {} (confidence: {})\n",
                entry.key, entry.value, entry.confidence
            ));
        }
        self.content = out;
    }
}

// ─── DependencyGraph ─────────────────────────────────────────────────────────

/// A graph of relationships between entities, components, assets, and scripts.
///
/// Built on demand from the current Scene and AssetDatabase, this graph
/// helps the AI agent understand how elements in the project relate to
/// each other, enabling more contextual operations.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// Nodes in the graph.
    pub nodes: Vec<GraphNode>,
    /// Edges between nodes.
    pub edges: Vec<GraphEdge>,
}

/// A node in the dependency graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique node identifier (e.g. "entity:1:1", "asset:textures/player.png").
    pub id: String,
    /// Node type for grouping and filtering.
    pub kind: NodeKind,
    /// Display label.
    pub label: String,
}

/// Classification of graph nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A scene entity (GameObject).
    Entity,
    /// A component attached to an entity.
    Component,
    /// An asset file (texture, mesh, audio, script).
    Asset,
    /// A script file.
    Script,
}

/// An edge representing a relationship between two nodes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node ID.
    pub from: String,
    /// Target node ID.
    pub to: String,
    /// Relationship type.
    pub relation: EdgeRelation,
}

/// Types of relationships in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRelation {
    /// Parent-child hierarchy.
    ParentChild,
    /// Entity has this component.
    HasComponent,
    /// Component references this asset.
    ReferencesAsset,
    /// Script controls this entity.
    ScriptControls,
    /// Asset depends on another asset.
    AssetDependency,
}

impl DependencyGraph {
    /// Builds a dependency graph from a Scene.
    ///
    /// Extracts entity hierarchy, component attachments, script references,
    /// and asset dependencies into a flat graph structure suitable for
    /// AI context injection.
    pub fn from_scene(scene: &engine_ecs::Scene) -> Self {
        let mut graph = Self::default();

        for (entity, obj) in scene.objects() {
            let entity_id = format!(
                "entity:{}:{}",
                entity.handle().slot(),
                entity.handle().generation().get()
            );
            graph.nodes.push(GraphNode {
                id: entity_id.clone(),
                kind: NodeKind::Entity,
                label: obj.name.clone(),
            });

            // Parent-child edges
            if let Some(parent) = scene.transforms().parent(entity) {
                let parent_id = format!(
                    "entity:{}:{}",
                    parent.handle().slot(),
                    parent.handle().generation().get()
                );
                graph.edges.push(GraphEdge {
                    from: parent_id,
                    to: entity_id.clone(),
                    relation: EdgeRelation::ParentChild,
                });
            }

            // Component edges from the components vec
            for component in &obj.components {
                let comp_id = format!("{}:{}", entity_id, component.type_id());
                graph.nodes.push(GraphNode {
                    id: comp_id.clone(),
                    kind: NodeKind::Component,
                    label: component.type_id().to_string(),
                });
                graph.edges.push(GraphEdge {
                    from: entity_id.clone(),
                    to: comp_id.clone(),
                    relation: EdgeRelation::HasComponent,
                });

                // Script component references
                if let engine_ecs::ComponentData::Script(proxy) = component {
                    if !proxy.script.is_empty() {
                        let script_id = format!("script:{}", proxy.script);
                        if !graph.nodes.iter().any(|n| n.id == script_id) {
                            graph.nodes.push(GraphNode {
                                id: script_id.clone(),
                                kind: NodeKind::Script,
                                label: proxy.script.clone(),
                            });
                        }
                        graph.edges.push(GraphEdge {
                            from: script_id,
                            to: entity_id.clone(),
                            relation: EdgeRelation::ScriptControls,
                        });
                    }
                }
            }

            // Script edges from the scripts vec (legacy format)
            for script in &obj.scripts {
                if !script.script.is_empty() {
                    let script_id = format!("script:{}", script.script);
                    if !graph.nodes.iter().any(|n| n.id == script_id) {
                        graph.nodes.push(GraphNode {
                            id: script_id.clone(),
                            kind: NodeKind::Script,
                            label: script.script.clone(),
                        });
                    }
                    let edge_exists = graph.edges.iter().any(|e| {
                        e.from == script_id
                            && e.to == entity_id
                            && e.relation == EdgeRelation::ScriptControls
                    });
                    if !edge_exists {
                        graph.edges.push(GraphEdge {
                            from: script_id,
                            to: entity_id.clone(),
                            relation: EdgeRelation::ScriptControls,
                        });
                    }
                }
            }
        }

        graph
    }

    /// Returns all nodes of a given kind.
    pub fn nodes_of_kind(&self, kind: NodeKind) -> Vec<&GraphNode> {
        self.nodes.iter().filter(|n| n.kind == kind).collect()
    }

    /// Returns edges connected to a specific node.
    pub fn edges_for(&self, node_id: &str) -> Vec<&GraphEdge> {
        self.edges
            .iter()
            .filter(|e| e.from == node_id || e.to == node_id)
            .collect()
    }

    /// Serializes the graph as a context value for AI injection.
    pub fn to_ai_context(&self) -> serde_json::Value {
        serde_json::json!({
            "dependency_graph": {
                "node_count": self.nodes.len(),
                "edge_count": self.edges.len(),
                "entities": self.nodes_of_kind(NodeKind::Entity).len(),
                "scripts": self.nodes_of_kind(NodeKind::Script).len(),
                "nodes": self.nodes,
                "edges": self.edges,
            }
        })
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn project_memory_roundtrip() {
        let tmp = env::temp_dir().join("aster_test_project_memory");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut mem = ProjectMemory::open(&tmp);
        assert!(!mem.is_populated());

        mem.set_content("# My Project\n\nA cool game.".to_string());
        assert!(mem.is_populated());
        mem.save().unwrap();

        let reloaded = ProjectMemory::open(&tmp);
        assert_eq!(reloaded.content(), "# My Project\n\nA cool game.");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn project_memory_append_section() {
        let tmp = env::temp_dir().join("aster_test_project_memory_append");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut mem = ProjectMemory::open(&tmp);
        mem.set_content("# Project".to_string());
        mem.append_section("Progress", "Implemented player movement.");
        assert!(mem.content().contains("## Progress"));
        assert!(mem.content().contains("Implemented player movement."));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn user_memory_upsert_and_parse() {
        let tmp = env::temp_dir().join("aster_test_user_memory");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut mem = UserMemory::open(&tmp);
        mem.upsert("naming", "prefers snake_case for entities");
        mem.upsert("style", "dark moody lighting");
        mem.save().unwrap();

        let reloaded = UserMemory::open(&tmp);
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(
            reloaded.get("naming").unwrap().value,
            "prefers snake_case for entities"
        );
        assert_eq!(reloaded.get("naming").unwrap().confidence, 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn user_memory_confidence_increases() {
        let tmp = env::temp_dir().join("aster_test_user_memory_conf");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut mem = UserMemory::open(&tmp);
        mem.upsert("naming", "snake_case");
        mem.upsert("naming", "snake_case confirmed");
        assert_eq!(mem.get("naming").unwrap().confidence, 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn user_memory_remove() {
        let tmp = env::temp_dir().join("aster_test_user_memory_rm");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut mem = UserMemory::open(&tmp);
        mem.upsert("key1", "val1");
        mem.upsert("key2", "val2");
        assert!(mem.remove("key1"));
        assert_eq!(mem.entries().len(), 1);
        assert!(mem.get("key1").is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dependency_graph_from_empty_scene() {
        let scene = engine_ecs::Scene::new();
        let graph = DependencyGraph::from_scene(&scene);
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
    }
}
