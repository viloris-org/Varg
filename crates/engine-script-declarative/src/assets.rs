//! Asset manifest and resource management.
//!
//! Declarative asset list and loading configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Asset manifest (list of all resources used in the project).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetManifest {
    /// All models/meshes.
    #[serde(default)]
    pub models: Vec<AssetRef>,

    /// All textures/images.
    #[serde(default)]
    pub textures: Vec<AssetRef>,

    /// All audio files.
    #[serde(default)]
    pub audio: Vec<AssetRef>,

    /// All scripts/behaviors.
    #[serde(default)]
    pub scripts: Vec<AssetRef>,

    /// All scenes.
    #[serde(default)]
    pub scenes: Vec<AssetRef>,

    /// Prefabs (reusable entity templates).
    #[serde(default)]
    pub prefabs: Vec<AssetRef>,

    /// Custom asset categories.
    #[serde(default)]
    pub custom: HashMap<String, Vec<AssetRef>>,
}

/// Reference to a single asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRef {
    /// Asset identifier/name.
    pub id: String,

    /// File path (relative to project root).
    pub path: String,

    /// Asset metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<AssetMeta>,

    /// Loading strategy.
    #[serde(default)]
    pub loading: LoadingStrategy,
}

/// Asset metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Size in bytes (optional, for preloading estimation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Dependencies (other assets this depends on).
    #[serde(default)]
    pub dependencies: Vec<String>,

    /// Custom metadata fields.
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

/// Asset loading strategy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum LoadingStrategy {
    /// Load immediately at startup.
    #[default]
    Preload,
    /// Load when first used.
    Lazy,
    /// Stream progressively (for large assets).
    Streaming,
}

/// Procedural generation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralConfig {
    /// Seed for random generation.
    pub seed: u64,

    /// Algorithm type.
    pub algorithm: String,

    /// Algorithm-specific parameters.
    #[serde(default)]
    pub params: HashMap<String, ProceduralParam>,
}

/// Parameter for procedural generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProceduralParam {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<f64>),
}

/// Prefab definition (reusable entity template).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefabSchema {
    /// Prefab name/identifier.
    pub name: String,

    /// Base object structure (from scene system).
    pub template: crate::scene::Object3D,

    /// Default behavior (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_behavior: Option<String>,

    /// Variants (named variations of this prefab).
    #[serde(default)]
    pub variants: HashMap<String, PrefabVariant>,
}

/// Prefab variant (modification of base prefab).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefabVariant {
    /// Variant name.
    pub name: String,

    /// Overrides to apply.
    pub overrides: HashMap<String, serde_json::Value>,
}

impl AssetManifest {
    /// Creates an empty asset manifest.
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            textures: Vec::new(),
            audio: Vec::new(),
            scripts: Vec::new(),
            scenes: Vec::new(),
            prefabs: Vec::new(),
            custom: HashMap::new(),
        }
    }

    /// Adds an asset to the manifest.
    pub fn add_asset(&mut self, category: &str, asset: AssetRef) {
        match category {
            "model" | "models" => self.models.push(asset),
            "texture" | "textures" => self.textures.push(asset),
            "audio" | "sound" | "sounds" => self.audio.push(asset),
            "script" | "scripts" => self.scripts.push(asset),
            "scene" | "scenes" => self.scenes.push(asset),
            "prefab" | "prefabs" => self.prefabs.push(asset),
            _ => {
                self.custom
                    .entry(category.to_string())
                    .or_insert_with(Vec::new)
                    .push(asset);
            }
        }
    }

    /// Validates the asset manifest.
    pub fn validate(&self) -> Result<(), String> {
        // Check for duplicate IDs
        let mut seen_ids = std::collections::HashSet::new();

        let all_assets = self
            .models
            .iter()
            .chain(&self.textures)
            .chain(&self.audio)
            .chain(&self.scripts)
            .chain(&self.scenes)
            .chain(&self.prefabs)
            .chain(self.custom.values().flatten());

        for asset in all_assets {
            if !seen_ids.insert(&asset.id) {
                return Err(format!("Duplicate asset ID: {}", asset.id));
            }
        }

        Ok(())
    }
}

impl Default for AssetManifest {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_manifest_creates_empty() {
        let manifest = AssetManifest::new();
        assert!(manifest.models.is_empty());
        assert!(manifest.textures.is_empty());
    }

    #[test]
    fn asset_manifest_adds_asset() {
        let mut manifest = AssetManifest::new();
        manifest.add_asset(
            "models",
            AssetRef {
                id: "player_model".to_string(),
                path: "models/player.gltf".to_string(),
                meta: None,
                loading: LoadingStrategy::Preload,
            },
        );

        assert_eq!(manifest.models.len(), 1);
        assert_eq!(manifest.models[0].id, "player_model");
    }

    #[test]
    fn asset_manifest_validates() {
        let manifest = AssetManifest {
            models: vec![AssetRef {
                id: "test".to_string(),
                path: "test.gltf".to_string(),
                meta: None,
                loading: LoadingStrategy::Preload,
            }],
            textures: vec![],
            audio: vec![],
            scripts: vec![],
            scenes: vec![],
            prefabs: vec![],
            custom: HashMap::new(),
        };

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn asset_manifest_detects_duplicates() {
        let manifest = AssetManifest {
            models: vec![
                AssetRef {
                    id: "test".to_string(),
                    path: "test1.gltf".to_string(),
                    meta: None,
                    loading: LoadingStrategy::Preload,
                },
                AssetRef {
                    id: "test".to_string(),
                    path: "test2.gltf".to_string(),
                    meta: None,
                    loading: LoadingStrategy::Preload,
                },
            ],
            textures: vec![],
            audio: vec![],
            scripts: vec![],
            scenes: vec![],
            prefabs: vec![],
            custom: HashMap::new(),
        };

        assert!(manifest.validate().is_err());
    }
}
