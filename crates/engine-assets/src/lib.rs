#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Asset database, registry, manifest, dependency, import, and reload primitives.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fmt, fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
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

impl DecodedTextureResource {
    /// Parses a decoded texture payload from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(|source| AssetError::Parse {
            format: "decoded texture",
            diagnostic: AssetDiagnostic::new(source.to_string()),
        })
    }

    fn to_bytes(&self) -> EngineResult<Arc<[u8]>> {
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

/// Imported model payload containing basic static meshes.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ModelResource {
    /// Mesh primitives available to runtime rendering.
    pub meshes: Vec<BasicMeshResource>,
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

    /// Resolves `builtin:/x` or `project:/x` resource references to native paths.
    pub fn resolve_resource_reference(&self, reference: &str) -> Result<PathBuf, AssetError> {
        if let Some(rest) = reference.strip_prefix("builtin:/") {
            Ok(self.builtin_root.join(rest))
        } else if let Some(rest) = reference.strip_prefix("project:/") {
            Ok(self.project_root.join(rest))
        } else {
            Err(AssetError::NotFound {
                diagnostic: AssetDiagnostic::new(
                    "resource reference must use builtin:/ or project:/",
                ),
            })
        }
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

/// Import and upload queues with separated CPU and GPU work.
#[derive(Clone, Debug, Default)]
pub struct ImportQueue {
    imports: VecDeque<ImportTask>,
    uploads: VecDeque<GpuUploadTask>,
}

impl ImportQueue {
    /// Queues a CPU import/load task.
    pub fn push_import(&mut self, task: ImportTask) {
        self.imports.push_back(task);
    }

    /// Queues a GPU upload task.
    pub fn push_upload(&mut self, task: GpuUploadTask) {
        self.uploads.push_back(task);
    }

    /// Pops one GPU upload task.
    pub fn pop_upload(&mut self) -> Option<GpuUploadTask> {
        self.uploads.pop_front()
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
                self.uploads.push_back(upload.clone());
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
        "gltf" | "glb" => Some((ResourceKind::Model, "gltf")),
        "json" if path.to_string_lossy().contains("material") => {
            Some((ResourceKind::Material, "material-json"))
        }
        "toml" if path.to_string_lossy().contains("material") => {
            Some((ResourceKind::Material, "material-toml"))
        }
        "wgsl" | "glsl" => Some((ResourceKind::Shader, "shader-source")),
        "wav" | "ogg" => Some((ResourceKind::Audio, "audio")),
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
            let Some((kind, importer)) = infer_importer(&relative) else {
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
        ResourceKind::Audio | ResourceKind::Animation => ImportedCpuPayload {
            bytes: Arc::from(bytes),
            summary: format!("{} bytes imported by {}", bytes.len(), importer),
            diagnostics: Vec::new(),
        },
    }
}

fn import_texture_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
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

fn import_model_payload(path: &Path, importer: &str, bytes: &[u8]) -> ImportedCpuPayload {
    let mut diagnostics = Vec::new();
    let (payload, summary) = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("gltf") || extension.eq_ignore_ascii_case("glb")
        }) {
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
            let normals = reader
                .read_normals()
                .map(|items| items.collect::<Vec<_>>())
                .unwrap_or_default();
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
}
