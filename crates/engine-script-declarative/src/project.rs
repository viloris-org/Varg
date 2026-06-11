//! Project structure and metadata.
//!
//! Top-level project configuration that ties everything together.

use serde::{Deserialize, Serialize};

use crate::{AssetManifest, SystemsConfig};

/// Complete project schema (top-level configuration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSchema {
    /// Project name.
    pub name: String,

    /// Project description.
    #[serde(default)]
    pub description: String,

    /// Game genre/category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,

    /// Art style.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art_style: Option<String>,

    /// Project version.
    #[serde(default = "default_version")]
    pub version: String,

    /// All scenes in the project.
    pub scenes: Vec<SceneRef>,

    /// Default/starting scene.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_scene: Option<String>,

    /// All UI layouts in the project.
    #[serde(default)]
    pub ui_layouts: Vec<UIRef>,

    /// Game systems configuration.
    #[serde(default)]
    pub systems: SystemsConfig,

    /// Asset manifest.
    #[serde(default)]
    pub assets: AssetManifest,

    /// Build settings.
    #[serde(default)]
    pub build: BuildSettings,

    /// Custom metadata.
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Reference to a scene file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRef {
    /// Scene name/identifier.
    pub name: String,

    /// Path to scene JSON file.
    pub path: String,

    /// Scene description (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Reference to a UI layout file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIRef {
    /// UI name/identifier.
    pub name: String,

    /// Path to UI JSON file.
    pub path: String,

    /// When to load this UI (e.g., "game", "menu", "always").
    #[serde(default)]
    pub context: String,
}

/// Build settings for the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSettings {
    /// Target platform(s).
    #[serde(default = "default_platforms")]
    pub platforms: Vec<Platform>,

    /// Build configuration.
    #[serde(default)]
    pub config: BuildConfig,

    /// Output directory.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,

    /// Optimization level.
    #[serde(default)]
    pub optimization: OptimizationLevel,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            platforms: default_platforms(),
            config: BuildConfig::default(),
            output_dir: default_output_dir(),
            optimization: OptimizationLevel::default(),
        }
    }
}

/// Target platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Platform {
    Windows,
    Linux,
    MacOS,
    Web,
    Android,
    IOS,
}

/// Build configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum BuildConfig {
    #[default]
    Debug,
    Release,
}

/// Optimization level.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum OptimizationLevel {
    None,
    #[default]
    Default,
    Aggressive,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_platforms() -> Vec<Platform> {
    vec![Platform::Windows, Platform::Linux, Platform::MacOS]
}

fn default_output_dir() -> String {
    "build".to_string()
}

impl ProjectSchema {
    /// Creates a new project schema with minimal configuration.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            genre: None,
            art_style: None,
            version: default_version(),
            scenes: Vec::new(),
            default_scene: None,
            ui_layouts: Vec::new(),
            systems: SystemsConfig {
                combat: None,
                economy: None,
                progression: None,
                physics: None,
                audio: None,
                custom: std::collections::HashMap::new(),
            },
            assets: AssetManifest::new(),
            build: BuildSettings::default(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Adds a scene to the project.
    pub fn add_scene(&mut self, name: impl Into<String>, path: impl Into<String>) {
        self.scenes.push(SceneRef {
            name: name.into(),
            path: path.into(),
            description: None,
            tags: Vec::new(),
        });
    }

    /// Adds a UI layout to the project.
    pub fn add_ui(
        &mut self,
        name: impl Into<String>,
        path: impl Into<String>,
        context: impl Into<String>,
    ) {
        self.ui_layouts.push(UIRef {
            name: name.into(),
            path: path.into(),
            context: context.into(),
        });
    }

    /// Validates the project schema.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Project name cannot be empty".to_string());
        }

        if self.scenes.is_empty() {
            return Err("Project must have at least one scene".to_string());
        }

        // Validate systems config
        self.systems.validate()?;

        // Validate asset manifest
        self.assets.validate()?;

        // Check default scene exists
        if let Some(default) = &self.default_scene {
            if !self.scenes.iter().any(|s| &s.name == default) {
                return Err(format!(
                    "Default scene '{}' not found in scenes list",
                    default
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_schema_creates() {
        let project = ProjectSchema::new("TestProject");
        assert_eq!(project.name, "TestProject");
    }

    #[test]
    fn project_schema_adds_scene() {
        let mut project = ProjectSchema::new("TestProject");
        project.add_scene("Level1", "scenes/level1.json");

        assert_eq!(project.scenes.len(), 1);
        assert_eq!(project.scenes[0].name, "Level1");
    }

    #[test]
    fn project_schema_validates_empty_scenes() {
        let project = ProjectSchema::new("TestProject");
        assert!(project.validate().is_err());
    }

    #[test]
    fn project_schema_validates_with_scene() {
        let mut project = ProjectSchema::new("TestProject");
        project.add_scene("Level1", "scenes/level1.json");
        assert!(project.validate().is_ok());
    }

    #[test]
    fn project_schema_serializes() {
        let mut project = ProjectSchema::new("TowerDefense");
        project.description = "赛博朋克塔防游戏".to_string();
        project.genre = Some("tower_defense".to_string());
        project.add_scene("MainMenu", "scenes/main_menu.json");
        project.add_scene("Level1", "scenes/level1.json");
        project.default_scene = Some("MainMenu".to_string());

        let json = serde_json::to_string_pretty(&project).unwrap();
        assert!(json.contains("TowerDefense"));
        assert!(json.contains("MainMenu"));
    }
}
