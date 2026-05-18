#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Asset database, registry, manifest, dependency, import, and reload primitives.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::SystemTime,
};

use engine_core::{AssetId, EngineError, EngineResult, Handle, HandleAllocator, ResourceId};
use serde::{Deserialize, Serialize};

/// Current schema version for asset-side files.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Stable resource GUID serialized as 128 bits.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
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
}
