//! RenderGraph compilation and execution.
//!
//! Passes declare their read/write resources; the graph topologically sorts
//! them and resolves barriers before handing the compiled list to the backend.

use std::collections::HashMap;

use engine_core::{EngineError, EngineResult};

use crate::resource::{BufferHandle, ImageHandle};

/// Logical stage of a frame relative to upscaling and UI composition.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum RenderStage {
    /// Rendering at internal resolution before upscaling.
    #[default]
    PreUpscale,
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

/// A single render pass node in the graph.
#[derive(Clone, Debug)]
pub struct RenderPass {
    /// Stable pass id.
    pub id: PassId,
    /// Human-readable name.
    pub name: String,
    /// Logical frame stage.
    pub stage: RenderStage,
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
        self.add_pass_at_stage(name, RenderStage::PreUpscale)
    }

    /// Adds a pass at a logical scaling stage and returns its id.
    pub fn add_pass_at_stage(&mut self, name: impl Into<String>, stage: RenderStage) -> PassId {
        let id = PassId(self.next_id);
        self.next_id += 1;
        self.passes.push(RenderPass {
            id,
            name: name.into(),
            stage,
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
    let neighbors = adj.entry(before).or_default();
    if !neighbors.contains(&after) {
        neighbors.push(after);
        *in_degree.entry(after).or_insert(0) += 1;
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

    // Track the most recent writer so accesses form ordered dependencies
    // instead of all readers depending on an arbitrary final writer.
    let mut image_writers: HashMap<ImageHandle, PassId> = HashMap::new();
    let mut buffer_writers: HashMap<BufferHandle, PassId> = HashMap::new();
    for pass in passes {
        for &(img, access) in &pass.image_accesses {
            if let Some(&writer) = image_writers.get(&img) {
                add_edge(writer, pass.id, &mut adj, &mut in_degree);
            }
            if matches!(access, ResourceAccess::Write | ResourceAccess::ReadWrite) {
                image_writers.insert(img, pass.id);
            }
        }
        for &(buf, access) in &pass.buffer_accesses {
            if let Some(&writer) = buffer_writers.get(&buf) {
                add_edge(writer, pass.id, &mut adj, &mut in_degree);
            }
            if matches!(access, ResourceAccess::Write | ResourceAccess::ReadWrite) {
                buffer_writers.insert(buf, pass.id);
            }
        }
    }

    let mut queue: Vec<PassId> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
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
}
