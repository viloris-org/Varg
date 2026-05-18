//! Project, prefab, editor preference, and build configuration schemas.

use engine_core::{EngineError, EngineResult};

use crate::scene::{SceneFile, SCENE_FILE_VERSION};

/// Current project manifest schema version.
pub const PROJECT_MANIFEST_VERSION: u32 = 1;
/// Current prefab schema version.
pub const PREFAB_FILE_VERSION: u32 = 1;
/// Current editor preferences schema version.
pub const EDITOR_PREFERENCES_VERSION: u32 = 1;
/// Current build configuration schema version.
pub const BUILD_CONFIGURATION_VERSION: u32 = 1;

/// Version metadata embedded in every data format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FormatVersion {
    /// Explicit schema version.
    pub version: u32,
}

impl FormatVersion {
    /// Creates version metadata.
    pub const fn new(version: u32) -> Self {
        Self { version }
    }
}

/// Schema evolution policy stored with data format definitions.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SchemaEvolution {
    /// Human-readable compatibility policy.
    pub policy: String,
    /// Whether unknown fields should be preserved by higher-level tooling.
    pub forward_compatible_read: bool,
    /// Whether migrations are available for this format family.
    pub migration_framework: Option<String>,
}

impl Default for SchemaEvolution {
    fn default() -> Self {
        Self {
            policy: "minor versions may add optional fields; major schema bumps require migration"
                .to_string(),
            forward_compatible_read: true,
            migration_framework: Some(
                "versioned Rust migrators keyed by explicit version".to_string(),
            ),
        }
    }
}

/// Format validation diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FormatDiagnostic {
    /// JSON/TOML path or logical field.
    pub path: String,
    /// Human-readable failure reason.
    pub message: String,
}

/// Project manifest format.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProjectManifest {
    /// Explicit schema version.
    pub format: FormatVersion,
    /// Project display name.
    pub name: String,
    /// Asset root relative to the project file.
    pub asset_root: String,
    /// Default scene path.
    pub default_scene: String,
    /// Schema evolution metadata.
    pub evolution: SchemaEvolution,
}

impl ProjectManifest {
    /// Creates an example project manifest.
    pub fn example() -> Self {
        Self {
            format: FormatVersion::new(PROJECT_MANIFEST_VERSION),
            name: "Aster Example".to_string(),
            asset_root: "assets".to_string(),
            default_scene: "scenes/example.aster_scene.json".to_string(),
            evolution: SchemaEvolution::default(),
        }
    }

    /// Validates manifest fields and returns diagnostics.
    pub fn diagnostics(&self) -> Vec<FormatDiagnostic> {
        let mut diagnostics = Vec::new();
        if self.format.version > PROJECT_MANIFEST_VERSION {
            diagnostics.push(FormatDiagnostic {
                path: "format.version".to_string(),
                message: format!(
                    "project manifest version {} is newer than supported version {}",
                    self.format.version, PROJECT_MANIFEST_VERSION
                ),
            });
        }
        if self.name.trim().is_empty() {
            diagnostics.push(FormatDiagnostic {
                path: "name".to_string(),
                message: "project name cannot be empty".to_string(),
            });
        }
        if self.default_scene.trim().is_empty() {
            diagnostics.push(FormatDiagnostic {
                path: "default_scene".to_string(),
                message: "default scene path cannot be empty".to_string(),
            });
        }
        diagnostics
    }

    /// Serializes the manifest to TOML.
    pub fn to_toml(&self) -> EngineResult<String> {
        toml::to_string_pretty(self).map_err(|error| {
            EngineError::other(format!("project manifest serialization failed: {error}"))
        })
    }
}

/// Prefab file format.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PrefabFile {
    /// Explicit schema version.
    pub format: FormatVersion,
    /// Prefab display name.
    pub name: String,
    /// Scene object subset contained by this prefab.
    pub scene: SceneFile,
    /// Schema evolution metadata.
    pub evolution: SchemaEvolution,
}

impl PrefabFile {
    /// Creates a prefab from a scene file subset.
    pub fn new(name: impl Into<String>, scene: SceneFile) -> Self {
        Self {
            format: FormatVersion::new(PREFAB_FILE_VERSION),
            name: name.into(),
            scene,
            evolution: SchemaEvolution::default(),
        }
    }

    /// Validates prefab fields and returns diagnostics.
    pub fn diagnostics(&self) -> Vec<FormatDiagnostic> {
        let mut diagnostics = Vec::new();
        if self.format.version > PREFAB_FILE_VERSION {
            diagnostics.push(FormatDiagnostic {
                path: "format.version".to_string(),
                message: format!(
                    "prefab version {} is newer than supported version {}",
                    self.format.version, PREFAB_FILE_VERSION
                ),
            });
        }
        if self.scene.version > SCENE_FILE_VERSION {
            diagnostics.push(FormatDiagnostic {
                path: "scene.version".to_string(),
                message: format!(
                    "embedded scene version {} is newer than supported version {}",
                    self.scene.version, SCENE_FILE_VERSION
                ),
            });
        }
        diagnostics
    }
}

/// Editor preferences format.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct EditorPreferences {
    /// Explicit schema version.
    pub format: FormatVersion,
    /// Preferred UI theme name.
    pub theme: String,
    /// Whether to reopen the last project on startup.
    pub reopen_last_project: bool,
    /// Schema evolution metadata.
    pub evolution: SchemaEvolution,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            format: FormatVersion::new(EDITOR_PREFERENCES_VERSION),
            theme: "system".to_string(),
            reopen_last_project: true,
            evolution: SchemaEvolution::default(),
        }
    }
}

/// Build configuration format.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct BuildConfiguration {
    /// Explicit schema version.
    pub format: FormatVersion,
    /// Build target triple or platform alias.
    pub target: String,
    /// Whether to produce an optimized build.
    pub release: bool,
    /// Feature flags enabled for the build.
    pub features: Vec<String>,
    /// Schema evolution metadata.
    pub evolution: SchemaEvolution,
}

impl BuildConfiguration {
    /// Creates a minimal runtime build configuration.
    pub fn runtime_min() -> Self {
        Self {
            format: FormatVersion::new(BUILD_CONFIGURATION_VERSION),
            target: "native".to_string(),
            release: false,
            features: vec!["runtime-min".to_string()],
            evolution: SchemaEvolution::default(),
        }
    }

    /// Validates build configuration fields and returns diagnostics.
    pub fn diagnostics(&self) -> Vec<FormatDiagnostic> {
        let mut diagnostics = Vec::new();
        if self.format.version > BUILD_CONFIGURATION_VERSION {
            diagnostics.push(FormatDiagnostic {
                path: "format.version".to_string(),
                message: format!(
                    "build configuration version {} is newer than supported version {}",
                    self.format.version, BUILD_CONFIGURATION_VERSION
                ),
            });
        }
        if self.target.trim().is_empty() {
            diagnostics.push(FormatDiagnostic {
                path: "target".to_string(),
                message: "build target cannot be empty".to_string(),
            });
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_manifest_is_valid_and_serializable() {
        let manifest = ProjectManifest::example();

        assert!(manifest.diagnostics().is_empty());
        assert!(manifest.to_toml().unwrap().contains("Aster Example"));
    }

    #[test]
    fn build_config_reports_future_version() {
        let mut config = BuildConfiguration::runtime_min();
        config.format.version = BUILD_CONFIGURATION_VERSION + 1;

        assert_eq!(config.diagnostics().len(), 1);
    }

    #[test]
    fn example_files_parse() {
        let manifest = include_str!("../../../examples/project/aster.project.toml");
        let scene = include_str!("../../../examples/project/scenes/example.aster_scene.json");
        let prefab = include_str!("../../../examples/project/prefabs/player.aster_prefab.json");
        let preferences = include_str!("../../../examples/project/editor.preferences.toml");
        let build = include_str!("../../../examples/project/build.runtime-min.toml");

        let manifest = toml::from_str::<ProjectManifest>(manifest).unwrap();
        let scene = serde_json::from_str::<SceneFile>(scene).unwrap();
        let prefab = serde_json::from_str::<PrefabFile>(prefab).unwrap();
        let preferences = toml::from_str::<EditorPreferences>(preferences).unwrap();
        let build = toml::from_str::<BuildConfiguration>(build).unwrap();

        assert!(manifest.diagnostics().is_empty());
        assert_eq!(scene.objects.len(), 2);
        assert!(prefab.diagnostics().is_empty());
        assert_eq!(preferences.theme, "system");
        assert!(build.diagnostics().is_empty());
    }
}
