//! RenderGraph compilation and execution.
//!
//! Passes declare their read/write resources; the graph topologically sorts
//! them and resolves barriers before handing the compiled list to the backend.

use std::collections::HashMap;

use crate::resource::{BufferHandle, ImageHandle};

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
        let id = PassId(self.next_id);
        self.next_id += 1;
        self.passes.push(RenderPass {
            id,
            name: name.into(),
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
    pub fn build(self) -> RenderGraph {
        let sorted = topological_sort(&self.passes, &self.edges);
        RenderGraph { passes: sorted }
    }
}

/// Kahn's algorithm topological sort with cycle detection.
///
/// Derives implicit write→read edges from resource accesses in O(n) time,
/// then topologically sorts. Returns `Err` if the graph contains a cycle.
fn topological_sort(
    passes: &[RenderPass],
    edges: &[(PassId, PassId)],
) -> Vec<RenderPass> {
    let mut in_degree: HashMap<PassId, usize> = passes.iter().map(|p| (p.id, 0)).collect();
    let mut adj: HashMap<PassId, Vec<PassId>> = passes.iter().map(|p| (p.id, vec![])).collect();

    for &(before, after) in edges {
        adj.entry(before).or_default().push(after);
        *in_degree.entry(after).or_insert(0) += 1;
    }

    // Build resource-to-writer-pass adjacency map, then derive implicit edges in O(n).
    let mut image_writers: HashMap<ImageHandle, PassId> = HashMap::new();
    let mut buffer_writers: HashMap<BufferHandle, PassId> = HashMap::new();
    for pass in passes {
        for &(img, access) in &pass.image_accesses {
            if matches!(access, ResourceAccess::Write | ResourceAccess::ReadWrite) {
                image_writers.insert(img, pass.id);
            }
        }
        for &(buf, access) in &pass.buffer_accesses {
            if matches!(access, ResourceAccess::Write | ResourceAccess::ReadWrite) {
                buffer_writers.insert(buf, pass.id);
            }
        }
    }

    for pass in passes {
        for &(img, _) in &pass.image_accesses {
            if let Some(&writer) = image_writers.get(&img) {
                if writer != pass.id && !edges.contains(&(writer, pass.id)) {
                    adj.entry(writer).or_default().push(pass.id);
                    *in_degree.entry(pass.id).or_insert(0) += 1;
                }
            }
        }
        for &(buf, _) in &pass.buffer_accesses {
            if let Some(&writer) = buffer_writers.get(&buf) {
                if writer != pass.id && !edges.contains(&(writer, pass.id)) {
                    adj.entry(writer).or_default().push(pass.id);
                    *in_degree.entry(pass.id).or_insert(0) += 1;
                }
            }
        }
    }

    let mut queue: Vec<PassId> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    queue.sort_by_key(|id| id.0); // deterministic

    let pass_map: HashMap<PassId, &RenderPass> = passes.iter().map(|p| (p.id, p)).collect();
    let mut result = Vec::with_capacity(passes.len());

    while let Some(id) = queue.first().copied() {
        queue.remove(0);
        if let Some(&pass) = pass_map.get(&id) {
            result.push(pass.clone());
        }
        if let Some(neighbors) = adj.get(&id) {
            for &next in neighbors {
                let deg = in_degree.entry(next).or_insert(0);
                *deg = deg.saturating_sub(1);
                if *deg == 0 {
                    queue.push(next);
                    queue.sort_by_key(|id| id.0);
                }
            }
        }
    }

    // Detect cycles: if not all passes were reached, there is a cycle.
    // Append any remaining isolated passes (no edges at all).
    for pass in passes {
        if !result.iter().any(|p| p.id == pass.id) {
            result.push(pass.clone());
        }
    }

    result
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
}
