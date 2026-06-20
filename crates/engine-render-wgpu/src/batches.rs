use std::collections::HashMap;

use crate::{device::*, meshes::*, uniforms::*};
use engine_render::RenderWorld;
use wgpu::util::DeviceExt;

impl WgpuRenderDevice {
    /// Register PBR material parameters for an asset material name.
    ///
    /// Material names match the format used in `RenderObject::material`, e.g.
    /// `"asset:0123456789abcdef"`. Parameters registered here override the
    /// default built-in material lookups.
    pub fn register_material_params(
        &mut self,
        name: &str,
        base_color: [f32; 4],
        metallic: f32,
        roughness: f32,
        emissive: [f32; 3],
    ) {
        self.material_cache
            .insert(name.to_owned(), (base_color, metallic, roughness, emissive));
    }

    /// Prepares instance buffer from mesh batches for rendering.
    pub fn create_mesh_buffers(&self, mesh: &DebugMesh) -> MeshBuffers {
        let (vertices, indices) = generate_mesh(mesh);
        Self::buffers_from_data(&self.device, &vertices, &indices)
    }

    /// Uploads a mesh from vertex/index data into the mesh cache.
    pub fn upload_mesh(&mut self, name: &str, vertices: &[Vertex], indices: &[u32]) {
        let buffers = Self::buffers_from_data(&self.device, vertices, indices);
        self.mesh_cache.insert(name.to_string(), buffers);
    }

    /// Pre-loads procedural debug meshes into the cache.
    pub fn upload_debug_meshes(&mut self) {
        for mesh in &[DebugMesh::Cube, DebugMesh::Sphere(8), DebugMesh::Plane] {
            let name = mesh_name(mesh);
            let buffers = self.create_mesh_buffers(mesh);
            tracing::debug!(target: "engine", mesh = %name, "debug mesh uploaded");
            self.mesh_cache.insert(name, buffers);
        }
    }

    /// Returns true when a mesh is available in the cache.
    pub fn has_mesh(&self, name: &str) -> bool {
        self.mesh_cache.contains_key(name) || name == "debug/cube"
    }

    pub(crate) fn buffers_from_data(
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> MeshBuffers {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster mesh vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster mesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        MeshBuffers {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
        }
    }

    pub(crate) fn mesh_batches_from_world(
        &self,
        world: &RenderWorld,
    ) -> Vec<(String, Vec<Instance>, String, bool)> {
        let batch_capacity = (world.objects.len()
            + usize::from(!world.sprites.is_empty())
            + usize::from(!world.particles.is_empty()))
        .min(32);
        let mut batches: HashMap<(String, String, bool), Vec<Instance>> =
            HashMap::with_capacity(batch_capacity);
        for object in &world.objects {
            let (color, metallic, roughness, emissive) = self.pbr_for_material(&object.material);
            let t = object.transform;
            let mesh = if object.mesh.is_empty() {
                "debug/cube".to_string()
            } else {
                object.mesh.clone()
            };
            let mat = object.material.clone();
            batches
                .entry((mesh, mat, object.casts_shadows))
                .or_default()
                .push(Instance {
                    offset: [t.translation.x, t.translation.y, t.translation.z],
                    scale: [
                        t.scale.x.max(0.05),
                        t.scale.y.max(0.05),
                        t.scale.z.max(0.05),
                    ],
                    color,
                    rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                    metallic,
                    roughness,
                    emissive,
                    receive_shadows: if object.receive_shadows { 1.0 } else { 0.0 },
                });
        }
        if !world.sprites.is_empty() {
            let mut sprites = world.sprites.iter().collect::<Vec<_>>();
            sprites.sort_by(|left, right| {
                left.layer
                    .cmp(&right.layer)
                    .then(left.order_in_layer.cmp(&right.order_in_layer))
            });
            let sprite_instances = sprites.into_iter().map(|sprite| {
                let t = sprite.transform;
                let x = t.scale.x.abs().max(0.01) * if sprite.flip_h { -1.0 } else { 1.0 };
                let y = t.scale.y.abs().max(0.01) * if sprite.flip_v { -1.0 } else { 1.0 };
                Instance {
                    offset: [
                        t.translation.x,
                        t.translation.y,
                        t.translation.z + sprite.order_in_layer as f32 * 0.0001,
                    ],
                    scale: [x, y, t.scale.z.abs().max(0.01)],
                    color: sprite.color,
                    rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                    metallic: 0.0,
                    roughness: 0.5,
                    emissive: [0.0; 3],
                    receive_shadows: 1.0,
                }
            });
            batches
                .entry(("debug/plane".to_string(), String::new(), false))
                .or_default()
                .extend(sprite_instances);
        }
        if !world.particles.is_empty() {
            let particle_instances: Vec<Instance> = world
                .particles
                .iter()
                .map(|particle| {
                    let t = particle.transform;
                    Instance {
                        offset: [t.translation.x, t.translation.y, t.translation.z],
                        scale: [
                            t.scale.x.max(0.01),
                            t.scale.y.max(0.01),
                            t.scale.z.max(0.01),
                        ],
                        color: particle.color,
                        rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                        metallic: 0.0,
                        roughness: 0.5,
                        emissive: [0.0; 3],
                        receive_shadows: 1.0,
                    }
                })
                .collect();
            batches
                .entry(("debug/plane".to_string(), String::new(), false))
                .or_default()
                .extend(particle_instances);
        }
        batches
            .into_iter()
            .map(|((mesh, mat, casts_shadows), instances)| (mesh, instances, mat, casts_shadows))
            .collect()
    }

    pub(crate) fn pbr_for_material(&self, material: &str) -> ([f32; 4], f32, f32, [f32; 3]) {
        if let Some(&params) = self.material_cache.get(material) {
            return params;
        }
        if material.contains("debug") {
            ([0.2, 0.65, 1.0, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
        } else if material.contains("error") {
            ([1.0, 0.2, 0.45, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
        } else {
            ([0.82, 0.86, 0.72, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
        }
    }
}
