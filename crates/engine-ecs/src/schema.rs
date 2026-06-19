//! Project, prefab, editor preference, and build configuration schemas.

use std::{fs, path::Path};

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

/// Serializable component field kind used by editor Inspector generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ComponentFieldKind {
    /// Boolean value.
    Bool,
    /// 32-bit floating point value.
    F32,
    /// UTF-8 string value.
    String,
    /// 3D vector value.
    Vec3,
    /// Asset GUID reference.
    AssetRef,
    /// Nested object value.
    Object,
}

/// Component field metadata for Inspector and migration tooling.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ComponentFieldSchema {
    /// Serialized field name.
    pub name: String,
    /// Field data kind.
    pub kind: ComponentFieldKind,
    /// Human-readable default value.
    pub default_value: String,
}

/// Component type metadata for scene and prefab serialization.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ComponentSchema {
    /// Stable component type ID.
    pub type_id: String,
    /// Human-readable component name.
    pub display_name: String,
    /// Schema version for migration.
    pub version: u32,
    /// Field metadata.
    pub fields: Vec<ComponentFieldSchema>,
    /// Migration policy.
    pub evolution: SchemaEvolution,
}

/// Registry of serializable component schemas.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ComponentSchemaRegistry {
    schemas: Vec<ComponentSchema>,
}

impl ComponentSchemaRegistry {
    /// Builds the built-in runtime component schema registry.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry.register(ComponentSchema {
            type_id: "Camera".to_string(),
            display_name: "Camera".to_string(),
            version: 2,
            fields: vec![
                field("vertical_fov_degrees", ComponentFieldKind::F32, "60"),
                field("near", ComponentFieldKind::F32, "0.01"),
                field("far", ComponentFieldKind::F32, "1000"),
                field("primary", ComponentFieldKind::Bool, "true"),
                field("clear_color", ComponentFieldKind::Vec3, "0.1,0.1,0.1"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "MeshRenderer".to_string(),
            display_name: "Mesh Renderer".to_string(),
            version: 2,
            fields: vec![
                field("mesh", ComponentFieldKind::AssetRef, "debug/cube"),
                field("material", ComponentFieldKind::Object, "debug/default"),
                field("casts_shadows", ComponentFieldKind::Bool, "true"),
                field("receive_shadows", ComponentFieldKind::Bool, "true"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "Light".to_string(),
            display_name: "Light".to_string(),
            version: 2,
            fields: vec![
                field("kind", ComponentFieldKind::String, "directional"),
                field("color", ComponentFieldKind::Vec3, "1,1,1"),
                field("intensity", ComponentFieldKind::F32, "1"),
                field("range", ComponentFieldKind::F32, "10"),
                field("spot_angle", ComponentFieldKind::F32, "30"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "Rigidbody".to_string(),
            display_name: "Rigidbody".to_string(),
            version: 2,
            fields: vec![
                field("body_type", ComponentFieldKind::String, "dynamic"),
                field("mass", ComponentFieldKind::F32, "1"),
                field("use_gravity", ComponentFieldKind::Bool, "true"),
                field("linear_damping", ComponentFieldKind::F32, "0"),
                field("angular_damping", ComponentFieldKind::F32, "0.05"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "Collider".to_string(),
            display_name: "Collider".to_string(),
            version: 2,
            fields: vec![
                field("shape", ComponentFieldKind::String, "box"),
                field("size", ComponentFieldKind::Vec3, "1,1,1"),
                field("is_trigger", ComponentFieldKind::Bool, "false"),
                field("mask", ComponentFieldKind::String, "4294967295"),
                field("physics_material", ComponentFieldKind::String, "default"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AudioSource".to_string(),
            display_name: "Audio Source".to_string(),
            version: 3,
            fields: vec![
                field("clip", ComponentFieldKind::AssetRef, ""),
                field("volume", ComponentFieldKind::F32, "1"),
                field("looping", ComponentFieldKind::Bool, "false"),
                field("play_on_start", ComponentFieldKind::Bool, "false"),
                field("spatial_blend", ComponentFieldKind::F32, "0"),
                field("spatial_mode", ComponentFieldKind::String, "direct"),
                field("shape", ComponentFieldKind::String, "point"),
                field("inner_angle_degrees", ComponentFieldKind::F32, "30"),
                field("outer_angle_degrees", ComponentFieldKind::F32, "60"),
                field("outer_gain", ComponentFieldKind::F32, "0"),
                field("sphere_radius", ComponentFieldKind::F32, "0"),
                field("attenuation", ComponentFieldKind::String, "none"),
                field("min_distance", ComponentFieldKind::F32, "1"),
                field("max_distance", ComponentFieldKind::F32, "100"),
                field("doppler_scale", ComponentFieldKind::F32, "1"),
                field("spread", ComponentFieldKind::F32, "1"),
                field("category", ComponentFieldKind::String, "sfx"),
                field("critical", ComponentFieldKind::Bool, "false"),
                field("use_hrtf", ComponentFieldKind::Bool, "true"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AudioListener".to_string(),
            display_name: "Audio Listener".to_string(),
            version: 1,
            fields: vec![
                field("output_mode", ComponentFieldKind::String, "stereo"),
                field("hrtf_quality", ComponentFieldKind::String, "medium"),
                field("hrtf_enabled", ComponentFieldKind::Bool, "true"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AcousticMaterial".to_string(),
            display_name: "Acoustic Material".to_string(),
            version: 1,
            fields: vec![
                field("absorption", ComponentFieldKind::Vec3, "0.2,0.3,0.45"),
                field("transmission", ComponentFieldKind::Vec3, "0.35,0.2,0.08"),
                field("scattering", ComponentFieldKind::F32, "0.25"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AcousticGeometry".to_string(),
            display_name: "Acoustic Geometry".to_string(),
            version: 1,
            fields: vec![
                field("size", ComponentFieldKind::Vec3, "1,1,1"),
                field("blocks_direct_path", ComponentFieldKind::Bool, "true"),
                field("material", ComponentFieldKind::Object, ""),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AcousticRoom".to_string(),
            display_name: "Acoustic Room".to_string(),
            version: 1,
            fields: vec![
                field("size", ComponentFieldKind::Vec3, "1,1,1"),
                field("reverb_send", ComponentFieldKind::F32, "0.25"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AcousticPortal".to_string(),
            display_name: "Acoustic Portal".to_string(),
            version: 1,
            fields: vec![
                field("size", ComponentFieldKind::Vec3, "1,1,1"),
                field("openness", ComponentFieldKind::F32, "1"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "AudioZone".to_string(),
            display_name: "Audio Zone".to_string(),
            version: 1,
            fields: vec![
                field("size", ComponentFieldKind::Vec3, "1,1,1"),
                field("reverb_send", ComponentFieldKind::F32, "0"),
                field("direct_gain", ComponentFieldKind::F32, "1"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "ParticleEmitter".to_string(),
            display_name: "Particle Emitter".to_string(),
            version: 1,
            fields: vec![
                field("max_particles", ComponentFieldKind::String, "128"),
                field("emission_rate", ComponentFieldKind::F32, "32"),
                field("lifetime", ComponentFieldKind::F32, "2"),
                field("start_speed", ComponentFieldKind::F32, "2.5"),
                field("start_size", ComponentFieldKind::F32, "0.12"),
                field("end_size", ComponentFieldKind::F32, "0.02"),
                field("start_color", ComponentFieldKind::Vec3, "1,0.85,0.25"),
                field("end_color", ComponentFieldKind::Vec3, "1,0.35,0.08"),
                field("gravity", ComponentFieldKind::Vec3, "0,-9.8,0"),
                field("spread_degrees", ComponentFieldKind::F32, "35"),
                field("looping", ComponentFieldKind::Bool, "true"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry.register(ComponentSchema {
            type_id: "Script".to_string(),
            display_name: "Script".to_string(),
            version: 1,
            fields: vec![
                field("backend", ComponentFieldKind::String, "native"),
                field("script", ComponentFieldKind::String, ""),
                field("pending_recovery", ComponentFieldKind::Bool, "false"),
            ],
            evolution: SchemaEvolution::default(),
        });
        registry
    }

    /// Registers or replaces a schema by type ID.
    pub fn register(&mut self, schema: ComponentSchema) {
        if let Some(existing) = self
            .schemas
            .iter_mut()
            .find(|candidate| candidate.type_id == schema.type_id)
        {
            *existing = schema;
        } else {
            self.schemas.push(schema);
            self.schemas
                .sort_by(|left, right| left.type_id.cmp(&right.type_id));
        }
    }

    /// Returns a schema by type ID.
    pub fn get(&self, type_id: &str) -> Option<&ComponentSchema> {
        self.schemas
            .iter()
            .find(|candidate| candidate.type_id == type_id)
    }

    /// Returns every registered schema.
    pub fn all(&self) -> &[ComponentSchema] {
        &self.schemas
    }
}

fn field(
    name: impl Into<String>,
    kind: ComponentFieldKind,
    default_value: impl Into<String>,
) -> ComponentFieldSchema {
    ComponentFieldSchema {
        name: name.into(),
        kind,
        default_value: default_value.into(),
    }
}

/// Returns the default build configuration path.
fn default_build_config() -> String {
    "build.runtime-min.toml".to_string()
}

/// Project manifest format.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProjectManifest {
    /// Explicit schema version.
    pub format: FormatVersion,
    /// Project display name.
    pub name: String,
    /// Asset root relative to the project file.
    #[serde(default = "default_asset_root")]
    pub asset_root: String,
    /// Default scene path.
    pub default_scene: String,
    /// Build configuration file path relative to project root.
    #[serde(default = "default_build_config")]
    pub build_config: String,
    /// Schema evolution metadata.
    pub evolution: SchemaEvolution,
}

/// Returns the default asset root path.
fn default_asset_root() -> String {
    "assets".to_string()
}

impl ProjectManifest {
    /// Creates an example project manifest.
    pub fn example() -> Self {
        Self {
            format: FormatVersion::new(PROJECT_MANIFEST_VERSION),
            name: "Aster Example".to_string(),
            asset_root: "assets".to_string(),
            default_scene: "scenes/example.aster_scene.json".to_string(),
            build_config: "build.runtime-min.toml".to_string(),
            evolution: SchemaEvolution::default(),
        }
    }

    /// Loads a project manifest from `aster.project.toml` in the given project root.
    ///
    /// Returns a descriptive error when the file is missing or contains invalid TOML.
    pub fn load(project_root: &Path) -> EngineResult<Self> {
        let manifest_path = project_root.join("aster.project.toml");
        let input =
            fs::read_to_string(&manifest_path).map_err(|source| EngineError::Filesystem {
                path: manifest_path.clone(),
                source,
            })?;
        Self::from_toml(&input).map_err(|error| {
            EngineError::other(format!(
                "failed to parse {}: {error}",
                manifest_path.display()
            ))
        })
    }

    /// Parses a [`ProjectManifest`] from a TOML string.
    pub fn from_toml(input: &str) -> EngineResult<Self> {
        toml::from_str(input).map_err(|error| EngineError::config(error.to_string()))
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
    fn builtin_component_registry_contains_inspector_schemas() {
        let registry = ComponentSchemaRegistry::builtin();

        assert!(registry.get("Camera").is_some());
        assert!(registry.get("MeshRenderer").is_some());
        assert!(registry.get("Rigidbody").is_some());
        assert!(registry.get("Collider").is_some());
        assert!(registry.get("AudioSource").is_some());
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
        assert_eq!(manifest.build_config, "build.runtime-min.toml");
        assert_eq!(scene.objects.len(), 2);
        assert!(prefab.diagnostics().is_empty());
        assert_eq!(preferences.theme, "system");
        assert!(build.diagnostics().is_empty());
    }

    #[test]
    fn loads_project_manifest_from_file() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir.join("../../examples/project");

        let manifest = ProjectManifest::load(&project_root).unwrap();

        assert_eq!(manifest.name, "Aster Example");
        assert_eq!(manifest.asset_root, "assets");
        assert_eq!(manifest.build_config, "build.runtime-min.toml");
        assert!(manifest.diagnostics().is_empty());
    }

    #[test]
    fn load_returns_error_for_missing_file() {
        let result = ProjectManifest::load(Path::new("/nonexistent/path"));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("aster.project.toml"));
    }

    #[test]
    fn from_toml_applies_defaults_for_missing_optional_fields() {
        let minimal_toml = r#"
name = "Minimal"
default_scene = "scenes/main.json"

[format]
version = 1

[evolution]
policy = "none"
forward_compatible_read = true
migration_framework = "none"
"#;
        let manifest = ProjectManifest::from_toml(minimal_toml).unwrap();
        assert_eq!(manifest.name, "Minimal");
        assert_eq!(manifest.asset_root, "assets");
        assert_eq!(manifest.build_config, "build.runtime-min.toml");
    }

    #[test]
    fn from_toml_returns_error_for_invalid_toml() {
        let result = ProjectManifest::from_toml("name = [invalid toml]]");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TOML"));
    }

    #[test]
    fn from_toml_returns_error_for_missing_required_fields() {
        let result = ProjectManifest::from_toml("[format]\nversion = 1");
        assert!(result.is_err());
    }
}
