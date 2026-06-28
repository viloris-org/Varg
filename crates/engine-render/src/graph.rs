//! RenderGraph compilation and execution.
//!
//! Passes declare their read/write resources; the graph topologically sorts
//! them and resolves barriers before handing the compiled list to the backend.

use std::collections::HashMap;

use engine_core::{EngineError, EngineResult};

use crate::resource::{BufferHandle, ImageHandle};

/// Backend-visible role of a Frame Pipeline pass.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum RenderPassKind {
    /// Clustered or tiled light list construction for the active view.
    LightCulling,
    /// Shadow-map or shadow-atlas rendering.
    Shadow,
    /// Cascaded directional shadow-map rendering.
    DirectionalShadow,
    /// Local light shadow-atlas rendering.
    LocalShadow,
    /// Geometry buffer population.
    GBuffer,
    /// Deferred lighting over geometry buffers.
    DeferredLighting,
    /// Forward scene rendering.
    Forward,
    /// Motion vectors, depth history, exposure, and reactive mask preparation.
    TemporalInputs,
    /// Resolution reconstruction or spatial scaling.
    Upscale,
    /// Screen-space ambient occlusion.
    AmbientOcclusion,
    /// Screen-space indirect diffuse lighting.
    ScreenSpaceGI,
    /// Screen-space specular reflection.
    Reflection,
    /// Full-resolution post-processing.
    PostProcess,
    /// UI/HUD composition at UI resolution.
    UiComposition,
    /// Editor or debug object outline rendering.
    Outline,
    /// Backend-specific pass not understood by the generic render crate.
    #[default]
    Custom,
}

impl RenderPassKind {
    /// Infers a known pass kind from a legacy pass name.
    pub fn from_name(name: &str) -> Self {
        match name {
            "light-culling" | "light_culling" | "clustered-lighting" | "clustered_lighting" => {
                Self::LightCulling
            }
            "shadow" => Self::Shadow,
            "directional-shadow" | "directional_shadow" | "csm-shadow" | "csm_shadow" => {
                Self::DirectionalShadow
            }
            "local-shadow" | "local_shadow" | "local-shadow-atlas" | "local_shadow_atlas" => {
                Self::LocalShadow
            }
            "gbuffer" => Self::GBuffer,
            "deferred-lighting" | "deferred_lighting" => Self::DeferredLighting,
            "forward" => Self::Forward,
            "temporal-inputs" | "temporal_inputs" => Self::TemporalInputs,
            "upscale" => Self::Upscale,
            "ambient-occlusion" | "ambient_occlusion" | "ssao" => Self::AmbientOcclusion,
            "screen-space-gi" | "screen_space_gi" | "ssgi" => Self::ScreenSpaceGI,
            "reflection"
            | "reflections"
            | "screen-space-reflection"
            | "screen_space_reflection"
            | "ssr" => Self::Reflection,
            "post" | "post-process" | "post_process" => Self::PostProcess,
            "ui" | "gui" => Self::UiComposition,
            "outline" => Self::Outline,
            _ => Self::Custom,
        }
    }

    /// Returns the default scaling stage for this pass kind.
    pub fn default_stage(self) -> RenderStage {
        match self {
            Self::TemporalInputs => RenderStage::TemporalInputs,
            Self::Upscale => RenderStage::Upscale,
            Self::PostProcess | Self::Outline => RenderStage::PostUpscale,
            Self::UiComposition => RenderStage::UiComposition,
            Self::AmbientOcclusion | Self::ScreenSpaceGI | Self::Reflection => {
                RenderStage::PreUpscale
            }
            Self::LightCulling
            | Self::DirectionalShadow
            | Self::LocalShadow
            | Self::Shadow
            | Self::GBuffer
            | Self::DeferredLighting
            | Self::Forward
            | Self::Custom => RenderStage::PreUpscale,
        }
    }
}

/// Logical stage of a frame relative to upscaling and UI composition.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum RenderStage {
    /// Rendering at internal resolution before upscaling.
    #[default]
    PreUpscale,
    /// Motion vectors, depth history, exposure, and reactive mask preparation.
    TemporalInputs,
    /// Resolution reconstruction or spatial scaling.
    Upscale,
    /// Full-resolution post-processing.
    PostUpscale,
    /// UI/HUD composition at UI resolution.
    UiComposition,
}

/// Stable pass identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PassId(u32);

impl PassId {
    /// Returns the raw id.
    pub fn raw(self) -> u32 {
        self.0
    }
}

/// Resource access declared by a pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceAccess {
    /// Read-only.
    Read,
    /// Write (exclusive).
    Write,
    /// Read-write.
    ReadWrite,
}

/// Scheduling and execution flags for a Frame Pipeline pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderPassFlags(u32);

impl RenderPassFlags {
    /// No flags.
    pub const NONE: Self = Self(0);
    /// Raster pass.
    pub const RASTER: Self = Self(1 << 0);
    /// Compute pass.
    pub const COMPUTE: Self = Self(1 << 1);
    /// Copy, clear, or transfer pass.
    pub const COPY: Self = Self(1 << 2);
    /// Pass has externally visible side effects and must not be culled by future graph optimizers.
    pub const NEVER_CULL: Self = Self(1 << 3);
    /// Pass presents or composes into a platform-owned output surface.
    pub const PRESENTATION: Self = Self(1 << 4);

    /// Combines two flag sets.
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns whether every flag in `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Descriptor used to add a pass to a [`RenderGraphBuilder`].
#[derive(Clone, Debug)]
pub struct RenderPassDesc {
    /// Human-readable name.
    pub name: String,
    /// Backend-visible pass role.
    pub kind: RenderPassKind,
    /// Logical frame stage.
    pub stage: RenderStage,
    /// Scheduling and execution flags.
    pub flags: RenderPassFlags,
}

impl RenderPassDesc {
    /// Creates a pass descriptor with the default stage for `kind`.
    pub fn new(name: impl Into<String>, kind: RenderPassKind) -> Self {
        let kind = match kind {
            RenderPassKind::Custom => {
                let name = name.into();
                let kind = RenderPassKind::from_name(&name);
                return Self {
                    name,
                    kind,
                    stage: kind.default_stage(),
                    flags: RenderPassFlags::NONE,
                };
            }
            known => known,
        };
        Self {
            name: name.into(),
            kind,
            stage: kind.default_stage(),
            flags: RenderPassFlags::NONE,
        }
    }

    /// Overrides the logical frame stage.
    pub fn with_stage(mut self, stage: RenderStage) -> Self {
        self.stage = stage;
        self
    }

    /// Overrides the scheduling and execution flags.
    pub fn with_flags(mut self, flags: RenderPassFlags) -> Self {
        self.flags = flags;
        self
    }
}

/// A single render pass node in the graph.
#[derive(Clone, Debug)]
pub struct RenderPass {
    /// Stable pass id.
    pub id: PassId,
    /// Human-readable name.
    pub name: String,
    /// Backend-visible pass role.
    pub kind: RenderPassKind,
    /// Logical frame stage.
    pub stage: RenderStage,
    /// Scheduling and execution flags.
    pub flags: RenderPassFlags,
    /// Image resources accessed by this pass.
    pub image_accesses: Vec<(ImageHandle, ResourceAccess)>,
    /// Buffer resources accessed by this pass.
    pub buffer_accesses: Vec<(BufferHandle, ResourceAccess)>,
}

/// Compiled, immutable render graph ready for execution.
#[derive(Debug, Default)]
pub struct RenderGraph {
    /// Topologically sorted passes.
    pub passes: Vec<RenderPass>,
}

impl RenderGraph {
    /// Returns the number of passes.
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// Returns whether the graph contains a pass with the given name.
    pub fn contains_pass(&self, name: &str) -> bool {
        self.passes.iter().any(|pass| pass.name == name)
    }

    /// Returns whether the graph contains at least one pass in the given stage.
    pub fn contains_stage(&self, stage: RenderStage) -> bool {
        self.passes.iter().any(|pass| pass.stage == stage)
    }

    /// Returns whether the graph contains at least one pass of the given kind.
    pub fn contains_pass_kind(&self, kind: RenderPassKind) -> bool {
        self.passes.iter().any(|pass| pass.kind == kind)
    }
}

/// Builder for constructing a [`RenderGraph`].
#[derive(Debug, Default)]
pub struct RenderGraphBuilder {
    passes: Vec<RenderPass>,
    next_id: u32,
    /// Explicit ordering edges: (before, after).
    edges: Vec<(PassId, PassId)>,
}

impl RenderGraphBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a pass and returns its id.
    pub fn add_pass(&mut self, name: impl Into<String>) -> PassId {
        let name = name.into();
        let kind = RenderPassKind::from_name(&name);
        self.add_pass_desc(RenderPassDesc::new(name, kind))
    }

    /// Adds a pass at a logical scaling stage and returns its id.
    pub fn add_pass_at_stage(&mut self, name: impl Into<String>, stage: RenderStage) -> PassId {
        let name = name.into();
        let kind = RenderPassKind::from_name(&name);
        self.add_pass_desc(RenderPassDesc::new(name, kind).with_stage(stage))
    }

    /// Adds a typed pass with the default stage for `kind` and returns its id.
    pub fn add_typed_pass(&mut self, name: impl Into<String>, kind: RenderPassKind) -> PassId {
        self.add_pass_desc(RenderPassDesc::new(name, kind))
    }

    /// Adds a pass from a descriptor and returns its id.
    pub fn add_pass_desc(&mut self, desc: RenderPassDesc) -> PassId {
        let id = PassId(self.next_id);
        self.next_id += 1;
        self.passes.push(RenderPass {
            id,
            name: desc.name,
            kind: desc.kind,
            stage: desc.stage,
            flags: desc.flags,
            image_accesses: Vec::new(),
            buffer_accesses: Vec::new(),
        });
        id
    }

    /// Declares an image access for a pass.
    pub fn use_image(&mut self, pass: PassId, image: ImageHandle, access: ResourceAccess) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.id == pass) {
            p.image_accesses.push((image, access));
        }
    }

    /// Declares a buffer access for a pass.
    pub fn use_buffer(&mut self, pass: PassId, buffer: BufferHandle, access: ResourceAccess) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.id == pass) {
            p.buffer_accesses.push((buffer, access));
        }
    }

    /// Adds an explicit ordering edge: `before` runs before `after`.
    pub fn order_before(&mut self, before: PassId, after: PassId) {
        self.edges.push((before, after));
    }

    /// Compiles the graph via topological sort.
    ///
    /// Panics when the graph contains a cycle. Use [`Self::try_build`] when
    /// constructing graphs from dynamic input.
    pub fn build(self) -> RenderGraph {
        self.try_build()
            .expect("render graph must not contain dependency cycles")
    }

    /// Compiles the graph and reports dependency cycles.
    pub fn try_build(self) -> EngineResult<RenderGraph> {
        let sorted = topological_sort(&self.passes, &self.edges)?;
        Ok(RenderGraph { passes: sorted })
    }
}

fn add_edge(
    before: PassId,
    after: PassId,
    adj: &mut HashMap<PassId, Vec<PassId>>,
    in_degree: &mut HashMap<PassId, usize>,
) {
    if before == after {
        return;
    }
    let neighbors = adj.entry(before).or_default();
    if !neighbors.contains(&after) {
        neighbors.push(after);
        *in_degree.entry(after).or_insert(0) += 1;
    }
}

#[derive(Default)]
struct ResourceState {
    writer: Option<PassId>,
    readers: Vec<PassId>,
}

fn add_resource_dependencies(
    state: &mut ResourceState,
    pass: PassId,
    access: ResourceAccess,
    adj: &mut HashMap<PassId, Vec<PassId>>,
    in_degree: &mut HashMap<PassId, usize>,
) {
    match access {
        ResourceAccess::Read => {
            if let Some(writer) = state.writer {
                add_edge(writer, pass, adj, in_degree);
            }
            if !state.readers.contains(&pass) {
                state.readers.push(pass);
            }
        }
        ResourceAccess::Write | ResourceAccess::ReadWrite => {
            if let Some(writer) = state.writer {
                add_edge(writer, pass, adj, in_degree);
            }
            for &reader in &state.readers {
                add_edge(reader, pass, adj, in_degree);
            }
            state.writer = Some(pass);
            state.readers.clear();
        }
    }
}

/// Kahn's algorithm topological sort with cycle detection.
fn topological_sort(
    passes: &[RenderPass],
    edges: &[(PassId, PassId)],
) -> EngineResult<Vec<RenderPass>> {
    let mut in_degree: HashMap<PassId, usize> = passes.iter().map(|p| (p.id, 0)).collect();
    let mut adj: HashMap<PassId, Vec<PassId>> = passes.iter().map(|p| (p.id, vec![])).collect();

    for &(before, after) in edges {
        if !in_degree.contains_key(&before) || !in_degree.contains_key(&after) {
            return Err(EngineError::other(
                "render graph edge references unknown pass",
            ));
        }
        add_edge(before, after, &mut adj, &mut in_degree);
    }

    // Track the live readers and most recent writer so read/write hazards
    // become ordered dependencies in declaration order.
    let mut image_states: HashMap<ImageHandle, ResourceState> = HashMap::new();
    let mut buffer_states: HashMap<BufferHandle, ResourceState> = HashMap::new();
    for pass in passes {
        for &(img, access) in &pass.image_accesses {
            add_resource_dependencies(
                image_states.entry(img).or_default(),
                pass.id,
                access,
                &mut adj,
                &mut in_degree,
            );
        }
        for &(buf, access) in &pass.buffer_accesses {
            add_resource_dependencies(
                buffer_states.entry(buf).or_default(),
                pass.id,
                access,
                &mut adj,
                &mut in_degree,
            );
        }
    }

    let mut queue: Vec<PassId> = in_degree
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    queue.sort_by_key(|id| id.0);

    let pass_map: HashMap<PassId, &RenderPass> = passes.iter().map(|p| (p.id, p)).collect();
    let mut result = Vec::with_capacity(passes.len());

    while let Some(id) = queue.first().copied() {
        queue.remove(0);
        if let Some(&pass) = pass_map.get(&id) {
            result.push(pass.clone());
        }
        if let Some(neighbors) = adj.get(&id) {
            for &next in neighbors {
                let deg = in_degree
                    .get_mut(&next)
                    .expect("validated render graph pass id");
                *deg -= 1;
                if *deg == 0 {
                    queue.push(next);
                    queue.sort_by_key(|id| id.0);
                }
            }
        }
    }

    if result.len() != passes.len() {
        return Err(EngineError::other(
            "render graph contains a dependency cycle",
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::{Generation, Handle};

    fn image(slot: u32) -> ImageHandle {
        ImageHandle::new(Handle::new(slot, Generation::FIRST))
    }

    #[test]
    fn empty_graph_builds_and_has_no_passes() {
        let graph = RenderGraphBuilder::new().build();
        assert_eq!(graph.pass_count(), 0);
    }

    #[test]
    fn single_pass_graph() {
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass("forward");
        let graph = builder.build();
        assert_eq!(graph.pass_count(), 1);
        assert_eq!(graph.passes[0].name, "forward");
        assert_eq!(graph.passes[0].kind, RenderPassKind::Forward);
    }

    #[test]
    fn typed_passes_keep_kind_stage_and_flags() {
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass_desc(
            RenderPassDesc::new("main-scene", RenderPassKind::Forward)
                .with_flags(RenderPassFlags::RASTER.or(RenderPassFlags::NEVER_CULL)),
        );

        let graph = builder.build();
        let pass = &graph.passes[0];
        assert_eq!(pass.name, "main-scene");
        assert_eq!(pass.kind, RenderPassKind::Forward);
        assert_eq!(pass.stage, RenderStage::PreUpscale);
        assert!(pass.flags.contains(RenderPassFlags::RASTER));
        assert!(pass.flags.contains(RenderPassFlags::NEVER_CULL));
        assert!(graph.contains_pass_kind(RenderPassKind::Forward));
    }

    #[test]
    fn lighting_pass_names_infer_typed_kinds() {
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass("light-culling");
        builder.add_pass("directional-shadow");
        builder.add_pass("local-shadow");
        builder.add_pass("ssao");
        builder.add_pass("ssgi");
        builder.add_pass("ssr");

        let graph = builder.build();
        assert!(graph.contains_pass_kind(RenderPassKind::LightCulling));
        assert!(graph.contains_pass_kind(RenderPassKind::DirectionalShadow));
        assert!(graph.contains_pass_kind(RenderPassKind::LocalShadow));
        assert!(graph.contains_pass_kind(RenderPassKind::AmbientOcclusion));
        assert!(graph.contains_pass_kind(RenderPassKind::ScreenSpaceGI));
        assert!(graph.contains_pass_kind(RenderPassKind::Reflection));
    }

    #[test]
    fn explicit_ordering_is_respected() {
        let mut builder = RenderGraphBuilder::new();
        let shadow = builder.add_pass("shadow");
        let forward = builder.add_pass("forward");
        let post = builder.add_pass("post");
        builder.order_before(shadow, forward);
        builder.order_before(forward, post);
        let graph = builder.build();
        assert_eq!(graph.passes[0].name, "shadow");
        assert_eq!(graph.passes[1].name, "forward");
        assert_eq!(graph.passes[2].name, "post");
    }

    #[test]
    fn dependency_cycle_is_rejected() {
        let mut builder = RenderGraphBuilder::new();
        let a = builder.add_pass("a");
        let b = builder.add_pass("b");
        builder.order_before(a, b);
        builder.order_before(b, a);

        assert!(builder.try_build().is_err());
    }

    #[test]
    fn passes_retain_scaling_stage_metadata() {
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass_at_stage("upscale", RenderStage::Upscale);
        let graph = builder.build();
        assert_eq!(graph.passes[0].stage, RenderStage::Upscale);
    }

    #[test]
    fn temporal_inputs_stage_orders_before_upscale() {
        let mut builder = RenderGraphBuilder::new();
        let forward = builder.add_pass("forward");
        let temporal = builder.add_pass_at_stage("temporal-inputs", RenderStage::TemporalInputs);
        let upscale = builder.add_pass_at_stage("upscale", RenderStage::Upscale);
        builder.order_before(forward, temporal);
        builder.order_before(temporal, upscale);

        let graph = builder.build();
        let names: Vec<&str> = graph.passes.iter().map(|pass| pass.name.as_str()).collect();
        assert_eq!(names, ["forward", "temporal-inputs", "upscale"]);
        assert_eq!(graph.passes[1].stage, RenderStage::TemporalInputs);
    }

    #[test]
    fn resource_writes_wait_for_prior_readers() {
        let mut builder = RenderGraphBuilder::new();
        let sample_history = builder.add_pass("sample-history");
        let overlay_history = builder.add_pass("overlay-history");
        let update_history = builder.add_pass("update-history");
        let history = image(7);
        builder.use_image(sample_history, history, ResourceAccess::Read);
        builder.use_image(overlay_history, history, ResourceAccess::Read);
        builder.use_image(update_history, history, ResourceAccess::Write);

        let graph = builder.build();
        let names: Vec<&str> = graph.passes.iter().map(|pass| pass.name.as_str()).collect();
        assert_eq!(
            names,
            ["sample-history", "overlay-history", "update-history"]
        );
    }

    #[test]
    fn conflicting_write_after_read_order_is_rejected() {
        let mut builder = RenderGraphBuilder::new();
        let sample_history = builder.add_pass("sample-history");
        let update_history = builder.add_pass("update-history");
        let history = image(7);
        builder.use_image(sample_history, history, ResourceAccess::Read);
        builder.use_image(update_history, history, ResourceAccess::Write);
        builder.order_before(update_history, sample_history);

        assert!(builder.try_build().is_err());
    }

    #[test]
    fn graph_reports_named_passes_and_stages() {
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass("forward");
        builder.add_pass_at_stage("ui", RenderStage::UiComposition);
        let graph = builder.build();
        assert!(graph.contains_pass("forward"));
        assert!(!graph.contains_pass("shadow"));
        assert!(graph.contains_stage(RenderStage::UiComposition));
    }
}
