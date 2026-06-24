#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Asset database, registry, manifest, dependency, import, and reload primitives.

pub mod amdl;
pub mod registry;
pub mod resource_trait;
pub mod resource_types;

pub use amdl::{
    AMDL_HEADER, AmdlColliderDecl, AmdlColliderShape, AmdlDiagnostic, AmdlDocument, AmdlLodDecl,
    AmdlMaterialDecl, AmdlMeshDecl, AmdlMeshSource, AmdlModelDecl, AmdlParserError,
    AmdlPrimitiveKind, AmdlRigidbodyDecl, AmdlRigidbodyMode, AmdlSocketDecl, AmdlValidator,
    AmdlValue, compile_amdl, diagnose_amdl, parse_amdl,
};
pub use registry::ResourceTypeRegistry;
pub use resource_trait::{Resource, ResourceHandle as TypedResourceHandle};
pub use resource_types::{
    CurveLoopMode, CurvePoint, CurveResource, FontResource, InputActionDef, InputMapResource,
    ThemeResource,
};

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt, fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use engine_core::{AssetId, EngineError, EngineResult, Handle, HandleAllocator, ResourceId};
use serde::{Deserialize, Serialize};

/// Current schema version for asset-side files.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Stable resource GUID serialized as 128 bits.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetGuid(u128);

impl AssetGuid {
    /// Creates a GUID from raw bits.
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Creates a GUID from the core asset identifier type.
    pub const fn from_asset_id(id: AssetId) -> Self {
        Self(id.as_u128())
    }

    /// Returns the raw GUID bits.
    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// Converts this GUID to the core asset identifier type.
    pub const fn as_asset_id(self) -> AssetId {
        AssetId::from_u128(self.0)
    }
}

impl fmt::Display for AssetGuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:032x}", self.0)
    }
}

impl Serialize for AssetGuid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AssetGuid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AssetGuidVisitor;

        impl serde::de::Visitor<'_> for AssetGuidVisitor {
            type Value = AssetGuid;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a 128-bit asset GUID as hex string or unsigned integer")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = value.strip_prefix("0x").unwrap_or(value);
                u128::from_str_radix(value, 16)
                    .or_else(|_| value.parse::<u128>())
                    .map(AssetGuid::from_u128)
                    .map_err(E::custom)
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AssetGuid::from_u128(value as u128))
            }

            fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AssetGuid::from_u128(value))
            }
        }

        deserializer.deserialize_any(AssetGuidVisitor)
    }
}

/// Engine asset path with explicit UTF-8 boundary handling.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct AssetPath {
    path: PathBuf,
}

impl AssetPath {
    /// Creates an asset path from a native path buffer.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the native path representation.
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Returns a UTF-8 string if the platform path can be represented as UTF-8.
    pub fn to_utf8(&self) -> EngineResult<&str> {
        self.path
            .to_str()
            .ok_or_else(|| EngineError::other("asset path is not valid UTF-8"))
    }
}

/// Supported high-level resource types.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// 2D, 3D, or cube texture data.
    Texture,
    /// Material parameter and binding data.
    Material,
    /// Shader source and specialization configuration.
    Shader,
    /// Audio clip or stream metadata.
    Audio,
    /// Static model geometry.
    Model,
    /// Skinned model geometry.
    SkinnedModel,
    /// Animation clip or animation set.
    Animation,
    /// Varg script source for the runtime.
    Script,
    /// Reusable scene object subset.
    Prefab,
    /// Scene definition data.
    Scene,
}

/// Runtime resource load state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceState {
    /// Known to the registry but not loaded.
    Unloaded,
    /// CPU-side data is being loaded or imported.
    LoadingCpu,
    /// CPU-side data is available.
    CpuReady,
    /// GPU upload has been queued.
    UploadQueued,
    /// GPU-side data is available.
    GpuReady,
    /// The resource must be reloaded before use.
    Stale,
    /// Loading or importing failed.
    Failed,
}

/// Structured diagnostic for failed load, import, or migration operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetDiagnostic {
    /// Optional path related to the failure.
    pub path: Option<PathBuf>,
    /// Optional GUID related to the failure.
    pub guid: Option<AssetGuid>,
    /// Human-readable error context.
    pub message: String,
}

impl AssetDiagnostic {
    /// Creates a diagnostic with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            path: None,
            guid: None,
            message: message.into(),
        }
    }

    /// Adds path context.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Adds GUID context.
    pub fn with_guid(mut self, guid: AssetGuid) -> Self {
        self.guid = Some(guid);
        self
    }
}

/// Asset-layer error with diagnostics suitable for editor surfacing.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// A file format failed to parse.
    #[error("failed to parse {format}: {diagnostic:?}")]
    Parse {
        /// Format name.
        format: &'static str,
        /// Structured diagnostic.
        diagnostic: AssetDiagnostic,
    },
    /// A format version cannot be loaded by this build.
    #[error("unsupported {format} schema version {version}, expected {expected}")]
    UnsupportedVersion {
        /// Format name.
        format: &'static str,
        /// Version found in the file.
        version: u32,
        /// Version supported by this build.
        expected: u32,
    },
    /// A requested resource or path was not found.
    #[error("asset was not found: {diagnostic:?}")]
    NotFound {
        /// Structured diagnostic.
        diagnostic: AssetDiagnostic,
    },
    /// A requested operation conflicts with existing database state.
    #[error("asset conflict: {diagnostic:?}")]
    Conflict {
        /// Structured diagnostic.
        diagnostic: AssetDiagnostic,
    },
}

impl From<AssetError> for EngineError {
    fn from(error: AssetError) -> Self {
        EngineError::other(error.to_string())
    }
}

fn ensure_schema(format: &'static str, version: u32) -> Result<(), AssetError> {
    if version == CURRENT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(AssetError::UnsupportedVersion {
            format,
            version,
            expected: CURRENT_SCHEMA_VERSION,
        })
    }
}

/// Resource metadata stored beside source assets.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResourceMetaFormat {
    /// Schema version.
    pub version: u32,
    /// Stable project GUID.
    pub guid: AssetGuid,
    /// Source path relative to the project asset root.
    pub source_path: PathBuf,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Importer identifier.
    pub importer: String,
    /// GUID dependencies declared by the asset or importer.
    #[serde(default)]
    pub dependencies: Vec<AssetGuid>,
}

impl ResourceMetaFormat {
    /// Parses resource metadata from TOML.
    pub fn from_toml(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = toml::from_str(input).map_err(|source| AssetError::Parse {
            format: "resource meta",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("resource meta", parsed.version)?;
        Ok(parsed)
    }
}

/// Runtime resource metadata including import state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceMeta {
    /// Stable asset GUID.
    pub guid: AssetGuid,
    /// Path relative to the asset root.
    pub path: PathBuf,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Current import / load state.
    pub import_state: ResourceState,
}

/// Texture resource metadata.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TextureResource {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Mipmap count.
    pub mip_levels: u32,
    /// Pixel format name.
    pub format: String,
}

/// Material file format.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct MaterialFormat {
    /// Schema version.
    pub version: u32,
    /// Shader dependency.
    pub shader: AssetGuid,
    /// Texture dependencies keyed by slot name.
    #[serde(default)]
    pub textures: HashMap<String, AssetGuid>,
    /// Numeric material parameters keyed by name.
    #[serde(default)]
    pub parameters: HashMap<String, f32>,
}

impl MaterialFormat {
    /// Parses a material from JSON.
    pub fn from_json(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = serde_json::from_str(input).map_err(|source| AssetError::Parse {
            format: "material",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("material", parsed.version)?;
        Ok(parsed)
    }

    /// Parses a material from TOML.
    pub fn from_toml(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = toml::from_str(input).map_err(|source| AssetError::Parse {
            format: "material",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("material", parsed.version)?;
        Ok(parsed)
    }
}

/// Decoded texture payload ready for a render backend upload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DecodedTextureResource {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Runtime pixel format.
    pub format: String,
    /// Tightly packed RGBA pixels.
    pub pixels: Vec<u8>,
}

/// Decoded cubemap resource with six tightly packed square RGBA faces.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DecodedCubemapResource {
    /// Width and height in pixels for every face.
    pub face_size: u32,
    /// Runtime pixel format.
    pub format: String,
    /// Six tightly packed RGBA faces in +X, -X, +Y, -Y, +Z, -Z order.
    pub pixels: Vec<u8>,
}

/// Source JSON for a six-image cubemap.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CubemapSource {
    /// Positive X face path, relative to the cubemap source file.
    pub positive_x: PathBuf,
    /// Negative X face path, relative to the cubemap source file.
    pub negative_x: PathBuf,
    /// Positive Y face path, relative to the cubemap source file.
    pub positive_y: PathBuf,
    /// Negative Y face path, relative to the cubemap source file.
    pub negative_y: PathBuf,
    /// Positive Z face path, relative to the cubemap source file.
    pub positive_z: PathBuf,
    /// Negative Z face path, relative to the cubemap source file.
    pub negative_z: PathBuf,
}

/// CPU-side texture resource with mip chain for GPU upload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CpuTextureResource {
    /// Width in pixels (base mip level).
    pub width: u32,
    /// Height in pixels (base mip level).
    pub height: u32,
    /// Pixel format name.
    pub format: String,
    /// Mip levels, each containing tightly packed pixel data.
    /// Level 0 is the full resolution, each subsequent level is half resolution.
    pub mip_levels: Vec<Vec<u8>>,
}

impl CpuTextureResource {
    /// Serializes to JSON bytes.
    pub fn to_bytes(&self) -> EngineResult<Arc<[u8]>> {
        serde_json::to_vec(self)
            .map(Arc::from)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    /// Parses from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(|source| AssetError::Parse {
            format: "cpu texture resource",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })
    }
}

/// Import options for asset importers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportOptions {
    /// Whether to generate mip chains for textures.
    pub generate_mips: bool,
    /// Maximum texture dimension (width or height).
    pub max_texture_size: Option<u32>,
}

impl DecodedTextureResource {
    /// Parses a decoded texture payload from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(|source| AssetError::Parse {
            format: "decoded texture",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })
    }

    /// Serializes to JSON bytes.
    pub fn to_bytes(&self) -> EngineResult<Arc<[u8]>> {
        serde_json::to_vec(self)
            .map(Arc::from)
            .map_err(|error| EngineError::other(error.to_string()))
    }
}

impl DecodedCubemapResource {
    /// Parses a decoded cubemap payload from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(|source| AssetError::Parse {
            format: "decoded cubemap",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })
    }

    /// Serializes to JSON bytes.
    pub fn to_bytes(&self) -> EngineResult<Arc<[u8]>> {
        serde_json::to_vec(self)
            .map(Arc::from)
            .map_err(|error| EngineError::other(error.to_string()))
    }
}

/// CPU-side mesh payload imported from a model file.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct BasicMeshResource {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals, if present.
    #[serde(default)]
    pub normals: Vec<[f32; 3]>,
    /// First texture coordinate set, if present.
    #[serde(default)]
    pub texcoords: Vec<[f32; 2]>,
    /// Triangle indices.
    #[serde(default)]
    pub indices: Vec<u32>,
    /// Material index referenced by the primitive, if present.
    pub material_index: Option<usize>,
}

/// CPU-side PBR material resource extracted from glTF.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CpuMaterialResource {
    /// Material name from glTF.
    pub name: String,
    /// Base color factor (RGBA, default white).
    pub base_color: [f32; 4],
    /// Metallic factor (0.0 = dielectric, 1.0 = metal).
    pub metallic: f32,
    /// Roughness factor (0.0 = smooth, 1.0 = rough).
    pub roughness: f32,
    /// Emissive factor (RGB).
    #[serde(default)]
    pub emissive: [f32; 3],
    /// Alpha mode: "OPAQUE", "BLEND", or "MASK".
    #[serde(default = "default_alpha_mode")]
    pub alpha_mode: String,
    /// Alpha cutoff threshold for MASK mode.
    #[serde(default = "default_alpha_cutoff")]
    pub alpha_cutoff: f32,
    /// Base color texture reference (relative asset path).
    pub base_color_texture_ref: Option<String>,
    /// Normal map texture reference (relative asset path).
    pub normal_texture_ref: Option<String>,
    /// Metallic-roughness texture reference (relative asset path).
    pub metallic_roughness_texture_ref: Option<String>,
}

fn default_alpha_mode() -> String {
    "OPAQUE".to_string()
}

fn default_alpha_cutoff() -> f32 {
    0.5
}

impl Default for CpuMaterialResource {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
            alpha_mode: "OPAQUE".to_string(),
            alpha_cutoff: 0.5,
            base_color_texture_ref: None,
            normal_texture_ref: None,
            metallic_roughness_texture_ref: None,
        }
    }
}

/// Imported model payload containing basic static meshes.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ModelResource {
    /// Mesh primitives available to runtime rendering.
    pub meshes: Vec<BasicMeshResource>,
    /// Materials extracted from the model.
    #[serde(default)]
    pub materials: Vec<CpuMaterialResource>,
}

impl ModelResource {
    /// Parses a model payload from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(|source| AssetError::Parse {
            format: "model resource",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })
    }

    fn to_bytes(&self) -> EngineResult<Arc<[u8]>> {
        serde_json::to_vec(self)
            .map(Arc::from)
            .map_err(|error| EngineError::other(error.to_string()))
    }
}

/// Shader configuration file format.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ShaderConfigFormat {
    /// Schema version.
    pub version: u32,
    /// Shader stage entry points keyed by stage name.
    pub stages: HashMap<String, PathBuf>,
    /// Compile-time defines.
    #[serde(default)]
    pub defines: HashMap<String, String>,
}

impl ShaderConfigFormat {
    /// Parses shader configuration from TOML.
    pub fn from_toml(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = toml::from_str(input).map_err(|source| AssetError::Parse {
            format: "shader config",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("shader config", parsed.version)?;
        Ok(parsed)
    }
}

/// Import cache entry produced by importers.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ImportCacheEntry {
    /// Source asset GUID.
    pub guid: AssetGuid,
    /// Source content hash recorded by the importer.
    pub source_hash: String,
    /// Imported artifact path.
    pub artifact_path: PathBuf,
    /// Imported resource kind.
    pub kind: ResourceKind,
    /// Importer identifier and version.
    pub importer_version: String,
}

/// Import cache file format.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct ImportCacheFormat {
    /// Schema version.
    pub version: u32,
    /// Cached imports.
    #[serde(default)]
    pub entries: Vec<ImportCacheEntry>,
}

impl ImportCacheFormat {
    /// Parses an import cache from JSON.
    pub fn from_json(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = serde_json::from_str(input).map_err(|source| AssetError::Parse {
            format: "import cache",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("import cache", parsed.version)?;
        Ok(parsed)
    }
}

/// Manifest entry stored in resource manifests.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AssetManifestEntry {
    /// Stable asset GUID.
    pub guid: AssetGuid,
    /// Asset path relative to the manifest root.
    pub path: AssetPath,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Direct dependency GUIDs.
    #[serde(default)]
    pub dependencies: Vec<AssetGuid>,
}

/// Versioned resource manifest file format.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ResourceManifestFormat {
    /// Schema version.
    pub version: u32,
    /// Manifest entries.
    #[serde(default)]
    pub entries: Vec<AssetManifestEntry>,
}

impl Default for ResourceManifestFormat {
    fn default() -> Self {
        Self {
            version: CURRENT_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

impl ResourceManifestFormat {
    /// Parses a resource manifest from JSON.
    pub fn from_json(input: &str) -> Result<Self, AssetError> {
        let parsed: Self = serde_json::from_str(input).map_err(|source| AssetError::Parse {
            format: "resource manifest",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })?;
        ensure_schema("resource manifest", parsed.version)?;
        Ok(parsed)
    }

    /// Adds or replaces an entry by GUID.
    pub fn upsert(&mut self, entry: AssetManifestEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| candidate.guid == entry.guid)
        {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Looks up an entry by GUID.
    pub fn get(&self, guid: AssetGuid) -> Option<&AssetManifestEntry> {
        self.entries.iter().find(|entry| entry.guid == guid)
    }
}

/// Resource dependency graph keyed by asset GUID.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DependencyGraph {
    outgoing: HashMap<AssetGuid, BTreeSet<AssetGuid>>,
    incoming: HashMap<AssetGuid, BTreeSet<AssetGuid>>,
}

impl DependencyGraph {
    /// Replaces all direct dependencies for a GUID.
    pub fn set_dependencies(
        &mut self,
        guid: AssetGuid,
        dependencies: impl IntoIterator<Item = AssetGuid>,
    ) {
        if let Some(previous) = self.outgoing.remove(&guid) {
            for dependency in previous {
                if let Some(dependents) = self.incoming.get_mut(&dependency) {
                    dependents.remove(&guid);
                }
            }
        }

        let dependencies = dependencies.into_iter().collect::<BTreeSet<_>>();
        for dependency in &dependencies {
            self.incoming.entry(*dependency).or_default().insert(guid);
        }
        self.outgoing.insert(guid, dependencies);
    }

    /// Returns direct dependencies for a GUID.
    pub fn dependencies(&self, guid: AssetGuid) -> Vec<AssetGuid> {
        self.outgoing
            .get(&guid)
            .map(|items| items.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Returns resources that directly depend on a GUID.
    pub fn dependents(&self, guid: AssetGuid) -> Vec<AssetGuid> {
        self.incoming
            .get(&guid)
            .map(|items| items.iter().copied().collect())
            .unwrap_or_default()
    }
}

/// Asset database for GUID/path resolution across project and built-in roots.
#[derive(Clone, Debug)]
pub struct AssetDatabase {
    project_root: PathBuf,
    builtin_root: PathBuf,
    guid_to_path: HashMap<AssetGuid, AssetPath>,
    path_to_guid: HashMap<PathBuf, AssetGuid>,
    meta: HashMap<AssetGuid, ResourceMetaFormat>,
    dependencies: DependencyGraph,
    /// Runtime resource metadata keyed by project-relative path.
    entries: HashMap<PathBuf, ResourceMeta>,
    /// Folder paths discovered during asset scan.
    folders: BTreeSet<PathBuf>,
}

impl AssetDatabase {
    /// Creates an empty asset database.
    pub fn new(project_root: impl Into<PathBuf>, builtin_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            builtin_root: builtin_root.into(),
            guid_to_path: HashMap::new(),
            path_to_guid: HashMap::new(),
            meta: HashMap::new(),
            dependencies: DependencyGraph::default(),
            entries: HashMap::new(),
            folders: BTreeSet::new(),
        }
    }

    /// Registers or updates metadata and GUID/path mappings.
    pub fn upsert_meta(&mut self, meta: ResourceMetaFormat) -> Result<(), AssetError> {
        ensure_schema("resource meta", meta.version)?;
        let path = AssetPath::new(meta.source_path.clone());
        if let Some(existing_guid) = self.path_to_guid.get(path.as_path()) {
            if *existing_guid != meta.guid {
                return Err(AssetError::Conflict {
                    diagnostic: AssetDiagnostic::new("path is already mapped to a different GUID")
                        .with_path(path.as_path()),
                });
            }
        }

        self.dependencies
            .set_dependencies(meta.guid, meta.dependencies.iter().copied());
        self.path_to_guid
            .insert(meta.source_path.clone(), meta.guid);
        self.guid_to_path.insert(meta.guid, path);
        self.meta.insert(meta.guid, meta);
        Ok(())
    }

    /// Creates a project resource record with no dependencies.
    pub fn create_project_resource(
        &mut self,
        guid: AssetGuid,
        path: impl Into<PathBuf>,
        kind: ResourceKind,
        importer: impl Into<String>,
    ) -> Result<(), AssetError> {
        self.upsert_meta(ResourceMetaFormat {
            version: CURRENT_SCHEMA_VERSION,
            guid,
            source_path: path.into(),
            kind,
            importer: importer.into(),
            dependencies: Vec::new(),
        })
    }

    /// Resolves a GUID to a project-relative path.
    pub fn resolve_guid(&self, guid: AssetGuid) -> Result<&AssetPath, AssetError> {
        self.guid_to_path
            .get(&guid)
            .ok_or_else(|| AssetError::NotFound {
                diagnostic: AssetDiagnostic::new("GUID is not present in the asset database")
                    .with_guid(guid),
            })
    }

    /// Resolves a project-relative path to a GUID.
    pub fn guid_for_path(&self, path: impl AsRef<Path>) -> Result<AssetGuid, AssetError> {
        let path = path.as_ref();
        self.path_to_guid
            .get(path)
            .copied()
            .ok_or_else(|| AssetError::NotFound {
                diagnostic: AssetDiagnostic::new("path is not present in the asset database")
                    .with_path(path),
            })
    }

    /// Resolves a project-relative path to a GUID, returning `None` when unknown.
    pub fn get_guid_for_path(&self, path: impl AsRef<Path>) -> Option<AssetGuid> {
        self.path_to_guid.get(path.as_ref()).copied()
    }

    /// Resolves `builtin:/x` or `project:/x` resource references to native paths.
    ///
    /// Rejects references whose resolved path escapes the intended root directory.
    pub fn resolve_resource_reference(&self, reference: &str) -> Result<PathBuf, AssetError> {
        let (root, rest) = if let Some(rest) = reference.strip_prefix("builtin:/") {
            (&self.builtin_root, rest)
        } else if let Some(rest) = reference.strip_prefix("project:/") {
            (&self.project_root, rest)
        } else {
            return Err(AssetError::NotFound {
                diagnostic: AssetDiagnostic::new(
                    "resource reference must use builtin:/ or project:/",
                ),
            });
        };

        let resolved = root.join(rest);
        // Canonicalize to resolve ../ components and symlinks.
        let canonical = resolved.canonicalize().map_err(|_| AssetError::NotFound {
            diagnostic: AssetDiagnostic::new("resource reference resolves to a non-existent path")
                .with_path(&resolved),
        })?;
        let canonical_root = root.canonicalize().map_err(|_| AssetError::NotFound {
            diagnostic: AssetDiagnostic::new("root directory does not exist").with_path(root),
        })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(AssetError::NotFound {
                diagnostic: AssetDiagnostic::new("resource reference escapes its root directory")
                    .with_path(&resolved),
            });
        }

        Ok(canonical)
    }

    /// Returns the dependency graph.
    pub fn dependencies(&self) -> &DependencyGraph {
        &self.dependencies
    }

    /// Builds a versioned manifest from registered database entries.
    pub fn manifest(&self) -> ResourceManifestFormat {
        let mut manifest = ResourceManifestFormat::default();
        for meta in self.meta.values() {
            manifest.upsert(AssetManifestEntry {
                guid: meta.guid,
                path: AssetPath::new(meta.source_path.clone()),
                kind: meta.kind,
                dependencies: meta.dependencies.clone(),
            });
        }
        manifest
    }

    /// Scans an asset root directory tree, registering resources and folders.
    ///
    /// New files are added with `import_state` set to `Unloaded`. Existing entries
    /// matching the same project-relative path are preserved (GUID stays stable).
    /// Entries whose paths no longer exist on disk are removed.
    pub fn scan(&mut self, root: &Path) -> EngineResult<()> {
        let asset_root = self.project_root.clone();
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            asset_root.join(root)
        };

        let mut current_paths: HashSet<PathBuf> = HashSet::new();
        let mut current_folders: BTreeSet<PathBuf> = BTreeSet::new();

        if !root.exists() {
            self.entries.clear();
            self.folders.clear();
            return Ok(());
        }

        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let dir_entries = fs::read_dir(&dir).map_err(|source| EngineError::Filesystem {
                path: dir.clone(),
                source,
            })?;
            for entry in dir_entries {
                let entry = entry.map_err(|source| EngineError::Filesystem {
                    path: dir.clone(),
                    source,
                })?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path.clone());
                    if let Ok(relative) = path.strip_prefix(&root) {
                        current_folders.insert(relative.to_path_buf());
                    }
                    continue;
                }
                if path.extension().and_then(|value| value.to_str()) == Some("meta") {
                    continue;
                }
                let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
                // Try extension-based inference first, then content-based for JSON files
                let (kind, importer) = match infer_importer(&relative) {
                    Some(result) => result,
                    None => {
                        if relative.extension().and_then(|v| v.to_str()) == Some("json") {
                            match infer_scene_json(&path) {
                                Some(result) => result,
                                None => continue,
                            }
                        } else {
                            continue;
                        }
                    }
                };
                current_paths.insert(relative.clone());

                // Preserve existing GUID or generate a new one
                let guid = self
                    .path_to_guid
                    .get(&relative)
                    .copied()
                    .unwrap_or_else(|| generate_asset_guid(&relative));

                let meta = ResourceMeta {
                    guid,
                    path: relative.clone(),
                    kind,
                    import_state: ResourceState::Unloaded,
                };
                self.entries.insert(relative.clone(), meta);

                // Also register in the persistent metadata tables
                let meta_format = ResourceMetaFormat {
                    version: CURRENT_SCHEMA_VERSION,
                    guid,
                    source_path: relative,
                    kind,
                    importer: importer.to_string(),
                    dependencies: Vec::new(),
                };
                let _ = self.upsert_meta(meta_format);
            }
        }

        // Remove entries whose paths no longer exist on disk
        self.entries.retain(|path, _| current_paths.contains(path));
        self.folders = current_folders;

        Ok(())
    }

    /// Returns all runtime resource entries.
    pub fn iter_entries(&self) -> impl Iterator<Item = &ResourceMeta> {
        self.entries.values()
    }

    /// Returns the runtime metadata for a specific path.
    pub fn entry_for_path(&self, path: &Path) -> Option<&ResourceMeta> {
        self.entries.get(path)
    }

    /// Returns the runtime metadata for a specific GUID.
    pub fn entry_for_guid(&self, guid: AssetGuid) -> Option<&ResourceMeta> {
        self.guid_to_path
            .get(&guid)
            .and_then(|asset_path| self.entries.get(asset_path.as_path()))
    }

    /// Returns discovered folder paths.
    pub fn folders(&self) -> &BTreeSet<PathBuf> {
        &self.folders
    }

    /// Returns mutable access to runtime resource entries.
    pub fn entries_mut(&mut self) -> &mut HashMap<PathBuf, ResourceMeta> {
        &mut self.entries
    }
}

/// Project panel preview and thumbnail metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewData {
    /// Optional thumbnail bytes in an implementation-defined encoded format.
    pub thumbnail: Option<Arc<[u8]>>,
    /// Human-readable preview summary.
    pub summary: String,
}

/// Stable Rust-native resource handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ResourceHandle {
    id: ResourceId,
    handle: Handle,
}

impl ResourceHandle {
    /// Creates a resource handle from raw parts.
    pub const fn new(id: ResourceId, handle: Handle) -> Self {
        Self { id, handle }
    }

    /// Stable resource ID.
    pub const fn id(self) -> ResourceId {
        self.id
    }

    /// Generational handle value.
    pub const fn raw_handle(self) -> Handle {
        self.handle
    }
}

/// Registered resource record.
#[derive(Clone, Debug)]
pub struct ResourceRecord {
    /// Stable resource ID.
    pub id: ResourceId,
    /// Source asset GUID.
    pub guid: AssetGuid,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Current load state.
    pub state: ResourceState,
    /// Direct dependency GUIDs.
    pub dependencies: Vec<AssetGuid>,
    /// Optional project panel preview data.
    pub preview: Option<PreviewData>,
}

/// CPU-side cached resource data.
#[derive(Clone, Debug)]
pub struct CpuResource {
    /// Resource kind.
    pub kind: ResourceKind,
    /// Implementation-defined CPU payload.
    pub bytes: Arc<[u8]>,
}

/// GPU-side cached resource data.
#[derive(Clone, Debug)]
pub struct GpuResource {
    /// Resource kind.
    pub kind: ResourceKind,
    /// Backend-owned opaque token.
    pub backend_token: u64,
}

/// Registry for stable resource handles and CPU/GPU cache lifetimes.
#[derive(Clone, Debug, Default)]
pub struct AssetRegistry {
    handles: HandleAllocator,
    by_handle: HashMap<Handle, ResourceRecord>,
    by_guid: HashMap<AssetGuid, ResourceHandle>,
    cpu_cache: HashMap<ResourceHandle, CpuResource>,
    gpu_cache: HashMap<ResourceHandle, GpuResource>,
    /// GPU resources pending deferred destruction (handle, backend_token, frames_remaining)
    gpu_destroy_queue: VecDeque<(ResourceHandle, u64, u32)>,
}

impl AssetRegistry {
    /// Registers a resource and returns its stable native handle.
    pub fn register(
        &mut self,
        guid: AssetGuid,
        kind: ResourceKind,
    ) -> EngineResult<ResourceHandle> {
        if let Some(handle) = self.by_guid.get(&guid) {
            return Ok(*handle);
        }

        let raw = self.handles.allocate()?;
        let id = ResourceId::from_u128(guid.as_u128());
        let handle = ResourceHandle::new(id, raw);
        self.by_handle.insert(
            raw,
            ResourceRecord {
                id,
                guid,
                kind,
                state: ResourceState::Unloaded,
                dependencies: Vec::new(),
                preview: None,
            },
        );
        self.by_guid.insert(guid, handle);
        Ok(handle)
    }

    /// Looks up a handle by GUID.
    pub fn handle_for_guid(&self, guid: AssetGuid) -> Option<ResourceHandle> {
        self.by_guid.get(&guid).copied()
    }

    /// Returns a registered resource record.
    pub fn record(&self, handle: ResourceHandle) -> Option<&ResourceRecord> {
        if self.handles.is_live(handle.raw_handle()) {
            self.by_handle.get(&handle.raw_handle())
        } else {
            None
        }
    }

    /// Updates resource state.
    pub fn set_state(&mut self, handle: ResourceHandle, state: ResourceState) -> EngineResult<()> {
        let record = self
            .by_handle
            .get_mut(&handle.raw_handle())
            .ok_or_else(|| EngineError::invalid_handle("resource handle does not exist"))?;
        record.state = state;
        Ok(())
    }

    /// Updates project panel preview data.
    pub fn set_preview(
        &mut self,
        handle: ResourceHandle,
        preview: PreviewData,
    ) -> EngineResult<()> {
        let record = self
            .by_handle
            .get_mut(&handle.raw_handle())
            .ok_or_else(|| EngineError::invalid_handle("resource handle does not exist"))?;
        record.preview = Some(preview);
        Ok(())
    }

    /// Inserts or replaces CPU cache data without changing GPU lifetime.
    pub fn put_cpu(&mut self, handle: ResourceHandle, resource: CpuResource) -> EngineResult<()> {
        self.ensure_live(handle)?;
        self.cpu_cache.insert(handle, resource);
        self.set_state(handle, ResourceState::CpuReady)
    }

    /// Returns CPU cache data for a resource.
    pub fn cpu_resource(&self, handle: ResourceHandle) -> Option<&CpuResource> {
        self.cpu_cache.get(&handle)
    }

    /// Returns GPU cache data for a resource.
    pub fn gpu_resource(&self, handle: ResourceHandle) -> Option<&GpuResource> {
        self.gpu_cache.get(&handle)
    }

    /// Inserts or replaces GPU cache data without changing CPU lifetime.
    pub fn put_gpu(&mut self, handle: ResourceHandle, resource: GpuResource) -> EngineResult<()> {
        self.ensure_live(handle)?;
        self.gpu_cache.insert(handle, resource);
        self.set_state(handle, ResourceState::GpuReady)
    }

    /// Drops only CPU-side cache data for a resource.
    pub fn drop_cpu(&mut self, handle: ResourceHandle) {
        self.cpu_cache.remove(&handle);
    }

    /// Drops only GPU-side cache data for a resource.
    pub fn drop_gpu(&mut self, handle: ResourceHandle) {
        self.gpu_cache.remove(&handle);
    }

    /// Marks a resource stale and drops both cache tiers.
    pub fn mark_stale(&mut self, handle: ResourceHandle) -> EngineResult<()> {
        self.drop_cpu(handle);
        self.drop_gpu(handle);
        self.set_state(handle, ResourceState::Stale)
    }

    /// Replaces GPU resource with a new one, enqueuing the old one for deferred destruction.
    ///
    /// The old GPU resource is kept alive for `frames` frames (default 3) to allow
    /// in-flight rendering commands to complete before the backend destroys it.
    pub fn swap_gpu(
        &mut self,
        handle: ResourceHandle,
        new_resource: GpuResource,
        frames: u32,
    ) -> EngineResult<()> {
        self.ensure_live(handle)?;

        // If there's an old GPU resource, enqueue it for deferred destruction
        if let Some(old_resource) = self.gpu_cache.get(&handle) {
            self.gpu_destroy_queue
                .push_back((handle, old_resource.backend_token, frames));
        }

        // Insert the new GPU resource
        self.gpu_cache.insert(handle, new_resource);
        self.set_state(handle, ResourceState::GpuReady)
    }

    /// Ticks the deferred GPU destroy queue, decrementing frame counters.
    ///
    /// Returns backend tokens that reached 0 during this tick.
    /// Decrements all counters first, then removes items that are now at 0.
    pub fn tick_gpu_destroy_queue(&mut self) -> Vec<u64> {
        let mut ready_to_destroy = Vec::new();

        // Check if any items are already at 0 before we start
        let had_zeros_before = self.gpu_destroy_queue.iter().any(|(_, _, f)| *f == 0);

        // If there were items at 0, remove them without decrementing
        if had_zeros_before {
            self.gpu_destroy_queue
                .retain(|(_handle, token, frames_remaining)| {
                    if *frames_remaining == 0 {
                        ready_to_destroy.push(*token);
                        false // Remove from queue
                    } else {
                        true // Keep in queue
                    }
                });
        } else {
            // No items at 0, so decrement all and then remove any that reached 0
            for (_handle, _token, frames_remaining) in &mut self.gpu_destroy_queue {
                *frames_remaining -= 1;
            }

            self.gpu_destroy_queue
                .retain(|(_handle, token, frames_remaining)| {
                    if *frames_remaining == 0 {
                        ready_to_destroy.push(*token);
                        false // Remove from queue
                    } else {
                        true // Keep in queue
                    }
                });
        }

        ready_to_destroy
    }

    /// Marks a resource as failed and logs the error.
    pub fn mark_failed(&mut self, handle: ResourceHandle, error: &str) -> EngineResult<()> {
        self.drop_cpu(handle);
        self.drop_gpu(handle);
        self.set_state(handle, ResourceState::Failed)?;

        // Store error in preview for display in the editor
        if let Some(record) = self.by_handle.get_mut(&handle.raw_handle()) {
            record.preview = Some(PreviewData {
                thumbnail: None,
                summary: format!("Import failed: {}", error),
            });
        }

        Ok(())
    }

    fn ensure_live(&self, handle: ResourceHandle) -> EngineResult<()> {
        if self.handles.is_live(handle.raw_handle()) {
            Ok(())
        } else {
            Err(EngineError::invalid_handle("resource handle is stale"))
        }
    }
}

/// Import task handled by CPU loading/import workers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportTask {
    /// Source GUID.
    pub guid: AssetGuid,
    /// Source path.
    pub source_path: PathBuf,
    /// Resource kind to import.
    pub kind: ResourceKind,
    /// Importer name.
    pub importer: String,
}

/// GPU upload task separated from CPU loading/import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuUploadTask {
    /// Destination resource handle.
    pub handle: ResourceHandle,
    /// Resource kind.
    pub kind: ResourceKind,
}

/// Result of an import task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportOutcome {
    /// Source GUID.
    pub guid: AssetGuid,
    /// Import diagnostics.
    pub diagnostics: Vec<AssetDiagnostic>,
    /// Optional upload task produced by the import.
    pub upload: Option<GpuUploadTask>,
}

/// glTF importer for mesh extraction.
pub struct GltfImporter;

impl GltfImporter {
    /// Imports a glTF file into a model resource with mesh primitives.
    ///
    /// Returns an `ImportOutcome` with diagnostics. On success, the model resource
    /// contains all mesh primitives with positions, normals, texcoords, and indices.
    pub fn import(path: &Path) -> EngineResult<ImportOutcome> {
        let mut diagnostics = Vec::new();

        // Import the glTF model
        let model = match import_gltf_model(path) {
            Ok(model) => model,
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("glTF import failed: {}", error)).with_path(path),
                );
                return Ok(ImportOutcome {
                    guid: generate_asset_guid(path),
                    diagnostics,
                    upload: None,
                });
            }
        };

        // Validate that we have at least one mesh
        if model.meshes.is_empty() {
            diagnostics.push(
                AssetDiagnostic::new("glTF file contains no mesh primitives").with_path(path),
            );
        }

        Ok(ImportOutcome {
            guid: generate_asset_guid(path),
            diagnostics,
            upload: None,
        })
    }

    /// Imports a glTF file and stores the result in the asset registry.
    pub fn import_to_registry(
        path: &Path,
        registry: &mut AssetRegistry,
        guid: AssetGuid,
    ) -> EngineResult<ImportOutcome> {
        let mut diagnostics = Vec::new();

        // Import the glTF model
        let model = match import_gltf_model(path) {
            Ok(model) => model,
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("glTF import failed: {}", error)).with_path(path),
                );
                return Ok(ImportOutcome {
                    guid,
                    diagnostics,
                    upload: None,
                });
            }
        };

        // Validate that we have at least one mesh
        if model.meshes.is_empty() {
            diagnostics.push(
                AssetDiagnostic::new("glTF file contains no mesh primitives").with_path(path),
            );
        }

        // Register and store in registry
        let handle = registry.register(guid, ResourceKind::Model)?;
        registry.set_state(handle, ResourceState::LoadingCpu)?;

        let model_bytes = model.to_bytes()?;
        registry.put_cpu(
            handle,
            CpuResource {
                kind: ResourceKind::Model,
                bytes: model_bytes,
            },
        )?;

        registry.set_preview(
            handle,
            PreviewData {
                thumbnail: None,
                summary: format!(
                    "glTF model with {} mesh primitive{}",
                    model.meshes.len(),
                    if model.meshes.len() == 1 { "" } else { "s" }
                ),
            },
        )?;

        Ok(ImportOutcome {
            guid,
            diagnostics,
            upload: Some(GpuUploadTask {
                handle,
                kind: ResourceKind::Model,
            }),
        })
    }
}

/// PNG importer with mip chain generation.
pub struct PngImporter;

impl PngImporter {
    /// Imports a PNG file into a CPU texture resource with mip chain.
    ///
    /// Returns an `ImportOutcome` with diagnostics. On success, the CPU texture resource
    /// is serialized and can be retrieved via the asset registry after calling
    /// `import_png_to_registry`.
    pub fn import(path: &Path, options: &ImportOptions) -> EngineResult<ImportOutcome> {
        let mut diagnostics = Vec::new();

        // Read the file
        let bytes = fs::read(path).map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;

        // Decode the PNG
        let image = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("PNG decode failed: {}", error)).with_path(path),
                );
                // Return outcome with diagnostics but no upload
                return Ok(ImportOutcome {
                    guid: generate_asset_guid(path),
                    diagnostics,
                    upload: None,
                });
            }
        };

        // Convert to RGBA8
        let rgba = image.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        // Generate mip chain
        let mip_levels = if options.generate_mips {
            generate_mip_chain(&rgba)
        } else {
            vec![rgba.into_raw()]
        };

        let _cpu_texture = CpuTextureResource {
            width,
            height,
            format: "Rgba8UnormSrgb".to_string(),
            mip_levels,
        };

        Ok(ImportOutcome {
            guid: generate_asset_guid(path),
            diagnostics,
            upload: None, // Caller will set this if needed
        })
    }

    /// Imports a PNG file and stores the result in the asset registry.
    pub fn import_to_registry(
        path: &Path,
        options: &ImportOptions,
        registry: &mut AssetRegistry,
        guid: AssetGuid,
    ) -> EngineResult<ImportOutcome> {
        let mut diagnostics = Vec::new();

        // Read the file
        let bytes = fs::read(path).map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;

        // Decode the PNG
        let image = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("PNG decode failed: {}", error)).with_path(path),
                );
                return Ok(ImportOutcome {
                    guid,
                    diagnostics,
                    upload: None,
                });
            }
        };

        // Convert to RGBA8
        let rgba = image.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        // Generate mip chain
        let mip_levels = if options.generate_mips {
            generate_mip_chain(&rgba)
        } else {
            vec![rgba.into_raw()]
        };

        let cpu_texture = CpuTextureResource {
            width,
            height,
            format: "Rgba8UnormSrgb".to_string(),
            mip_levels,
        };

        // Register and store in registry
        let handle = registry.register(guid, ResourceKind::Texture)?;
        registry.set_state(handle, ResourceState::LoadingCpu)?;

        let texture_bytes = cpu_texture.to_bytes()?;
        registry.put_cpu(
            handle,
            CpuResource {
                kind: ResourceKind::Texture,
                bytes: texture_bytes,
            },
        )?;

        registry.set_preview(
            handle,
            PreviewData {
                thumbnail: None,
                summary: format!(
                    "{}x{} {} texture with {} mip levels",
                    width,
                    height,
                    cpu_texture.format,
                    cpu_texture.mip_levels.len()
                ),
            },
        )?;

        Ok(ImportOutcome {
            guid,
            diagnostics,
            upload: Some(GpuUploadTask {
                handle,
                kind: ResourceKind::Texture,
            }),
        })
    }
}

/// Generates a mip chain from a base RGBA8 image using box filtering.
fn generate_mip_chain(base: &image::RgbaImage) -> Vec<Vec<u8>> {
    let mut mip_levels = Vec::new();

    // Level 0: original image
    mip_levels.push(base.clone().into_raw());

    let mut current = base.clone();

    // Generate subsequent levels until we reach 1x1
    while current.width() > 1 || current.height() > 1 {
        let new_width = (current.width() / 2).max(1);
        let new_height = (current.height() / 2).max(1);

        let downsampled = downsample_rgba8(&current, new_width, new_height);
        mip_levels.push(downsampled.clone().into_raw());
        current = downsampled;
    }

    mip_levels
}

/// Downsamples an RGBA8 image to a smaller size using box filtering.
fn downsample_rgba8(
    source: &image::RgbaImage,
    target_width: u32,
    target_height: u32,
) -> image::RgbaImage {
    use image::imageops::FilterType;
    image::imageops::resize(source, target_width, target_height, FilterType::Triangle)
}

/// Job sent to the import worker thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportJob {
    /// Asset path to import.
    pub asset_path: PathBuf,
    /// Resource kind.
    pub resource_kind: ResourceKind,
    /// Import options (currently unused, reserved for future).
    pub import_options: ImportOptions,
}

/// Handle to a background import worker thread.
pub struct ImportWorker {
    thread_handle: Option<JoinHandle<()>>,
    job_sender: Sender<ImportJob>,
    outcome_receiver: Receiver<ImportOutcome>,
}

impl ImportWorker {
    /// Spawns a new background worker thread for processing imports.
    pub fn spawn() -> Self {
        let (job_sender, job_receiver) = mpsc::channel::<ImportJob>();
        let (outcome_sender, outcome_receiver) = mpsc::channel::<ImportOutcome>();

        let thread_handle = thread::spawn(move || {
            loop {
                match job_receiver.recv() {
                    Ok(job) => {
                        let outcome = Self::process_job(job);
                        // If the outcome receiver is dropped, the main thread has exited
                        if outcome_sender.send(outcome).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Self {
            thread_handle: Some(thread_handle),
            job_sender,
            outcome_receiver,
        }
    }

    /// Enqueues an import job to be processed by the worker.
    pub fn enqueue(&self, job: ImportJob) -> EngineResult<()> {
        self.job_sender
            .send(job)
            .map_err(|_| EngineError::other("import worker thread has terminated"))
    }

    /// Polls for completed import outcomes without blocking.
    pub fn try_recv_outcome(&self) -> Option<ImportOutcome> {
        self.outcome_receiver.try_recv().ok()
    }

    /// Processes a single import job by dispatching to the appropriate importer.
    fn process_job(job: ImportJob) -> ImportOutcome {
        let guid = generate_asset_guid(&job.asset_path);

        // Dispatch to the correct importer based on resource kind and extension
        match job.resource_kind {
            ResourceKind::Texture => {
                // Use PngImporter for texture imports
                PngImporter::import(&job.asset_path, &job.import_options).unwrap_or_else(|error| {
                    ImportOutcome {
                        guid,
                        diagnostics: vec![
                            AssetDiagnostic::new(format!("Texture import failed: {}", error))
                                .with_path(&job.asset_path),
                        ],
                        upload: None,
                    }
                })
            }
            ResourceKind::Model => {
                // Use GltfImporter for model imports
                GltfImporter::import(&job.asset_path).unwrap_or_else(|error| ImportOutcome {
                    guid,
                    diagnostics: vec![
                        AssetDiagnostic::new(format!("Model import failed: {}", error))
                            .with_path(&job.asset_path),
                    ],
                    upload: None,
                })
            }
            _ => {
                // Unsupported resource kind
                ImportOutcome {
                    guid,
                    diagnostics: vec![
                        AssetDiagnostic::new(format!(
                            "Unsupported resource kind for import: {:?}",
                            job.resource_kind
                        ))
                        .with_path(&job.asset_path),
                    ],
                    upload: None,
                }
            }
        }
    }
}

impl Drop for ImportWorker {
    fn drop(&mut self) {
        // Take the thread handle first
        if let Some(handle) = self.thread_handle.take() {
            // Drop the sender explicitly by replacing it with a dummy channel
            // This signals the worker thread to exit
            drop(std::mem::replace(&mut self.job_sender, {
                let (tx, _rx) = mpsc::channel();
                tx
            }));

            // Wait for the worker thread to finish
            let _ = handle.join();
        }
    }
}

/// Import and upload queues with separated CPU and GPU work.
#[derive(Clone, Debug, Default)]
pub struct ImportQueue {
    imports: VecDeque<ImportTask>,
    uploads: Arc<Mutex<VecDeque<GpuUploadTask>>>,
}

impl ImportQueue {
    /// Queues a CPU import/load task.
    pub fn push_import(&mut self, task: ImportTask) {
        self.imports.push_back(task);
    }

    /// Queues a GPU upload task.
    pub fn push_upload(&mut self, task: GpuUploadTask) {
        if let Ok(mut uploads) = self.uploads.lock() {
            uploads.push_back(task);
        }
    }

    /// Pops one GPU upload task.
    pub fn pop_upload(&mut self) -> Option<GpuUploadTask> {
        self.uploads.lock().ok()?.pop_front()
    }

    /// Drains all pending GPU upload tasks.
    pub fn drain_gpu_uploads(&mut self) -> Vec<GpuUploadTask> {
        match self.uploads.lock() {
            Ok(mut uploads) => uploads.drain(..).collect(),
            _ => Vec::new(),
        }
    }

    /// Spawns a background worker thread for processing imports.
    ///
    /// The worker will process import jobs and produce GPU upload tasks
    /// that can be consumed via `drain_gpu_uploads()`.
    pub fn spawn_worker(&self) -> ImportWorker {
        ImportWorker::spawn()
    }

    /// Drains imports across worker threads and appends produced upload tasks.
    pub fn drain_imports_parallel<F>(
        &mut self,
        worker_count: usize,
        import: F,
    ) -> Vec<ImportOutcome>
    where
        F: Fn(ImportTask) -> ImportOutcome + Sync,
    {
        let worker_count = worker_count.max(1);
        let tasks = self.imports.drain(..).collect::<Vec<_>>();
        if tasks.is_empty() {
            return Vec::new();
        }

        let chunk_size = tasks.len().div_ceil(worker_count);
        let mut outcomes = thread::scope(|scope| {
            let mut workers = Vec::new();
            for chunk in tasks.chunks(chunk_size) {
                let chunk = chunk.to_vec();
                let import = &import;
                workers
                    .push(scope.spawn(move || chunk.into_iter().map(import).collect::<Vec<_>>()));
            }

            workers
                .into_iter()
                .flat_map(|worker| worker.join().unwrap_or_default())
                .collect::<Vec<_>>()
        });

        for outcome in &outcomes {
            if let Some(upload) = &outcome.upload {
                self.push_upload(upload.clone());
            }
        }
        outcomes.sort_by_key(|outcome| outcome.guid);
        outcomes
    }
}

/// Hot-reload tracker based on source modification stamps.
#[derive(Clone, Debug, Default)]
pub struct HotReloadTracker {
    stamps: HashMap<AssetGuid, SystemTime>,
}

impl HotReloadTracker {
    /// Updates a resource stamp and returns true when it changed.
    pub fn observe(&mut self, guid: AssetGuid, modified: SystemTime) -> bool {
        match self.stamps.insert(guid, modified) {
            Some(previous) => previous != modified,
            None => false,
        }
    }

    /// Marks changed resources stale in a registry.
    pub fn reload_changed(
        &mut self,
        registry: &mut AssetRegistry,
        changed: impl IntoIterator<Item = (AssetGuid, SystemTime)>,
    ) -> EngineResult<Vec<ResourceHandle>> {
        let mut reloaded = Vec::new();
        for (guid, modified) in changed {
            if self.observe(guid, modified) {
                if let Some(handle) = registry.handle_for_guid(guid) {
                    registry.mark_stale(handle)?;
                    reloaded.push(handle);
                }
            }
        }
        Ok(reloaded)
    }
}

/// Hot reload coordinator that manages the full reimport flow.
///
/// Handles file change events → mark stale → reimport → GPU upload → swap.
pub struct HotReloadCoordinator {
    /// Import queue for background processing.
    import_queue: ImportQueue,
    /// Import worker thread.
    import_worker: Option<ImportWorker>,
    /// Number of frames to delay GPU resource destruction (default 3).
    gpu_destroy_delay_frames: u32,
}

impl HotReloadCoordinator {
    /// Creates a new hot reload coordinator.
    pub fn new(_asset_root: impl Into<PathBuf>) -> Self {
        let import_queue = ImportQueue::default();
        let import_worker = Some(import_queue.spawn_worker());

        Self {
            import_queue,
            import_worker,
            gpu_destroy_delay_frames: 3,
        }
    }

    /// Processes file events and enqueues reimports for modified/created assets.
    pub fn process_file_events(
        &mut self,
        events: &[FileEvent],
        database: &mut AssetDatabase,
    ) -> EngineResult<Vec<AssetGuid>> {
        let mut affected_guids = Vec::new();

        for event in events {
            if let Some(guid) = database.handle_event(event)? {
                // Get the runtime metadata to create an import task
                if let Some(runtime_meta) = database.entry_for_guid(guid) {
                    // Infer importer from the path
                    if let Some((kind, importer)) = infer_importer(&runtime_meta.path) {
                        let task = ImportTask {
                            guid,
                            source_path: runtime_meta.path.clone(),
                            kind,
                            importer: importer.to_string(),
                        };

                        // Enqueue the import task
                        self.import_queue.push_import(task);
                        affected_guids.push(guid);
                    }
                }
            }
        }

        Ok(affected_guids)
    }

    /// Polls for completed imports and processes them.
    ///
    /// Returns import outcomes with diagnostics for logging to the console.
    pub fn poll_completed_imports(&mut self, registry: &mut AssetRegistry) -> Vec<ImportOutcome> {
        let mut outcomes = Vec::new();

        if let Some(worker) = &self.import_worker {
            while let Some(outcome) = worker.try_recv_outcome() {
                // Process the import outcome
                if outcome.diagnostics.is_empty() {
                    // Import succeeded - the upload task will be processed separately
                    if let Some(upload) = &outcome.upload {
                        self.import_queue.push_upload(upload.clone());
                    }
                } else {
                    // Import failed - mark the resource as failed
                    if let Some(handle) = registry.handle_for_guid(outcome.guid) {
                        let error_msg = outcome
                            .diagnostics
                            .iter()
                            .map(|d| d.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ");
                        let _ = registry.mark_failed(handle, &error_msg);
                    }
                }

                outcomes.push(outcome);
            }
        }

        outcomes
    }

    /// Processes GPU upload tasks by swapping in new resources.
    ///
    /// The caller must provide a function that performs the actual GPU upload
    /// and returns the backend token for the new GPU resource.
    pub fn process_gpu_uploads<F>(
        &mut self,
        registry: &mut AssetRegistry,
        mut upload_fn: F,
    ) -> EngineResult<Vec<ResourceHandle>>
    where
        F: FnMut(&GpuUploadTask, &CpuResource) -> EngineResult<u64>,
    {
        let mut uploaded = Vec::new();
        let uploads = self.import_queue.drain_gpu_uploads();

        for upload in uploads {
            // Get the CPU resource data
            if let Some(cpu_resource) = registry.cpu_resource(upload.handle) {
                // Perform the GPU upload
                match upload_fn(&upload, cpu_resource) {
                    Ok(backend_token) => {
                        // Swap in the new GPU resource
                        let new_gpu_resource = GpuResource {
                            kind: upload.kind,
                            backend_token,
                        };
                        registry.swap_gpu(
                            upload.handle,
                            new_gpu_resource,
                            self.gpu_destroy_delay_frames,
                        )?;
                        uploaded.push(upload.handle);
                    }
                    Err(error) => {
                        // GPU upload failed - mark as failed
                        registry.mark_failed(upload.handle, &error.to_string())?;
                    }
                }
            }
        }

        Ok(uploaded)
    }

    /// Ticks the GPU destroy queue and returns backend tokens ready for destruction.
    ///
    /// The caller must destroy these GPU resources using the render backend.
    pub fn tick_gpu_destroy_queue(&mut self, registry: &mut AssetRegistry) -> Vec<u64> {
        registry.tick_gpu_destroy_queue()
    }

    /// Enqueues an import job to the worker thread.
    pub fn enqueue_import(&mut self, job: ImportJob) -> EngineResult<()> {
        if let Some(worker) = &self.import_worker {
            worker.enqueue(job)
        } else {
            Err(EngineError::other("import worker not available"))
        }
    }
}

/// Importer backend availability compiled into the current build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImporterBackend {
    /// Built-in lightweight importer.
    BuiltIn,
    /// FBX importer, present only with `fbx-importer`.
    #[cfg(feature = "fbx-importer")]
    Fbx,
    /// Assimp importer, present only with `assimp-importer`.
    #[cfg(feature = "assimp-importer")]
    Assimp,
}

/// Returns importer backends available in this build.
pub fn available_importers() -> Vec<ImporterBackend> {
    let mut importers = Vec::new();
    importers.push(ImporterBackend::BuiltIn);
    #[cfg(feature = "fbx-importer")]
    importers.push(ImporterBackend::Fbx);
    #[cfg(feature = "assimp-importer")]
    importers.push(ImporterBackend::Assimp);
    importers
}

/// Infers the resource kind and importer name for a source asset path.
pub fn infer_importer(path: &Path) -> Option<(ResourceKind, &'static str)> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)?;
    match extension.as_str() {
        "png" | "jpg" | "jpeg" => Some((ResourceKind::Texture, "image")),
        "amdl" => Some((ResourceKind::Model, "amdl")),
        "gltf" | "glb" => Some((ResourceKind::Model, "gltf")),
        "wgsl" | "glsl" => Some((ResourceKind::Shader, "shader-source")),
        "wav" | "ogg" => Some((ResourceKind::Audio, "audio")),
        "py" => Some((ResourceKind::Script, "script-python")),
        "varg" => Some((ResourceKind::Script, "script-varg")),
        "vscene" => Some((ResourceKind::Scene, "vscene")),
        "vasset" => Some((ResourceKind::Material, "vasset")),
        "json" => {
            let path_text = path.to_string_lossy();
            if path_text.contains("cubemap") || path_text.contains("skybox") {
                Some((ResourceKind::Texture, "cubemap-json"))
            } else if path_text.contains("material") {
                Some((ResourceKind::Material, "material-json"))
            } else if path_text.contains("prefab") {
                Some((ResourceKind::Prefab, "prefab-json"))
            } else {
                infer_scene_json(path)
            }
        }
        "toml" => {
            if path.to_string_lossy().contains("material") {
                Some((ResourceKind::Material, "material-toml"))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Detects whether a JSON file is a scene by checking for required top-level keys.
fn infer_scene_json(path: &Path) -> Option<(ResourceKind, &'static str)> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    match (
        value.get("version").and_then(|v| v.as_u64()),
        value.get("objects").and_then(|o| o.as_array()),
    ) {
        (Some(_), Some(_)) => Some((ResourceKind::Scene, "scene-json")),
        _ => None,
    }
}

static NEXT_GENERATED_GUID: AtomicU64 = AtomicU64::new(1);

fn generate_asset_guid(path: &Path) -> AssetGuid {
    let mut entropy = std::collections::hash_map::DefaultHasher::new();
    "aster-asset-guid-v2".hash(&mut entropy);
    path.hash(&mut entropy);
    std::process::id().hash(&mut entropy);
    let entropy = entropy.finish() as u128;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = NEXT_GENERATED_GUID.fetch_add(1, Ordering::Relaxed) as u128;
    AssetGuid::from_u128(timestamp ^ (counter << 64) ^ entropy)
}

fn meta_path_for_source(path: &Path) -> PathBuf {
    let mut meta_path = path.to_path_buf();
    if let Some(name) = path.file_name() {
        let mut meta_name = name.to_os_string();
        meta_name.push(".meta");
        meta_path.set_file_name(meta_name);
    } else {
        meta_path.set_extension("meta");
    }
    meta_path
}

fn read_resource_meta(path: &Path) -> EngineResult<Option<ResourceMetaFormat>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    ResourceMetaFormat::from_toml(&text)
        .map(Some)
        .map_err(EngineError::from)
}

fn write_resource_meta(path: &Path, meta: &ResourceMetaFormat) -> EngineResult<()> {
    let text =
        toml::to_string_pretty(meta).map_err(|error| EngineError::other(error.to_string()))?;
    fs::write(path, text).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

/// Result of scanning a project asset root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetScanReport {
    /// Metadata discovered or generated during the scan.
    pub metas: Vec<ResourceMetaFormat>,
    /// Files ignored because no importer accepts them.
    pub ignored: Vec<PathBuf>,
}

/// Scans a project asset root and registers supported resources in the database.
pub fn scan_project_assets(
    asset_root: impl AsRef<Path>,
    database: &mut AssetDatabase,
) -> EngineResult<AssetScanReport> {
    let asset_root = asset_root.as_ref();
    let mut report = AssetScanReport::default();
    if !asset_root.exists() {
        return Ok(report);
    }

    let mut stack = vec![asset_root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let entries = fs::read_dir(&path).map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) == Some("meta") {
                continue;
            }
            let relative = path.strip_prefix(asset_root).unwrap_or(&path).to_path_buf();
            let detected = infer_importer(&relative).or_else(|| {
                if path.extension().and_then(|value| value.to_str()) == Some("json") {
                    infer_scene_json(&path)
                } else {
                    None
                }
            });
            let Some((kind, importer)) = detected else {
                report.ignored.push(relative);
                continue;
            };
            let meta_path = meta_path_for_source(&path);
            let previous = read_resource_meta(&meta_path)?;
            let meta = match previous.clone() {
                Some(mut meta) => {
                    meta.version = CURRENT_SCHEMA_VERSION;
                    meta.source_path = relative;
                    meta.kind = kind;
                    meta.importer = importer.to_string();
                    meta.dependencies = discover_asset_dependencies(&path, kind, importer)?;
                    meta
                }
                None => ResourceMetaFormat {
                    version: CURRENT_SCHEMA_VERSION,
                    guid: generate_asset_guid(&relative),
                    source_path: relative,
                    kind,
                    importer: importer.to_string(),
                    dependencies: discover_asset_dependencies(&path, kind, importer)?,
                },
            };
            if previous.as_ref() != Some(&meta) {
                write_resource_meta(&meta_path, &meta)?;
            }
            database
                .upsert_meta(meta.clone())
                .map_err(EngineError::from)?;
            report.metas.push(meta);
        }
    }
    report
        .metas
        .sort_by(|left, right| left.source_path.cmp(&right.source_path));
    report.ignored.sort();
    Ok(report)
}

/// File system event kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileEventKind {
    /// File was created.
    Created,
    /// File was modified.
    Modified,
    /// File was removed.
    Removed,
    /// File was renamed.
    Renamed,
}

/// File system event from the watcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEvent {
    /// Path relative to the watched root.
    pub path: PathBuf,
    /// Event kind.
    pub kind: FileEventKind,
}

/// File watcher for asset change detection with debouncing.
pub struct FileWatcher {
    _watcher: notify::RecommendedWatcher,
    receiver: Receiver<notify::Result<notify::Event>>,
    root: PathBuf,
    debounce_buffer: HashMap<PathBuf, (FileEventKind, SystemTime)>,
    debounce_duration: std::time::Duration,
}

impl FileWatcher {
    /// Starts watching the given directory for file changes.
    pub fn start(asset_root: impl AsRef<Path>) -> EngineResult<Self> {
        use notify::{RecursiveMode, Watcher};

        let asset_root = asset_root.as_ref();
        let (sender, receiver) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(sender)
            .map_err(|e| EngineError::other(format!("Failed to create file watcher: {}", e)))?;

        watcher
            .watch(asset_root, RecursiveMode::Recursive)
            .map_err(|e| {
                EngineError::other(format!(
                    "Failed to watch directory {}: {}",
                    asset_root.display(),
                    e
                ))
            })?;

        Ok(Self {
            _watcher: watcher,
            receiver,
            root: asset_root.to_path_buf(),
            debounce_buffer: HashMap::new(),
            debounce_duration: std::time::Duration::from_millis(200),
        })
    }

    /// Polls for file events, returning debounced events.
    ///
    /// Modified events within 200ms window are debounced to only emit the latest.
    pub fn poll_events(&mut self) -> Vec<FileEvent> {
        let now = SystemTime::now();

        // Drain all pending events from the channel
        while let Ok(result) = self.receiver.try_recv() {
            if let Ok(event) = result {
                for path in &event.paths {
                    // Skip .meta files
                    if path.extension().and_then(|e| e.to_str()) == Some("meta") {
                        continue;
                    }

                    // Convert to relative path
                    let relative = match path.strip_prefix(&self.root) {
                        Ok(rel) => rel.to_path_buf(),
                        Err(_) => continue,
                    };

                    let kind = match event.kind {
                        notify::EventKind::Create(_) => FileEventKind::Created,
                        notify::EventKind::Modify(_) => FileEventKind::Modified,
                        notify::EventKind::Remove(_) => FileEventKind::Removed,
                        _ => continue,
                    };

                    // Buffer the event with timestamp
                    self.debounce_buffer.insert(relative, (kind, now));
                }
            }
        }

        // Collect events that are past the debounce window
        let mut ready_events = Vec::new();
        self.debounce_buffer.retain(|path, (kind, timestamp)| {
            if let Ok(elapsed) = now.duration_since(*timestamp) {
                if elapsed >= self.debounce_duration {
                    ready_events.push(FileEvent {
                        path: path.clone(),
                        kind: kind.clone(),
                    });
                    return false; // Remove from buffer
                }
            }
            true // Keep in buffer
        });

        ready_events
    }
}

impl AssetDatabase {
    /// Handles a file system event by updating the database state.
    ///
    /// - Modified: marks asset as Stale and enqueues reimport
    /// - Created: adds new ResourceMeta with Unloaded state
    /// - Removed: removes ResourceMeta from database
    ///
    /// Returns the GUID of the affected asset if an import should be queued.
    pub fn handle_event(&mut self, event: &FileEvent) -> EngineResult<Option<AssetGuid>> {
        match event.kind {
            FileEventKind::Modified => {
                // Mark existing asset as stale and return GUID for reimport
                if let Some(meta) = self.entries.get_mut(&event.path) {
                    meta.import_state = ResourceState::Stale;
                    return Ok(Some(meta.guid));
                }
                Ok(None)
            }
            FileEventKind::Created => {
                // Add new asset with Unloaded state
                let absolute_path = self.project_root.join(&event.path);
                if let Some((kind, importer)) = infer_importer(&event.path) {
                    let guid = self
                        .path_to_guid
                        .get(&event.path)
                        .copied()
                        .unwrap_or_else(|| generate_asset_guid(&event.path));

                    let meta = ResourceMeta {
                        guid,
                        path: event.path.clone(),
                        kind,
                        import_state: ResourceState::Unloaded,
                    };
                    self.entries.insert(event.path.clone(), meta);

                    // Also register in persistent metadata
                    let meta_format = ResourceMetaFormat {
                        version: CURRENT_SCHEMA_VERSION,
                        guid,
                        source_path: event.path.clone(),
                        kind,
                        importer: importer.to_string(),
                        dependencies: Vec::new(),
                    };
                    self.upsert_meta(meta_format)?;
                    return Ok(Some(guid));
                } else if absolute_path.extension().and_then(|e| e.to_str()) == Some("json") {
                    // Try content-based detection for JSON files
                    if let Some((kind, importer)) = infer_scene_json(&absolute_path) {
                        let guid = self
                            .path_to_guid
                            .get(&event.path)
                            .copied()
                            .unwrap_or_else(|| generate_asset_guid(&event.path));

                        let meta = ResourceMeta {
                            guid,
                            path: event.path.clone(),
                            kind,
                            import_state: ResourceState::Unloaded,
                        };
                        self.entries.insert(event.path.clone(), meta);

                        let meta_format = ResourceMetaFormat {
                            version: CURRENT_SCHEMA_VERSION,
                            guid,
                            source_path: event.path.clone(),
                            kind,
                            importer: importer.to_string(),
                            dependencies: Vec::new(),
                        };
                        self.upsert_meta(meta_format)?;
                        return Ok(Some(guid));
                    }
                }
                Ok(None)
            }
            FileEventKind::Removed => {
                // Remove asset from database and return GUID for cleanup
                if let Some(meta) = self.entries.remove(&event.path) {
                    return Ok(Some(meta.guid));
                }
                Ok(None)
            }
            FileEventKind::Renamed => {
                // Treat as remove + create (handled by separate events)
                Ok(None)
            }
        }
    }
}

fn discover_asset_dependencies(
    path: &Path,
    kind: ResourceKind,
    importer: &str,
) -> EngineResult<Vec<AssetGuid>> {
    if kind != ResourceKind::Material {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    let material = if importer == "material-toml" {
        MaterialFormat::from_toml(&text)
    } else {
        MaterialFormat::from_json(&text)
    }
    .map_err(EngineError::from)?;
    let mut dependencies = material.textures.values().copied().collect::<Vec<_>>();
    dependencies.push(material.shader);
    dependencies.sort();
    dependencies.dedup();
    Ok(dependencies)
}

/// Runs a built-in import task into CPU cache and queues a GPU upload.
pub fn import_builtin_asset(
    project_asset_root: impl AsRef<Path>,
    registry: &mut AssetRegistry,
    task: ImportTask,
) -> EngineResult<ImportOutcome> {
    let handle = registry.register(task.guid, task.kind)?;
    registry.set_state(handle, ResourceState::LoadingCpu)?;
    let path = project_asset_root.as_ref().join(&task.source_path);
    let mut file = fs::File::open(&path).map_err(|source| EngineError::Filesystem {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
    let imported = import_cpu_payload(&path, task.kind, &task.importer, &bytes);
    registry.put_cpu(
        handle,
        CpuResource {
            kind: task.kind,
            bytes: imported.bytes,
        },
    )?;
    registry.set_preview(
        handle,
        PreviewData {
            thumbnail: None,
            summary: imported.summary,
        },
    )?;
    Ok(ImportOutcome {
        guid: task.guid,
        diagnostics: imported.diagnostics,
        upload: Some(GpuUploadTask {
            handle,
            kind: task.kind,
        }),
    })
}

struct ImportedCpuPayload {
    bytes: Arc<[u8]>,
    summary: String,
    diagnostics: Vec<AssetDiagnostic>,
}

fn import_cpu_payload(
    path: &Path,
    kind: ResourceKind,
    importer: &str,
    bytes: &[u8],
) -> ImportedCpuPayload {
    match kind {
        ResourceKind::Texture => import_texture_payload(path, importer, bytes),
        ResourceKind::Model | ResourceKind::SkinnedModel => {
            import_model_payload(path, importer, bytes)
        }
        ResourceKind::Shader => import_shader_payload(path, importer, bytes),
        ResourceKind::Material => import_material_payload(path, importer, bytes),
        ResourceKind::Audio
        | ResourceKind::Animation
        | ResourceKind::Script
        | ResourceKind::Prefab
        | ResourceKind::Scene => ImportedCpuPayload {
            bytes: Arc::from(bytes),
            summary: format!("{} bytes imported by {}", bytes.len(), importer),
            diagnostics: Vec::new(),
        },
    }
}

fn import_texture_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    if importer == "cubemap-json" {
        return import_cubemap_payload(path, importer, bytes);
    }

    let mut diagnostics = Vec::new();
    let (payload, summary) = match image::load_from_memory(bytes) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            let width = rgba.width();
            let height = rgba.height();
            let texture = DecodedTextureResource {
                width,
                height,
                format: "rgba8_srgb".to_string(),
                pixels: rgba.into_raw(),
            };
            match texture.to_bytes() {
                Ok(bytes) => (
                    bytes,
                    format!("decoded {width}x{height} rgba8_srgb texture by {importer}"),
                ),
                Err(error) => {
                    diagnostics.push(
                        AssetDiagnostic::new(format!("texture encode failed: {error}"))
                            .with_path(path),
                    );
                    (
                        Arc::from(bytes),
                        format!(
                            "{} bytes texture source imported by {importer}",
                            bytes.len()
                        ),
                    )
                }
            }
        }
        Err(error) => {
            diagnostics.push(
                AssetDiagnostic::new(format!("texture decode failed: {error}")).with_path(path),
            );
            let summary = if let Some((format, width, height)) = parse_image_dimensions(bytes) {
                format!("{format} {width}x{height} texture source imported by {importer}")
            } else {
                format!(
                    "{} bytes texture source imported by {importer}",
                    bytes.len()
                )
            };
            (Arc::from(bytes), summary)
        }
    };
    ImportedCpuPayload {
        bytes: payload,
        summary,
        diagnostics,
    }
}

fn import_cubemap_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    let mut diagnostics = Vec::new();
    let source = match serde_json::from_slice::<CubemapSource>(bytes) {
        Ok(source) => source,
        Err(error) => {
            diagnostics.push(
                AssetDiagnostic::new(format!("cubemap manifest parse failed: {error}"))
                    .with_path(path),
            );
            return ImportedCpuPayload {
                bytes: Arc::from(bytes),
                summary: format!(
                    "{} bytes cubemap source imported by {importer}",
                    bytes.len()
                ),
                diagnostics,
            };
        }
    };

    let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let faces = [
        source.positive_x,
        source.negative_x,
        source.positive_y,
        source.negative_y,
        source.positive_z,
        source.negative_z,
    ];
    let mut face_size = None;
    let mut pixels = Vec::new();
    for face in faces {
        let face_path = base_dir.join(&face);
        let face_bytes = match fs::read(&face_path) {
            Ok(bytes) => bytes,
            Err(source) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("cubemap face read failed: {source}"))
                        .with_path(face_path),
                );
                return ImportedCpuPayload {
                    bytes: Arc::from(bytes),
                    summary: format!(
                        "{} bytes cubemap source imported by {importer}",
                        bytes.len()
                    ),
                    diagnostics,
                };
            }
        };
        let image = match image::load_from_memory(&face_bytes) {
            Ok(image) => image.to_rgba8(),
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("cubemap face decode failed: {error}"))
                        .with_path(face_path),
                );
                return ImportedCpuPayload {
                    bytes: Arc::from(bytes),
                    summary: format!(
                        "{} bytes cubemap source imported by {importer}",
                        bytes.len()
                    ),
                    diagnostics,
                };
            }
        };
        if image.width() != image.height() {
            diagnostics
                .push(AssetDiagnostic::new("cubemap face must be square").with_path(face_path));
            return ImportedCpuPayload {
                bytes: Arc::from(bytes),
                summary: format!(
                    "{} bytes cubemap source imported by {importer}",
                    bytes.len()
                ),
                diagnostics,
            };
        }
        match face_size {
            Some(size) if size != image.width() => {
                diagnostics.push(
                    AssetDiagnostic::new("all cubemap faces must have identical dimensions")
                        .with_path(face_path),
                );
                return ImportedCpuPayload {
                    bytes: Arc::from(bytes),
                    summary: format!(
                        "{} bytes cubemap source imported by {importer}",
                        bytes.len()
                    ),
                    diagnostics,
                };
            }
            Some(_) => {}
            None => face_size = Some(image.width()),
        }
        pixels.extend_from_slice(&image.into_raw());
    }

    let face_size = face_size.unwrap_or(1);
    let cubemap = DecodedCubemapResource {
        face_size,
        format: "cubemap_rgba8_srgb".to_string(),
        pixels,
    };
    let payload = match cubemap.to_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            diagnostics.push(
                AssetDiagnostic::new(format!("cubemap encode failed: {error}")).with_path(path),
            );
            Arc::from(bytes)
        }
    };

    ImportedCpuPayload {
        bytes: payload,
        summary: format!("decoded {face_size}x{face_size}x6 rgba8_srgb cubemap by {importer}"),
        diagnostics,
    }
}

fn import_model_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    let mut diagnostics = Vec::new();
    let (payload, summary) = if importer == "amdl" {
        match std::str::from_utf8(bytes)
            .map_err(|error| error.to_string())
            .and_then(|source| {
                compile_amdl(source).map_err(|diagnostics| {
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| diagnostic.message)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
            }) {
            Ok(document) => match serde_json::to_vec(&document) {
                Ok(encoded) => {
                    let model_count = document.models.len();
                    (
                        Arc::from(encoded),
                        format!(
                            "Aster model declaration imported by {importer}: {model_count} models"
                        ),
                    )
                }
                Err(error) => {
                    diagnostics.push(
                        AssetDiagnostic::new(format!("Aster model encode failed: {error}"))
                            .with_path(path),
                    );
                    (
                        Arc::from(bytes),
                        format!("{} bytes model source imported by {importer}", bytes.len()),
                    )
                }
            },
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("Aster model parse failed: {error}"))
                        .with_path(path),
                );
                (
                    Arc::from(bytes),
                    format!("{} bytes model source imported by {importer}", bytes.len()),
                )
            }
        }
    } else if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("gltf") || extension.eq_ignore_ascii_case("glb")
        })
    {
        match import_gltf_model(path) {
            Ok(model) => {
                let primitive_count = model.meshes.len();
                match model.to_bytes() {
                    Ok(bytes) => (
                        bytes,
                        format!(
                            "glTF model imported by {importer}: {primitive_count} mesh primitives"
                        ),
                    ),
                    Err(error) => {
                        diagnostics.push(
                            AssetDiagnostic::new(format!("model encode failed: {error}"))
                                .with_path(path),
                        );
                        (
                            Arc::from(bytes),
                            format!("{} bytes model source imported by {importer}", bytes.len()),
                        )
                    }
                }
            }
            Err(error) => {
                diagnostics.push(
                    AssetDiagnostic::new(format!("glTF import failed: {error}")).with_path(path),
                );
                (
                    Arc::from(bytes),
                    format!("{} bytes model source imported by {importer}", bytes.len()),
                )
            }
        }
    } else {
        (
            Arc::from(bytes),
            format!("{} bytes model source imported by {importer}", bytes.len()),
        )
    };
    ImportedCpuPayload {
        bytes: payload,
        summary,
        diagnostics,
    }
}

fn import_gltf_model(path: &Path) -> EngineResult<ModelResource> {
    let (document, buffers, _) =
        gltf::import(path).map_err(|error| EngineError::other(error.to_string()))?;
    let mut model = ModelResource::default();

    // Extract materials
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        let metallic = pbr.metallic_factor();
        let roughness = pbr.roughness_factor();
        let emissive = material.emissive_factor();

        let alpha_mode = match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => "OPAQUE",
            gltf::material::AlphaMode::Blend => "BLEND",
            gltf::material::AlphaMode::Mask => "MASK",
        };
        let alpha_cutoff = material.alpha_cutoff().unwrap_or(0.5);

        // Extract texture references (store as relative paths for AssetDatabase resolution)
        let base_color_texture_ref = pbr.base_color_texture().and_then(|info| {
            let source = info.texture().source().source();
            match source {
                gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
                _ => None,
            }
        });

        let normal_texture_ref = material.normal_texture().and_then(|info| {
            let source = info.texture().source().source();
            match source {
                gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
                _ => None,
            }
        });

        let metallic_roughness_texture_ref = pbr.metallic_roughness_texture().and_then(|info| {
            let source = info.texture().source().source();
            match source {
                gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
                _ => None,
            }
        });

        model.materials.push(CpuMaterialResource {
            name: material.name().unwrap_or("").to_string(),
            base_color,
            metallic,
            roughness,
            emissive,
            alpha_mode: alpha_mode.to_string(),
            alpha_cutoff,
            base_color_texture_ref,
            normal_texture_ref,
            metallic_roughness_texture_ref,
        });
    }

    // Extract meshes
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &**data));
            let positions = reader
                .read_positions()
                .map(|items| items.collect::<Vec<_>>())
                .unwrap_or_default();
            if positions.is_empty() {
                continue;
            }
            // Read normals, or fill with default (0, 1, 0) if missing
            let normals = reader
                .read_normals()
                .map(|items| items.collect::<Vec<_>>())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let texcoords = reader
                .read_tex_coords(0)
                .map(|items| items.into_f32().collect::<Vec<_>>())
                .unwrap_or_default();
            let indices = reader
                .read_indices()
                .map(|items| items.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            model.meshes.push(BasicMeshResource {
                positions,
                normals,
                texcoords,
                indices,
                material_index: primitive.material().index(),
            });
        }
    }
    Ok(model)
}

fn import_material_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    let mut diagnostics = Vec::new();
    let material = if importer == "material-toml" {
        std::str::from_utf8(bytes)
            .map_err(|error| AssetError::Parse {
                format: "material",
                diagnostic: AssetDiagnostic::new(error.to_string()).with_path(path),
            })
            .and_then(MaterialFormat::from_toml)
    } else {
        std::str::from_utf8(bytes)
            .map_err(|error| AssetError::Parse {
                format: "material",
                diagnostic: AssetDiagnostic::new(error.to_string()).with_path(path),
            })
            .and_then(MaterialFormat::from_json)
    };
    let summary = match material {
        Ok(material) => format!(
            "material imported by {importer}: {} textures, {} parameters",
            material.textures.len(),
            material.parameters.len()
        ),
        Err(error) => {
            diagnostics.push(AssetDiagnostic::new(error.to_string()).with_path(path));
            format!(
                "{} bytes material source imported by {importer}",
                bytes.len()
            )
        }
    };
    ImportedCpuPayload {
        bytes: Arc::from(bytes),
        summary,
        diagnostics,
    }
}

fn import_shader_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    let mut diagnostics = Vec::new();
    if std::str::from_utf8(bytes).is_err() {
        diagnostics.push(
            AssetDiagnostic::new("shader source is not valid UTF-8; queued raw bytes")
                .with_path(path),
        );
    }
    ImportedCpuPayload {
        bytes: Arc::from(bytes),
        summary: format!(
            "{} bytes shader source imported by {}",
            bytes.len(),
            importer
        ),
        diagnostics,
    }
}

fn parse_image_dimensions(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    parse_png_dimensions(bytes).or_else(|| parse_jpeg_dimensions(bytes))
}

fn parse_png_dimensions(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    if bytes.len() < 24 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some(("png", width, height))
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let mut cursor = 2;
    while cursor + 9 < bytes.len() {
        if bytes[cursor] != 0xff {
            cursor += 1;
            continue;
        }
        let marker = bytes[cursor + 1];
        cursor += 2;
        if marker == 0xd8 || marker == 0xd9 {
            continue;
        }
        if cursor + 2 > bytes.len() {
            return None;
        }
        let segment_len = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        if segment_len < 2 || cursor + segment_len > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            let height = u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]) as u32;
            return Some(("jpeg", width, height));
        }
        cursor += segment_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guid(value: u128) -> AssetGuid {
        AssetGuid::from_u128(value)
    }

    #[test]
    fn manifest_upsert_replaces_by_guid() {
        let id = guid(7);
        let mut manifest = ResourceManifestFormat::default();
        manifest.upsert(AssetManifestEntry {
            guid: id,
            path: AssetPath::new("old.mesh"),
            kind: ResourceKind::Model,
            dependencies: Vec::new(),
        });
        manifest.upsert(AssetManifestEntry {
            guid: id,
            path: AssetPath::new("new.mesh"),
            kind: ResourceKind::Model,
            dependencies: Vec::new(),
        });

        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(
            manifest.get(id).unwrap().path.to_utf8().unwrap(),
            "new.mesh"
        );
    }

    #[test]
    fn database_resolves_guid_and_dependencies() {
        let mut database = AssetDatabase::new("assets", "builtin");
        database
            .upsert_meta(ResourceMetaFormat {
                version: CURRENT_SCHEMA_VERSION,
                guid: guid(1),
                source_path: PathBuf::from("materials/player.aster_material.json"),
                kind: ResourceKind::Material,
                importer: "material-json".to_string(),
                dependencies: vec![guid(2)],
            })
            .unwrap();

        assert_eq!(
            database.resolve_guid(guid(1)).unwrap().to_utf8().unwrap(),
            "materials/player.aster_material.json"
        );
        assert_eq!(database.dependencies().dependencies(guid(1)), vec![guid(2)]);
        assert_eq!(database.dependencies().dependents(guid(2)), vec![guid(1)]);
    }

    #[test]
    fn registry_keeps_cpu_and_gpu_cache_lifetimes_separate() {
        let mut registry = AssetRegistry::default();
        let handle = registry.register(guid(9), ResourceKind::Texture).unwrap();
        registry
            .put_cpu(
                handle,
                CpuResource {
                    kind: ResourceKind::Texture,
                    bytes: Arc::<[u8]>::from([1_u8, 2, 3]),
                },
            )
            .unwrap();
        registry
            .put_gpu(
                handle,
                GpuResource {
                    kind: ResourceKind::Texture,
                    backend_token: 42,
                },
            )
            .unwrap();

        registry.drop_cpu(handle);

        assert!(!registry.cpu_cache.contains_key(&handle));
        assert!(registry.gpu_cache.contains_key(&handle));
        assert_eq!(
            registry.record(handle).unwrap().state,
            ResourceState::GpuReady
        );
    }

    #[test]
    fn import_queue_separates_import_and_upload_work() {
        let handle = ResourceHandle::new(
            ResourceId::from_u128(1),
            Handle::new(0, engine_core::Generation::FIRST),
        );
        let mut queue = ImportQueue::default();
        queue.push_import(ImportTask {
            guid: guid(1),
            source_path: PathBuf::from("textures/a.png"),
            kind: ResourceKind::Texture,
            importer: "image".to_string(),
        });

        let outcomes = queue.drain_imports_parallel(2, |_| ImportOutcome {
            guid: guid(1),
            diagnostics: Vec::new(),
            upload: Some(GpuUploadTask {
                handle,
                kind: ResourceKind::Texture,
            }),
        });

        assert_eq!(outcomes.len(), 1);
        assert_eq!(queue.pop_upload().unwrap().handle, handle);
    }

    #[test]
    fn runtime_min_has_only_builtin_importer_by_default() {
        let mut expected = Vec::new();
        expected.push(ImporterBackend::BuiltIn);
        #[cfg(feature = "fbx-importer")]
        expected.push(ImporterBackend::Fbx);
        #[cfg(feature = "assimp-importer")]
        expected.push(ImporterBackend::Assimp);

        assert_eq!(available_importers(), expected);
    }

    #[test]
    fn scans_and_imports_supported_assets() {
        let root = std::env::temp_dir().join(format!("aster-assets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/player.png"), [1_u8, 2, 3, 4]).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let report = scan_project_assets(&root, &mut database).unwrap();
        let meta = report
            .metas
            .iter()
            .find(|meta| meta.source_path == PathBuf::from("textures/player.png"))
            .unwrap();
        assert!(root.join("textures/player.png.meta").exists());

        let mut registry = AssetRegistry::default();
        let outcome = import_builtin_asset(
            &root,
            &mut registry,
            ImportTask {
                guid: meta.guid,
                source_path: meta.source_path.clone(),
                kind: meta.kind,
                importer: meta.importer.clone(),
            },
        )
        .unwrap();

        assert!(outcome.upload.is_some());
        assert_eq!(
            registry
                .record(outcome.upload.unwrap().handle)
                .unwrap()
                .state,
            ResourceState::CpuReady
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_png_as_decoded_texture_payload() {
        let root =
            std::env::temp_dir().join(format!("aster-texture-decode-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/white.png"), one_pixel_png()).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let report = scan_project_assets(&root, &mut database).unwrap();
        let meta = report
            .metas
            .iter()
            .find(|meta| meta.source_path == PathBuf::from("textures/white.png"))
            .unwrap();
        let mut registry = AssetRegistry::default();
        import_builtin_asset(
            &root,
            &mut registry,
            ImportTask {
                guid: meta.guid,
                source_path: meta.source_path.clone(),
                kind: meta.kind,
                importer: meta.importer.clone(),
            },
        )
        .unwrap();

        let handle = registry.handle_for_guid(meta.guid).unwrap();
        let texture =
            DecodedTextureResource::from_bytes(&registry.cpu_resource(handle).unwrap().bytes)
                .unwrap();

        assert_eq!(texture.width, 1);
        assert_eq!(texture.height, 1);
        assert_eq!(texture.format, "rgba8_srgb");
        assert_eq!(texture.pixels, vec![255, 255, 255, 255]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_cubemap_manifest_as_decoded_cube_payload() {
        let root =
            std::env::temp_dir().join(format!("aster-cubemap-decode-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures/cube")).unwrap();
        for name in ["px", "nx", "py", "ny", "pz", "nz"] {
            std::fs::write(
                root.join(format!("textures/cube/{name}.png")),
                one_pixel_png(),
            )
            .unwrap();
        }
        std::fs::write(
            root.join("textures/skybox.cubemap.json"),
            r#"{
  "positive_x": "cube/px.png",
  "negative_x": "cube/nx.png",
  "positive_y": "cube/py.png",
  "negative_y": "cube/ny.png",
  "positive_z": "cube/pz.png",
  "negative_z": "cube/nz.png"
}"#,
        )
        .unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let report = scan_project_assets(&root, &mut database).unwrap();
        let meta = report
            .metas
            .iter()
            .find(|meta| meta.source_path == PathBuf::from("textures/skybox.cubemap.json"))
            .unwrap();
        assert_eq!(meta.kind, ResourceKind::Texture);
        assert_eq!(meta.importer, "cubemap-json");

        let mut registry = AssetRegistry::default();
        import_builtin_asset(
            &root,
            &mut registry,
            ImportTask {
                guid: meta.guid,
                source_path: meta.source_path.clone(),
                kind: meta.kind,
                importer: meta.importer.clone(),
            },
        )
        .unwrap();

        let handle = registry.handle_for_guid(meta.guid).unwrap();
        let cubemap =
            DecodedCubemapResource::from_bytes(&registry.cpu_resource(handle).unwrap().bytes)
                .unwrap();

        assert_eq!(cubemap.face_size, 1);
        assert_eq!(cubemap.format, "cubemap_rgba8_srgb");
        assert_eq!(cubemap.pixels.len(), 6 * 4);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn material_scan_records_shader_and_texture_dependencies() {
        let root =
            std::env::temp_dir().join(format!("aster-material-deps-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("materials")).unwrap();
        std::fs::write(
            root.join("materials/player.material.json"),
            format!(
                r#"{{
  "version": 1,
  "shader": "{shader}",
  "textures": {{"albedo": "{texture}"}},
  "parameters": {{}}
}}"#,
                shader = guid(11),
                texture = guid(12),
            ),
        )
        .unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let report = scan_project_assets(&root, &mut database).unwrap();
        let material = report.metas.first().unwrap();

        assert_eq!(
            database.dependencies().dependencies(material.guid),
            vec![guid(11), guid(12)]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn one_pixel_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn scan_preserves_guid_from_moved_meta_file() {
        let root =
            std::env::temp_dir().join(format!("aster-assets-meta-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/player.png"), [1_u8, 2, 3, 4]).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let first = scan_project_assets(&root, &mut database).unwrap();
        let guid = first.metas[0].guid;

        std::fs::rename(
            root.join("textures/player.png"),
            root.join("textures/hero.png"),
        )
        .unwrap();
        std::fs::rename(
            root.join("textures/player.png.meta"),
            root.join("textures/hero.png.meta"),
        )
        .unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        let second = scan_project_assets(&root, &mut database).unwrap();
        assert_eq!(second.metas[0].guid, guid);
        assert_eq!(
            second.metas[0].source_path,
            PathBuf::from("textures/hero.png")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_registers_files_with_correct_resource_kinds() {
        let root = std::env::temp_dir().join(format!("aster-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Create subdirectories
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        std::fs::create_dir_all(root.join("scenes")).unwrap();

        // Texture: .png file
        std::fs::write(root.join("textures/player.png"), one_pixel_png()).unwrap();
        // Model: .gltf file (minimal ASCII glTF JSON)
        let gltf_json = r#"{"asset":{"version":"2.0"}}"#;
        std::fs::write(root.join("models/hero.gltf"), gltf_json).unwrap();
        // Shader: .wgsl file
        std::fs::write(root.join("shaders/pbr.wgsl"), "fn main() {}").unwrap();
        // Script: .varg file
        std::fs::write(
            root.join("scripts/player.varg"),
            "script Player { func update(_ dt: Float) {} }",
        )
        .unwrap();
        std::fs::write(
            root.join("scripts/player.py"),
            "def update(ctx):\n    pass\n",
        )
        .unwrap();
        // Scene: JSON file with version + objects
        let scene_json = r#"{"version":1,"name":"test","objects":[]}"#;
        std::fs::write(root.join("scenes/level.scene.json"), scene_json).unwrap();
        // Non-asset file (should be ignored)
        std::fs::write(root.join("readme.txt"), "hello").unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        database.scan(&root).unwrap();

        // Verify all supported files are registered with correct kinds
        assert_eq!(
            database
                .entry_for_path(Path::new("textures/player.png"))
                .unwrap()
                .kind,
            ResourceKind::Texture,
            "PNG files should map to Texture"
        );
        assert_eq!(
            database
                .entry_for_path(Path::new("models/hero.gltf"))
                .unwrap()
                .kind,
            ResourceKind::Model,
            "glTF files should map to Model"
        );
        assert_eq!(
            database
                .entry_for_path(Path::new("shaders/pbr.wgsl"))
                .unwrap()
                .kind,
            ResourceKind::Shader,
            "WGSL files should map to Shader"
        );
        assert_eq!(
            database
                .entry_for_path(Path::new("scripts/player.varg"))
                .unwrap()
                .kind,
            ResourceKind::Script,
            "Varg script files should map to Script"
        );
        assert_eq!(
            database
                .entry_for_path(Path::new("scripts/player.py"))
                .unwrap()
                .kind,
            ResourceKind::Script,
            "Python files should map to Script"
        );
        assert_eq!(
            database
                .entry_for_path(Path::new("scenes/level.scene.json"))
                .unwrap()
                .kind,
            ResourceKind::Scene,
            "Scene JSON files should map to Scene"
        );

        // All entries should start with Unloaded import state
        for entry in database.iter_entries() {
            assert_eq!(
                entry.import_state,
                ResourceState::Unloaded,
                "import_state should default to Unloaded"
            );
        }

        // Non-asset file should not be registered
        assert!(
            database
                .entry_for_path(&PathBuf::from("readme.txt"))
                .is_none(),
            "Unsupported files should not be registered"
        );

        // Folder entries should be tracked
        let folders = database.folders();
        assert!(
            folders.contains(&PathBuf::from("textures")),
            "textures folder should be tracked"
        );
        assert!(
            folders.contains(&PathBuf::from("models")),
            "models folder should be tracked"
        );
        assert!(
            folders.contains(&PathBuf::from("scripts")),
            "scripts folder should be tracked"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_removes_deleted_files() {
        let root =
            std::env::temp_dir().join(format!("aster-scan-delete-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/a.png"), one_pixel_png()).unwrap();
        std::fs::write(root.join("textures/b.png"), one_pixel_png()).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        database.scan(&root).unwrap();
        assert_eq!(database.iter_entries().count(), 2);

        // Delete b.png and rescan
        std::fs::remove_file(root.join("textures/b.png")).unwrap();
        database.scan(&root).unwrap();

        assert_eq!(database.iter_entries().count(), 1);
        assert!(
            database
                .entry_for_path(&PathBuf::from("textures/a.png"))
                .is_some()
        );
        assert!(
            database
                .entry_for_path(&PathBuf::from("textures/b.png"))
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_preserves_existing_guid_on_rescan() {
        let root =
            std::env::temp_dir().join(format!("aster-scan-guid-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/player.png"), one_pixel_png()).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        database.scan(&root).unwrap();
        let first_guid = database
            .entry_for_path(&PathBuf::from("textures/player.png"))
            .unwrap()
            .guid;

        // Rescan without changing any files
        database.scan(&root).unwrap();
        let second_guid = database
            .entry_for_path(&PathBuf::from("textures/player.png"))
            .unwrap()
            .guid;

        assert_eq!(
            first_guid, second_guid,
            "GUID should be preserved across rescans"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn png_importer_imports_valid_png_with_mips() {
        let root =
            std::env::temp_dir().join(format!("aster-png-importer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a 4x4 white PNG
        let png_path = root.join("test.png");
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        image.save(&png_path).unwrap();

        // Import with mip generation
        let options = ImportOptions {
            generate_mips: true,
            max_texture_size: None,
        };
        let outcome = PngImporter::import(&png_path, &options).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "Valid PNG should import without diagnostics"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn png_importer_to_registry_stores_texture_resource() {
        let root =
            std::env::temp_dir().join(format!("aster-png-registry-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a 4x4 white PNG
        let png_path = root.join("test.png");
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        image.save(&png_path).unwrap();

        let mut registry = AssetRegistry::default();
        let test_guid = guid(999);

        // Import with mip generation
        let options = ImportOptions {
            generate_mips: true,
            max_texture_size: None,
        };
        let outcome =
            PngImporter::import_to_registry(&png_path, &options, &mut registry, test_guid).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "Valid PNG should import without diagnostics"
        );
        assert!(
            outcome.upload.is_some(),
            "Valid PNG should queue GPU upload"
        );

        // Verify the texture was stored in the registry
        let handle = registry.handle_for_guid(test_guid).unwrap();
        let cpu_resource = registry.cpu_resource(handle).unwrap();
        assert_eq!(cpu_resource.kind, ResourceKind::Texture);

        // Deserialize and verify the texture
        let texture = CpuTextureResource::from_bytes(&cpu_resource.bytes).unwrap();
        assert_eq!(texture.width, 4);
        assert_eq!(texture.height, 4);
        assert_eq!(texture.format, "Rgba8UnormSrgb");
        // 4x4 -> 2x2 -> 1x1 = 3 mip levels
        assert_eq!(texture.mip_levels.len(), 3);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn png_importer_handles_invalid_png() {
        let root =
            std::env::temp_dir().join(format!("aster-png-invalid-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create an invalid PNG file
        let png_path = root.join("invalid.png");
        std::fs::write(&png_path, b"not a png file").unwrap();

        let options = ImportOptions {
            generate_mips: false,
            max_texture_size: None,
        };
        let outcome = PngImporter::import(&png_path, &options).unwrap();

        assert!(
            !outcome.diagnostics.is_empty(),
            "Invalid PNG should produce at least one diagnostic"
        );
        assert!(
            outcome.upload.is_none(),
            "Invalid PNG should not queue upload"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mip_chain_generation_produces_correct_levels() {
        // Create a 4x4 image
        let base = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let mip_levels = generate_mip_chain(&base);

        // 4x4 -> 2x2 -> 1x1 = 3 levels
        assert_eq!(mip_levels.len(), 3, "4x4 image should produce 3 mip levels");

        // Level 0: 4x4 = 64 bytes (4*4*4)
        assert_eq!(mip_levels[0].len(), 64);

        // Level 1: 2x2 = 16 bytes (2*2*4)
        assert_eq!(mip_levels[1].len(), 16);

        // Level 2: 1x1 = 4 bytes (1*1*4)
        assert_eq!(mip_levels[2].len(), 4);
    }

    #[test]
    fn cpu_texture_resource_serialization_roundtrip() {
        let texture = CpuTextureResource {
            width: 2,
            height: 2,
            format: "Rgba8UnormSrgb".to_string(),
            mip_levels: vec![
                vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                ],
                vec![128, 128, 128, 255],
            ],
        };

        let bytes = texture.to_bytes().unwrap();
        let deserialized = CpuTextureResource::from_bytes(&bytes).unwrap();

        assert_eq!(texture, deserialized);
    }

    #[test]
    fn gltf_importer_imports_valid_gltf_with_mesh() {
        let root =
            std::env::temp_dir().join(format!("aster-gltf-importer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a minimal valid glTF file with a triangle mesh
        let gltf_path = root.join("test.gltf");
        create_minimal_gltf(&gltf_path);

        // Import the glTF
        let outcome = GltfImporter::import(&gltf_path).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "Valid glTF should import without diagnostics"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gltf_importer_to_registry_stores_model_resource() {
        let root =
            std::env::temp_dir().join(format!("aster-gltf-registry-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a minimal valid glTF file with a triangle mesh
        let gltf_path = root.join("test.gltf");
        create_minimal_gltf(&gltf_path);

        let mut registry = AssetRegistry::default();
        let test_guid = guid(888);

        // Import the glTF
        let outcome =
            GltfImporter::import_to_registry(&gltf_path, &mut registry, test_guid).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "Valid glTF should import without diagnostics"
        );
        assert!(
            outcome.upload.is_some(),
            "Valid glTF should queue GPU upload"
        );

        // Verify the model was stored in the registry
        let handle = registry.handle_for_guid(test_guid).unwrap();
        let cpu_resource = registry.cpu_resource(handle).unwrap();
        assert_eq!(cpu_resource.kind, ResourceKind::Model);

        // Deserialize and verify the model
        let model = ModelResource::from_bytes(&cpu_resource.bytes).unwrap();
        assert_eq!(model.meshes.len(), 1, "Should have 1 mesh primitive");
        assert_eq!(
            model.meshes[0].positions.len(),
            3,
            "Triangle should have 3 vertices"
        );
        assert_eq!(
            model.meshes[0].indices.len(),
            3,
            "Triangle should have 3 indices"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gltf_importer_handles_invalid_gltf() {
        let root =
            std::env::temp_dir().join(format!("aster-gltf-invalid-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create an invalid glTF file
        let gltf_path = root.join("invalid.gltf");
        std::fs::write(&gltf_path, b"not a gltf file").unwrap();

        let outcome = GltfImporter::import(&gltf_path).unwrap();

        assert!(
            !outcome.diagnostics.is_empty(),
            "Invalid glTF should produce at least one diagnostic"
        );
        assert!(
            outcome.upload.is_none(),
            "Invalid glTF should not queue upload"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gltf_importer_fills_default_normals_when_missing() {
        let root =
            std::env::temp_dir().join(format!("aster-gltf-normals-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a glTF file without normals
        let gltf_path = root.join("no_normals.gltf");
        create_gltf_without_normals(&gltf_path);

        let mut registry = AssetRegistry::default();
        let test_guid = guid(777);

        // Import the glTF
        let outcome =
            GltfImporter::import_to_registry(&gltf_path, &mut registry, test_guid).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "glTF without normals should still import successfully"
        );

        // Verify the model has default normals
        let handle = registry.handle_for_guid(test_guid).unwrap();
        let cpu_resource = registry.cpu_resource(handle).unwrap();
        let model = ModelResource::from_bytes(&cpu_resource.bytes).unwrap();

        assert_eq!(model.meshes.len(), 1);
        assert_eq!(
            model.meshes[0].normals.len(),
            3,
            "Should have normals for all 3 vertices"
        );
        // Default normal should be (0, 1, 0)
        assert_eq!(model.meshes[0].normals[0], [0.0, 1.0, 0.0]);
        assert_eq!(model.meshes[0].normals[1], [0.0, 1.0, 0.0]);
        assert_eq!(model.meshes[0].normals[2], [0.0, 1.0, 0.0]);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Creates a minimal valid glTF file with a single triangle mesh.
    fn create_minimal_gltf(path: &Path) {
        // Triangle vertices: positions and normals
        let positions: Vec<f32> = vec![
            0.0, 0.0, 0.0, // vertex 0
            1.0, 0.0, 0.0, // vertex 1
            0.0, 1.0, 0.0, // vertex 2
        ];
        let normals: Vec<f32> = vec![
            0.0, 0.0, 1.0, // normal 0
            0.0, 0.0, 1.0, // normal 1
            0.0, 0.0, 1.0, // normal 2
        ];
        let indices: Vec<u32> = vec![0, 1, 2];

        // Convert to bytes
        let positions_bytes: Vec<u8> = positions.iter().flat_map(|f| f.to_le_bytes()).collect();
        let normals_bytes: Vec<u8> = normals.iter().flat_map(|f| f.to_le_bytes()).collect();
        let indices_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();

        // Create binary buffer
        let mut buffer_data = Vec::new();
        let positions_offset = 0;
        let normals_offset = positions_bytes.len();
        let indices_offset = normals_offset + normals_bytes.len();
        buffer_data.extend_from_slice(&positions_bytes);
        buffer_data.extend_from_slice(&normals_bytes);
        buffer_data.extend_from_slice(&indices_bytes);

        // Write binary buffer
        let bin_path = path.with_extension("bin");
        std::fs::write(&bin_path, &buffer_data).unwrap();

        // Create glTF JSON
        let gltf_json = serde_json::json!({
            "asset": {
                "version": "2.0"
            },
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{
                "primitives": [{
                    "attributes": {
                        "POSITION": 0,
                        "NORMAL": 1
                    },
                    "indices": 2
                }]
            }],
            "accessors": [
                {
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3",
                    "min": [0.0, 0.0, 0.0],
                    "max": [1.0, 1.0, 0.0]
                },
                {
                    "bufferView": 1,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3"
                },
                {
                    "bufferView": 2,
                    "componentType": 5125,
                    "count": 3,
                    "type": "SCALAR"
                }
            ],
            "bufferViews": [
                {
                    "buffer": 0,
                    "byteOffset": positions_offset,
                    "byteLength": positions_bytes.len()
                },
                {
                    "buffer": 0,
                    "byteOffset": normals_offset,
                    "byteLength": normals_bytes.len()
                },
                {
                    "buffer": 0,
                    "byteOffset": indices_offset,
                    "byteLength": indices_bytes.len()
                }
            ],
            "buffers": [{
                "uri": bin_path.file_name().unwrap().to_str().unwrap(),
                "byteLength": buffer_data.len()
            }]
        });

        std::fs::write(path, serde_json::to_string_pretty(&gltf_json).unwrap()).unwrap();
    }

    /// Creates a glTF file without normals to test default normal filling.
    fn create_gltf_without_normals(path: &Path) {
        // Triangle vertices: positions only (no normals)
        let positions: Vec<f32> = vec![
            0.0, 0.0, 0.0, // vertex 0
            1.0, 0.0, 0.0, // vertex 1
            0.0, 1.0, 0.0, // vertex 2
        ];
        let indices: Vec<u32> = vec![0, 1, 2];

        // Convert to bytes
        let positions_bytes: Vec<u8> = positions.iter().flat_map(|f| f.to_le_bytes()).collect();
        let indices_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();

        // Create binary buffer
        let mut buffer_data = Vec::new();
        let positions_offset = 0;
        let indices_offset = positions_bytes.len();
        buffer_data.extend_from_slice(&positions_bytes);
        buffer_data.extend_from_slice(&indices_bytes);

        // Write binary buffer
        let bin_path = path.with_extension("bin");
        std::fs::write(&bin_path, &buffer_data).unwrap();

        // Create glTF JSON (no NORMAL attribute)
        let gltf_json = serde_json::json!({
            "asset": {
                "version": "2.0"
            },
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{
                "primitives": [{
                    "attributes": {
                        "POSITION": 0
                    },
                    "indices": 1
                }]
            }],
            "accessors": [
                {
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3",
                    "min": [0.0, 0.0, 0.0],
                    "max": [1.0, 1.0, 0.0]
                },
                {
                    "bufferView": 1,
                    "componentType": 5125,
                    "count": 3,
                    "type": "SCALAR"
                }
            ],
            "bufferViews": [
                {
                    "buffer": 0,
                    "byteOffset": positions_offset,
                    "byteLength": positions_bytes.len()
                },
                {
                    "buffer": 0,
                    "byteOffset": indices_offset,
                    "byteLength": indices_bytes.len()
                }
            ],
            "buffers": [{
                "uri": bin_path.file_name().unwrap().to_str().unwrap(),
                "byteLength": buffer_data.len()
            }]
        });

        std::fs::write(path, serde_json::to_string_pretty(&gltf_json).unwrap()).unwrap();
    }

    #[test]
    fn gltf_importer_extracts_pbr_material() {
        let root =
            std::env::temp_dir().join(format!("aster-gltf-material-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a glTF file with a PBR material
        let gltf_path = root.join("material.gltf");
        create_gltf_with_pbr_material(&gltf_path);

        let mut registry = AssetRegistry::default();
        let test_guid = guid(999);

        // Import the glTF
        let outcome =
            GltfImporter::import_to_registry(&gltf_path, &mut registry, test_guid).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "glTF with material should import without diagnostics"
        );

        // Verify the model has materials
        let handle = registry.handle_for_guid(test_guid).unwrap();
        let cpu_resource = registry.cpu_resource(handle).unwrap();
        let model = ModelResource::from_bytes(&cpu_resource.bytes).unwrap();

        assert_eq!(model.materials.len(), 1, "Should have 1 material");
        let material = &model.materials[0];
        assert_eq!(material.name, "TestMaterial");
        assert_eq!(material.base_color, [0.8, 0.2, 0.2, 1.0]);
        assert_eq!(material.metallic, 0.9);
        assert_eq!(material.roughness, 0.3);
        assert_eq!(material.emissive, [0.1, 0.1, 0.1]);
        assert_eq!(material.alpha_mode, "OPAQUE");
        assert_eq!(material.alpha_cutoff, 0.5);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gltf_importer_creates_default_material_when_none() {
        let root = std::env::temp_dir().join(format!(
            "aster-gltf-no-material-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a glTF file without materials (uses create_minimal_gltf which has no materials)
        let gltf_path = root.join("no_material.gltf");
        create_minimal_gltf(&gltf_path);

        let mut registry = AssetRegistry::default();
        let test_guid = guid(1000);

        // Import the glTF
        let outcome =
            GltfImporter::import_to_registry(&gltf_path, &mut registry, test_guid).unwrap();

        assert!(
            outcome.diagnostics.is_empty(),
            "glTF without materials should import without diagnostics"
        );

        // Verify the model has no materials (glTF without materials section)
        let handle = registry.handle_for_guid(test_guid).unwrap();
        let cpu_resource = registry.cpu_resource(handle).unwrap();
        let model = ModelResource::from_bytes(&cpu_resource.bytes).unwrap();

        // glTF files without a materials array will have 0 materials extracted
        assert_eq!(
            model.materials.len(),
            0,
            "glTF without materials should have 0 materials"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Creates a glTF file with a PBR material.
    fn create_gltf_with_pbr_material(path: &Path) {
        // Triangle vertices: positions and normals
        let positions: Vec<f32> = vec![
            0.0, 0.0, 0.0, // vertex 0
            1.0, 0.0, 0.0, // vertex 1
            0.0, 1.0, 0.0, // vertex 2
        ];
        let normals: Vec<f32> = vec![
            0.0, 0.0, 1.0, // normal 0
            0.0, 0.0, 1.0, // normal 1
            0.0, 0.0, 1.0, // normal 2
        ];
        let indices: Vec<u32> = vec![0, 1, 2];

        // Convert to bytes
        let positions_bytes: Vec<u8> = positions.iter().flat_map(|f| f.to_le_bytes()).collect();
        let normals_bytes: Vec<u8> = normals.iter().flat_map(|f| f.to_le_bytes()).collect();
        let indices_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();

        // Create binary buffer
        let mut buffer_data = Vec::new();
        let positions_offset = 0;
        let normals_offset = positions_bytes.len();
        let indices_offset = normals_offset + normals_bytes.len();
        buffer_data.extend_from_slice(&positions_bytes);
        buffer_data.extend_from_slice(&normals_bytes);
        buffer_data.extend_from_slice(&indices_bytes);

        // Write binary buffer
        let bin_path = path.with_extension("bin");
        std::fs::write(&bin_path, &buffer_data).unwrap();

        // Create glTF JSON with PBR material
        let gltf_json = serde_json::json!({
            "asset": {
                "version": "2.0"
            },
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "materials": [{
                "name": "TestMaterial",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.8, 0.2, 0.2, 1.0],
                    "metallicFactor": 0.9,
                    "roughnessFactor": 0.3
                },
                "emissiveFactor": [0.1, 0.1, 0.1],
                "alphaMode": "OPAQUE"
            }],
            "meshes": [{
                "primitives": [{
                    "attributes": {
                        "POSITION": 0,
                        "NORMAL": 1
                    },
                    "indices": 2,
                    "material": 0
                }]
            }],
            "accessors": [
                {
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3",
                    "min": [0.0, 0.0, 0.0],
                    "max": [1.0, 1.0, 0.0]
                },
                {
                    "bufferView": 1,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3"
                },
                {
                    "bufferView": 2,
                    "componentType": 5125,
                    "count": 3,
                    "type": "SCALAR"
                }
            ],
            "bufferViews": [
                {
                    "buffer": 0,
                    "byteOffset": positions_offset,
                    "byteLength": positions_bytes.len()
                },
                {
                    "buffer": 0,
                    "byteOffset": normals_offset,
                    "byteLength": normals_bytes.len()
                },
                {
                    "buffer": 0,
                    "byteOffset": indices_offset,
                    "byteLength": indices_bytes.len()
                }
            ],
            "buffers": [{
                "uri": bin_path.file_name().unwrap().to_str().unwrap(),
                "byteLength": buffer_data.len()
            }]
        });

        std::fs::write(path, serde_json::to_string_pretty(&gltf_json).unwrap()).unwrap();
    }

    #[test]
    fn import_worker_processes_png_and_produces_upload_task() {
        use std::time::Duration;

        // Create a temporary directory with a test PNG
        let temp_dir = std::env::temp_dir().join("aster_import_worker_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let png_path = temp_dir.join("test.png");

        // Create a simple 2x2 red PNG
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.save(&png_path).unwrap();

        // Spawn worker and enqueue import job
        let queue = ImportQueue::default();
        let worker = queue.spawn_worker();

        let job = ImportJob {
            asset_path: png_path.clone(),
            resource_kind: ResourceKind::Texture,
            import_options: ImportOptions::default(),
        };

        worker.enqueue(job).unwrap();

        // Poll for outcome (with timeout)
        let mut outcome = None;
        for _i in 0..100 {
            if let Some(result) = worker.try_recv_outcome() {
                outcome = Some(result);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Drop worker to ensure thread cleanup
        drop(worker);

        // Verify outcome was produced
        let outcome = outcome.expect("Worker should produce an outcome within 1 second");
        assert_eq!(
            outcome.diagnostics.len(),
            0,
            "Import should succeed without diagnostics"
        );

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn import_worker_handles_invalid_file() {
        use std::time::Duration;

        // Create a temporary directory with an invalid PNG
        let temp_dir = std::env::temp_dir().join("aster_import_worker_invalid_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let png_path = temp_dir.join("invalid.png");
        std::fs::write(&png_path, b"not a valid PNG file").unwrap();

        // Spawn worker and enqueue import job
        let queue = ImportQueue::default();
        let worker = queue.spawn_worker();

        let job = ImportJob {
            asset_path: png_path.clone(),
            resource_kind: ResourceKind::Texture,
            import_options: ImportOptions::default(),
        };

        worker.enqueue(job).unwrap();

        // Poll for outcome (with timeout)
        let mut outcome = None;
        for _ in 0..50 {
            if let Some(result) = worker.try_recv_outcome() {
                outcome = Some(result);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Verify outcome was produced with diagnostics
        let outcome = outcome.expect("Worker should produce an outcome even for invalid files");
        assert!(
            !outcome.diagnostics.is_empty(),
            "Invalid file should produce diagnostics"
        );

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn import_queue_drain_gpu_uploads_returns_tasks() {
        let mut queue = ImportQueue::default();

        let handle = ResourceHandle::new(
            ResourceId::from_u128(1),
            Handle::new(0, engine_core::Generation::FIRST),
        );

        // Push some upload tasks
        queue.push_upload(GpuUploadTask {
            handle,
            kind: ResourceKind::Texture,
        });
        queue.push_upload(GpuUploadTask {
            handle,
            kind: ResourceKind::Model,
        });

        // Drain uploads
        let uploads = queue.drain_gpu_uploads();
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0].kind, ResourceKind::Texture);
        assert_eq!(uploads[1].kind, ResourceKind::Model);

        // Verify queue is empty after drain
        let uploads2 = queue.drain_gpu_uploads();
        assert_eq!(uploads2.len(), 0);
    }

    #[test]
    fn file_watcher_detects_file_creation() {
        use std::time::Duration;

        // Create a temporary directory
        let temp_dir =
            std::env::temp_dir().join(format!("aster_watcher_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Start the watcher
        let mut watcher = FileWatcher::start(&temp_dir).unwrap();

        // Give the watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        // Create a new file
        let test_file = temp_dir.join("test.png");
        std::fs::write(&test_file, b"fake png data").unwrap();

        // Poll for events with timeout
        let mut events = Vec::new();
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(10));
            let mut polled = watcher.poll_events();
            events.append(&mut polled);
            if !events.is_empty() {
                break;
            }
        }

        // Wait for debounce window to pass
        std::thread::sleep(Duration::from_millis(250));
        let mut final_events = watcher.poll_events();
        events.append(&mut final_events);

        // Verify event was detected
        assert!(!events.is_empty(), "Should detect file creation event");
        let created_event = events.iter().find(|e| e.path == PathBuf::from("test.png"));
        assert!(created_event.is_some(), "Should have event for test.png");

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn file_watcher_debounces_modified_events() {
        use std::time::Duration;

        let temp_dir = std::env::temp_dir().join(format!(
            "aster_watcher_debounce_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("test.txt");
        std::fs::write(&test_file, b"initial").unwrap();

        let mut watcher = FileWatcher::start(&temp_dir).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        // Modify the file multiple times rapidly
        for i in 0..5 {
            std::fs::write(&test_file, format!("content {}", i)).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }

        // Poll immediately (events should be buffered)
        let _immediate_events = watcher.poll_events();

        // Wait for debounce window
        std::thread::sleep(Duration::from_millis(250));

        // Poll again (should get debounced events)
        let debounced_events = watcher.poll_events();

        // Should only get one event per file due to debouncing
        let test_events: Vec<_> = debounced_events
            .iter()
            .filter(|e| e.path == PathBuf::from("test.txt"))
            .collect();

        // We should have at most a few events, not 5
        assert!(
            test_events.len() <= 2,
            "Events should be debounced, got {} events",
            test_events.len()
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn asset_database_handle_event_marks_modified_as_stale() {
        let root = std::env::temp_dir().join(format!("aster_db_event_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");

        // Add an asset manually
        let path = PathBuf::from("test.png");
        let guid = generate_asset_guid(&path);
        let meta = ResourceMeta {
            guid,
            path: path.clone(),
            kind: ResourceKind::Texture,
            import_state: ResourceState::GpuReady,
        };
        database.entries.insert(path.clone(), meta);

        // Handle a Modified event
        let event = FileEvent {
            path: path.clone(),
            kind: FileEventKind::Modified,
        };
        database.handle_event(&event).unwrap();

        // Verify asset is marked as Stale
        let updated = database.entry_for_path(&path).unwrap();
        assert_eq!(updated.import_state, ResourceState::Stale);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asset_database_handle_event_adds_created_asset() {
        let root =
            std::env::temp_dir().join(format!("aster_db_create_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");

        // Handle a Created event
        let event = FileEvent {
            path: PathBuf::from("new_texture.png"),
            kind: FileEventKind::Created,
        };
        database.handle_event(&event).unwrap();

        // Verify asset was added
        let entry = database.entry_for_path(&PathBuf::from("new_texture.png"));
        assert!(entry.is_some(), "Created asset should be in database");
        assert_eq!(entry.unwrap().kind, ResourceKind::Texture);
        assert_eq!(entry.unwrap().import_state, ResourceState::Unloaded);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asset_database_handle_event_removes_deleted_asset() {
        let root =
            std::env::temp_dir().join(format!("aster_db_remove_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");

        // Add an asset manually
        let path = PathBuf::from("to_delete.png");
        let guid = generate_asset_guid(&path);
        let meta = ResourceMeta {
            guid,
            path: path.clone(),
            kind: ResourceKind::Texture,
            import_state: ResourceState::GpuReady,
        };
        database.entries.insert(path.clone(), meta);

        // Handle a Removed event
        let event = FileEvent {
            path: path.clone(),
            kind: FileEventKind::Removed,
        };
        database.handle_event(&event).unwrap();

        // Verify asset was removed
        let entry = database.entry_for_path(&path);
        assert!(entry.is_none(), "Removed asset should not be in database");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hot_reload_coordinator_processes_file_events_and_enqueues_imports() {
        let root =
            std::env::temp_dir().join(format!("aster_hot_reload_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create a test PNG file
        let png_path = root.join("test.png");
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.save(&png_path).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");
        database.scan(&root).unwrap();

        let mut coordinator = HotReloadCoordinator::new(&root);

        // Simulate a file modification event
        let events = vec![FileEvent {
            path: PathBuf::from("test.png"),
            kind: FileEventKind::Modified,
        }];

        let affected = coordinator
            .process_file_events(&events, &mut database)
            .unwrap();
        assert_eq!(affected.len(), 1, "Should have one affected asset");

        // Verify the asset was marked as stale
        let entry = database.entry_for_path(&PathBuf::from("test.png")).unwrap();
        assert_eq!(entry.import_state, ResourceState::Stale);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asset_registry_swap_gpu_enqueues_old_resource_for_destruction() {
        let mut registry = AssetRegistry::default();
        let guid = AssetGuid::from_u128(12345);
        let handle = registry.register(guid, ResourceKind::Texture).unwrap();

        // Put initial GPU resource
        let old_gpu = GpuResource {
            kind: ResourceKind::Texture,
            backend_token: 100,
        };
        registry.put_gpu(handle, old_gpu).unwrap();

        // Swap with new GPU resource
        let new_gpu = GpuResource {
            kind: ResourceKind::Texture,
            backend_token: 200,
        };
        registry.swap_gpu(handle, new_gpu, 3).unwrap();

        // Verify new resource is active
        let current = registry.gpu_resource(handle).unwrap();
        assert_eq!(current.backend_token, 200);

        // Verify old resource is in destroy queue
        assert_eq!(registry.gpu_destroy_queue.len(), 1);
        assert_eq!(registry.gpu_destroy_queue[0].1, 100); // old token
        assert_eq!(registry.gpu_destroy_queue[0].2, 3); // frames remaining
    }

    #[test]
    fn asset_registry_tick_gpu_destroy_queue_releases_resources() {
        let mut registry = AssetRegistry::default();
        let guid = AssetGuid::from_u128(12345);
        let handle = registry.register(guid, ResourceKind::Texture).unwrap();

        // Manually add items to destroy queue with different frame delays
        registry.gpu_destroy_queue.push_back((handle, 100, 2));
        registry.gpu_destroy_queue.push_back((handle, 101, 0));
        registry.gpu_destroy_queue.push_back((handle, 102, 1));

        // Tick 1 - should release token 101 (frames=0) without decrementing others
        let ready = registry.tick_gpu_destroy_queue();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], 101);
        assert_eq!(registry.gpu_destroy_queue.len(), 2);

        // Tick 2 - no items at 0, so decrement all
        let ready = registry.tick_gpu_destroy_queue();
        assert_eq!(ready.len(), 1); // 102 reaches 0 and is removed
        assert_eq!(ready[0], 102);
        assert_eq!(registry.gpu_destroy_queue.len(), 1);

        // Tick 3 - no items at 0, so decrement (100: 1->0) and remove
        let ready = registry.tick_gpu_destroy_queue();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], 100);
        assert_eq!(registry.gpu_destroy_queue.len(), 0);

        // Tick 4 - queue is empty
        let ready = registry.tick_gpu_destroy_queue();
        assert_eq!(ready.len(), 0);
        assert_eq!(registry.gpu_destroy_queue.len(), 0);
    }

    #[test]
    fn asset_registry_mark_failed_sets_error_state() {
        let mut registry = AssetRegistry::default();
        let guid = AssetGuid::from_u128(12345);
        let handle = registry.register(guid, ResourceKind::Texture).unwrap();

        // Put some resources
        registry
            .put_cpu(
                handle,
                CpuResource {
                    kind: ResourceKind::Texture,
                    bytes: Arc::from(vec![1, 2, 3]),
                },
            )
            .unwrap();
        registry
            .put_gpu(
                handle,
                GpuResource {
                    kind: ResourceKind::Texture,
                    backend_token: 100,
                },
            )
            .unwrap();

        // Mark as failed
        registry
            .mark_failed(handle, "Import failed: invalid format")
            .unwrap();

        // Verify state
        let record = registry.record(handle).unwrap();
        assert_eq!(record.state, ResourceState::Failed);
        assert!(record.preview.is_some());
        assert!(
            record
                .preview
                .as_ref()
                .unwrap()
                .summary
                .contains("Import failed")
        );

        // Verify caches were cleared
        assert!(registry.cpu_resource(handle).is_none());
        assert!(registry.gpu_resource(handle).is_none());
    }

    #[test]
    fn hot_reload_full_flow_integration() {
        use std::time::Duration;

        let root =
            std::env::temp_dir().join(format!("aster_hot_reload_full_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Create initial PNG file
        let png_path = root.join("test.png");
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        img.save(&png_path).unwrap();

        // Set up database and registry
        let mut database = AssetDatabase::new(&root, "builtin");
        database.scan(&root).unwrap();

        let mut registry = AssetRegistry::default();
        let entry = database.entry_for_path(&PathBuf::from("test.png")).unwrap();
        let handle = registry.register(entry.guid, entry.kind).unwrap();

        // Initial import
        let options = ImportOptions::default();
        let outcome =
            PngImporter::import_to_registry(&png_path, &options, &mut registry, entry.guid)
                .unwrap();
        assert!(outcome.diagnostics.is_empty());
        assert_eq!(
            registry.record(handle).unwrap().state,
            ResourceState::CpuReady
        );

        // Simulate GPU upload
        registry
            .put_gpu(
                handle,
                GpuResource {
                    kind: ResourceKind::Texture,
                    backend_token: 1000,
                },
            )
            .unwrap();

        // Modify the file
        let img2 = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255]));
        img2.save(&png_path).unwrap();

        // Process file event
        let mut coordinator = HotReloadCoordinator::new(&root);
        let events = vec![FileEvent {
            path: PathBuf::from("test.png"),
            kind: FileEventKind::Modified,
        }];
        coordinator
            .process_file_events(&events, &mut database)
            .unwrap();

        // Verify asset marked as stale
        let entry = database.entry_for_path(&PathBuf::from("test.png")).unwrap();
        assert_eq!(entry.import_state, ResourceState::Stale);

        // Enqueue reimport via worker
        let job = ImportJob {
            asset_path: png_path.clone(),
            resource_kind: ResourceKind::Texture,
            import_options: options,
        };
        coordinator.enqueue_import(job).unwrap();

        // Poll for completed import
        let mut outcomes = Vec::new();
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(10));
            let mut polled = coordinator.poll_completed_imports(&mut registry);
            outcomes.append(&mut polled);
            if !outcomes.is_empty() {
                break;
            }
        }

        // Note: The worker processes imports but doesn't update the registry directly
        // In a real system, the outcomes would be processed to update the registry

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn asset_database_handle_event_returns_guid_for_modified() {
        let root = std::env::temp_dir().join(format!("aster_db_guid_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut database = AssetDatabase::new(&root, "builtin");

        // Add an asset manually
        let path = PathBuf::from("test.png");
        let guid = generate_asset_guid(&path);
        let meta = ResourceMeta {
            guid,
            path: path.clone(),
            kind: ResourceKind::Texture,
            import_state: ResourceState::GpuReady,
        };
        database.entries.insert(path.clone(), meta);

        // Handle a Modified event
        let event = FileEvent {
            path: path.clone(),
            kind: FileEventKind::Modified,
        };
        let result_guid = database.handle_event(&event).unwrap();

        // Verify GUID was returned
        assert!(result_guid.is_some());
        assert_eq!(result_guid.unwrap(), guid);

        // Verify asset was marked as stale
        let entry = database.entry_for_path(&path).unwrap();
        assert_eq!(entry.import_state, ResourceState::Stale);

        let _ = std::fs::remove_dir_all(&root);
    }
}
