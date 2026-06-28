use std::collections::HashMap;
use std::ops::Range;

use crate::{device::*, meshes::*, uniforms::*};
use engine_render::RenderWorld;
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub(crate) struct RenderBatchInstance {
    pub(crate) instance: Instance,
    pub(crate) shadow_center: engine_core::math::Vec3,
    pub(crate) shadow_radius: f32,
    pub(crate) shadow_depth_min: f32,
    pub(crate) shadow_depth_max: f32,
}

#[derive(Clone)]
pub(crate) struct RenderBatchDraw {
    pub(crate) mesh_name: String,
    pub(crate) count: u32,
    pub(crate) material_name: String,
    pub(crate) casts_shadows: bool,
    pub(crate) instance_start: u32,
    pub(crate) shadow_ranges: [Vec<Range<u32>>; CSM_CASCADE_COUNT],
}

pub(crate) type RenderBatch = (String, Vec<RenderBatchInstance>, String, bool);

fn render_instance(instance: Instance) -> RenderBatchInstance {
    let shadow_center =
        engine_core::math::Vec3::new(instance.offset[0], instance.offset[1], instance.offset[2]);
    let shadow_radius = instance.scale[0]
        .abs()
        .max(instance.scale[1].abs())
        .max(instance.scale[2].abs());
    RenderBatchInstance {
        instance,
        shadow_center,
        shadow_radius,
        shadow_depth_min: f32::NEG_INFINITY,
        shadow_depth_max: f32::INFINITY,
    }
}

fn object_shadow_instance(
    object: &engine_render::RenderObject,
    camera: Option<&engine_render::RenderCamera>,
    instance: Instance,
) -> RenderBatchInstance {
    let scaled_center = engine_core::math::Vec3::new(
        object.bounds.center.x * object.transform.scale.x,
        object.bounds.center.y * object.transform.scale.y,
        object.bounds.center.z * object.transform.scale.z,
    );
    let world_center =
        object.transform.translation + object.transform.rotation.rotate(scaled_center);
    let scale = object
        .transform
        .scale
        .x
        .abs()
        .max(object.transform.scale.y.abs())
        .max(object.transform.scale.z.abs());
    let radius = object.bounds.radius.max(0.0) * scale.max(0.0001);
    let (shadow_depth_min, shadow_depth_max) = camera
        .map(|camera| {
            let forward = camera
                .look_at_target
                .map(|target| (target - camera.transform.translation).normalized())
                .unwrap_or_else(|| {
                    camera
                        .transform
                        .rotation
                        .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                        .normalized()
                });
            let relative = world_center - camera.transform.translation;
            let depth = relative.dot(forward);
            (depth - radius, depth + radius)
        })
        .unwrap_or((f32::NEG_INFINITY, f32::INFINITY));
    RenderBatchInstance {
        instance,
        shadow_center: world_center,
        shadow_radius: radius,
        shadow_depth_min,
        shadow_depth_max,
    }
}

pub(crate) fn shadow_ranges_for_instances(
    instances: &[RenderBatchInstance],
    instance_start: u32,
    cascade_vp: &[[f32; 4]; 4],
    cascade_near: f32,
    cascade_far: f32,
) -> Vec<Range<u32>> {
    let mut ranges = Vec::new();
    let mut active_start: Option<u32> = None;
    for (index, instance) in instances.iter().enumerate() {
        let overlaps = instance.shadow_depth_max >= cascade_near
            && instance.shadow_depth_min <= cascade_far
            && sphere_intersects_cascade_clip(
                cascade_vp,
                instance.shadow_center,
                instance.shadow_radius,
            );
        let instance = instance_start + index as u32;
        match (active_start, overlaps) {
            (None, true) => active_start = Some(instance),
            (Some(start), false) => {
                ranges.push(start..instance);
                active_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = active_start {
        ranges.push(start..instance_start + instances.len() as u32);
    }
    ranges
}

#[allow(dead_code)]
pub(crate) fn shadow_ranges_for_depth_bounds(
    bounds: &[(f32, f32)],
    instance_start: u32,
    cascade_near: f32,
    cascade_far: f32,
) -> Vec<Range<u32>> {
    let instances = bounds
        .iter()
        .map(|(depth_min, depth_max)| RenderBatchInstance {
            instance: Instance {
                offset: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                color: [1.0; 4],
                rotation: [0.0, 0.0, 0.0, 1.0],
                metallic: 0.0,
                roughness: 0.5,
                emissive: [0.0; 3],
                receive_shadows: 1.0,
            },
            shadow_center: engine_core::math::Vec3::ZERO,
            shadow_radius: 0.0,
            shadow_depth_min: *depth_min,
            shadow_depth_max: *depth_max,
        })
        .collect::<Vec<_>>();
    shadow_ranges_for_instances(
        &instances,
        instance_start,
        &crate::math::IDENTITY_MAT4,
        cascade_near,
        cascade_far,
    )
}

pub(crate) fn sphere_intersects_cascade_clip(
    cascade_vp: &[[f32; 4]; 4],
    center: engine_core::math::Vec3,
    radius: f32,
) -> bool {
    let clip_center = crate::math::mul_mat4_vec3(cascade_vp, center);
    let radius = radius.max(0.0);
    let clip_radius_x = radius
        * (cascade_vp[0][0] * cascade_vp[0][0]
            + cascade_vp[1][0] * cascade_vp[1][0]
            + cascade_vp[2][0] * cascade_vp[2][0])
            .sqrt();
    let clip_radius_y = radius
        * (cascade_vp[0][1] * cascade_vp[0][1]
            + cascade_vp[1][1] * cascade_vp[1][1]
            + cascade_vp[2][1] * cascade_vp[2][1])
            .sqrt();
    let clip_radius_z = radius
        * (cascade_vp[0][2] * cascade_vp[0][2]
            + cascade_vp[1][2] * cascade_vp[1][2]
            + cascade_vp[2][2] * cascade_vp[2][2])
            .sqrt();

    clip_center.x + clip_radius_x >= -1.0
        && clip_center.x - clip_radius_x <= 1.0
        && clip_center.y + clip_radius_y >= -1.0
        && clip_center.y - clip_radius_y <= 1.0
        && clip_center.z + clip_radius_z >= 0.0
        && clip_center.z - clip_radius_z <= 1.0
}

pub(crate) fn active_csm_cascade_count(csm: &CsmUniform) -> usize {
    let mut count = 1usize;
    for index in 1..4 {
        if csm.cascade_splits[index] > csm.cascade_splits[index - 1] + f32::EPSILON {
            count += 1;
        }
    }
    count
}

pub(crate) fn csm_cascade_depth_range(csm: &CsmUniform, cascade_idx: usize) -> (f32, f32) {
    let split_idx = cascade_idx.min(3);
    let near = if split_idx == 0 {
        0.0
    } else {
        csm.cascade_splits[split_idx - 1] - CSM_CASCADE_FADE_RANGE
    };
    let far = csm.cascade_splits[split_idx] + CSM_CASCADE_FADE_RANGE;
    (near.max(0.0), far.max(0.0))
}

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
        for mesh in &[
            DebugMesh::Cube,
            DebugMesh::Sphere(8),
            DebugMesh::Cylinder(24),
            DebugMesh::Cone(24),
            DebugMesh::Plane,
        ] {
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
            label: Some("varg mesh vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg mesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        MeshBuffers {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
        }
    }

    pub(crate) fn mesh_batches_from_world_visible(
        &self,
        world: &RenderWorld,
        aspect: f32,
    ) -> (Vec<RenderBatch>, engine_render::VisibilityResult) {
        let visibility = engine_render::select_visibility(world, aspect);
        let batch_capacity = (world.objects.len()
            + usize::from(!world.sprites.is_empty())
            + usize::from(!world.particles.is_empty()))
        .min(32);
        let mut batches: HashMap<(String, String, bool), Vec<RenderBatchInstance>> =
            HashMap::with_capacity(batch_capacity);
        for (&object_index, selected_mesh) in visibility
            .visible_indices
            .iter()
            .zip(&visibility.selected_meshes)
        {
            let object = &world.objects[object_index];
            let (color, metallic, roughness, emissive) =
                self.pbr_for_world_material(world, &object.material);
            let t = object.transform;
            let mesh = if selected_mesh.is_empty() {
                "debug/cube".to_string()
            } else {
                selected_mesh.clone()
            };
            let mat = object.material.clone();
            let instance = Instance {
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
            };
            batches
                .entry((mesh, mat, object.casts_shadows))
                .or_default()
                .push(object_shadow_instance(
                    object,
                    world.camera.as_ref(),
                    instance,
                ));
        }
        let mut batches: Vec<_> = batches
            .into_iter()
            .map(|((mesh, mat, casts_shadows), instances)| (mesh, instances, mat, casts_shadows))
            .collect();
        if !world.sprites.is_empty() {
            let mut sprites = world.sprites.iter().collect::<Vec<_>>();
            sprites.sort_by(|left, right| {
                left.layer
                    .cmp(&right.layer)
                    .then(left.order_in_layer.cmp(&right.order_in_layer))
            });
            let mut current_texture: Option<String> = None;
            let mut current_instances = Vec::new();
            for sprite in sprites {
                let t = sprite.transform;
                let x = t.scale.x.abs().max(0.01) * if sprite.flip_h { -1.0 } else { 1.0 };
                let y = t.scale.y.abs().max(0.01) * if sprite.flip_v { -1.0 } else { 1.0 };
                let instance = render_instance(Instance {
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
                });
                let texture = sprite.texture.clone().unwrap_or_default();
                if current_texture
                    .as_ref()
                    .is_some_and(|current| current != &texture)
                {
                    batches.push((
                        "debug/plane".to_owned(),
                        std::mem::take(&mut current_instances),
                        format!("@sprite:{}", current_texture.take().unwrap_or_default()),
                        false,
                    ));
                }
                current_texture = Some(texture);
                current_instances.push(instance);
            }
            if !current_instances.is_empty() {
                batches.push((
                    "debug/plane".to_owned(),
                    current_instances,
                    format!("@sprite:{}", current_texture.unwrap_or_default()),
                    false,
                ));
            }
        }
        if world.particle_emitters.is_empty() && !world.particles.is_empty() {
            let mut particles = world.particles.iter().collect::<Vec<_>>();
            if let Some(camera) = &world.camera {
                particles.sort_by(|left, right| {
                    let left_distance =
                        (left.transform.translation - camera.transform.translation).length();
                    let right_distance =
                        (right.transform.translation - camera.transform.translation).length();
                    right_distance.total_cmp(&left_distance)
                });
            }
            let camera_rotation = world
                .camera
                .as_ref()
                .map_or(engine_core::math::Quat::IDENTITY, |camera| {
                    camera.transform.rotation
                });
            let particle_instances: Vec<RenderBatchInstance> = particles
                .into_iter()
                .map(|particle| {
                    let t = particle.transform;
                    render_instance(Instance {
                        offset: [t.translation.x, t.translation.y, t.translation.z],
                        scale: [
                            t.scale.x.max(0.01),
                            t.scale.y.max(0.01),
                            t.scale.z.max(0.01),
                        ],
                        color: particle.color,
                        rotation: [
                            camera_rotation.x,
                            camera_rotation.y,
                            camera_rotation.z,
                            camera_rotation.w,
                        ],
                        metallic: 0.0,
                        roughness: 0.5,
                        emissive: [0.0; 3],
                        receive_shadows: 1.0,
                    })
                })
                .collect();
            batches.push((
                "debug/plane".to_owned(),
                particle_instances,
                "@particle".to_owned(),
                false,
            ));
        }
        (batches, visibility)
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

    pub(crate) fn pbr_for_world_material(
        &self,
        world: &RenderWorld,
        material: &str,
    ) -> ([f32; 4], f32, f32, [f32; 3]) {
        if let Some(params) = world.material_params.get(material) {
            return (
                params.base_color,
                params.metallic,
                params.roughness,
                params.emissive,
            );
        }
        self.pbr_for_material(material)
    }
}
