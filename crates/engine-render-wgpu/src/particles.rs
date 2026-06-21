use crate::{device::WgpuRenderDevice, meshes::MeshBuffers};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuParticleEmitter {
    origin: [f32; 4],
    gravity_lifetime: [f32; 4],
    start_color: [f32; 4],
    end_color: [f32; 4],
    motion: [f32; 4],
    emission: [f32; 4],
    meta: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuParticleInstance {
    offset: [f32; 4],
    scale: [f32; 4],
    color: [f32; 4],
}

pub(crate) struct GpuParticlePipeline {
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    compute_bind_group_layout: wgpu::BindGroupLayout,
    compute_bind_group: wgpu::BindGroup,
    camera_bind_group: wgpu::BindGroup,
    emitter_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    emitter_capacity: usize,
    instance_capacity: usize,
    emitter_count: u32,
    block_capacity: u32,
}

impl GpuParticlePipeline {
    pub(crate) fn new(device: &wgpu::Device, camera_uniform: &wgpu::Buffer) -> Self {
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster gpu particle compute shader"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_COMPUTE_SHADER.into()),
        });
        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster gpu particle render shader"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_RENDER_SHADER.into()),
        });
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster gpu particle compute layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let compute_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster gpu particle compute pipeline layout"),
            bind_group_layouts: &[Some(&compute_bind_group_layout)],
            immediate_size: 0,
        });
        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aster gpu particle compute pipeline"),
            layout: Some(&compute_layout),
            module: &compute_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster gpu particle camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster gpu particle camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform.as_entire_binding(),
            }],
        });
        let render_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster gpu particle render pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster gpu particle render pipeline"),
            layout: Some(&render_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<crate::uniforms::Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32x3,
                            2 => Float32x2,
                            3 => Float32x4
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GpuParticleInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            4 => Float32x4,
                            5 => Float32x4,
                            6 => Float32x4
                        ],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });

        let emitter_capacity = 1;
        let instance_capacity = 1;
        let emitter_buffer = create_emitter_buffer(device, emitter_capacity);
        let instance_buffer = create_instance_buffer(device, instance_capacity);
        let compute_bind_group = create_compute_bind_group(
            device,
            &compute_bind_group_layout,
            &emitter_buffer,
            &instance_buffer,
        );
        Self {
            compute_pipeline,
            render_pipeline,
            compute_bind_group_layout,
            compute_bind_group,
            camera_bind_group,
            emitter_buffer,
            instance_buffer,
            emitter_capacity,
            instance_capacity,
            emitter_count: 0,
            block_capacity: 0,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        emitters: &[engine_render::RenderParticleEmitter],
    ) {
        self.emitter_count = emitters.len() as u32;
        self.block_capacity = emitters
            .iter()
            .map(|emitter| emitter.max_particles)
            .max()
            .unwrap_or(0);
        if self.emitter_count == 0 || self.block_capacity == 0 {
            return;
        }
        let required_emitters = emitters.len();
        let required_instances = required_emitters.saturating_mul(self.block_capacity as usize);
        let mut recreate_bind_group = false;
        if required_emitters > self.emitter_capacity {
            self.emitter_capacity = required_emitters.next_power_of_two();
            self.emitter_buffer = create_emitter_buffer(device, self.emitter_capacity);
            recreate_bind_group = true;
        }
        if required_instances > self.instance_capacity {
            self.instance_capacity = required_instances.next_power_of_two();
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
            recreate_bind_group = true;
        }
        if recreate_bind_group {
            self.compute_bind_group = create_compute_bind_group(
                device,
                &self.compute_bind_group_layout,
                &self.emitter_buffer,
                &self.instance_buffer,
            );
        }
        let gpu_emitters: Vec<_> = emitters
            .iter()
            .map(|emitter| GpuParticleEmitter {
                origin: [
                    emitter.transform.translation.x,
                    emitter.transform.translation.y,
                    emitter.transform.translation.z,
                    1.0,
                ],
                gravity_lifetime: [
                    emitter.gravity.x,
                    emitter.gravity.y,
                    emitter.gravity.z,
                    emitter.lifetime,
                ],
                start_color: emitter.start_color,
                end_color: emitter.end_color,
                motion: [
                    emitter.start_speed,
                    emitter.size_range[0],
                    emitter.size_range[1],
                    emitter.elapsed,
                ],
                emission: [
                    emitter.emission_rate,
                    emitter.spread_degrees.to_radians(),
                    0.0,
                    0.0,
                ],
                meta: [
                    emitter.max_particles,
                    emitter.seed,
                    u32::from(emitter.looping),
                    self.block_capacity,
                ],
            })
            .collect();
        queue.write_buffer(&self.emitter_buffer, 0, bytemuck::cast_slice(&gpu_emitters));
    }

    pub(crate) fn encode_compute(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.emitter_count == 0 || self.block_capacity == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aster gpu particle simulation"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.compute_pipeline);
        pass.set_bind_group(0, &self.compute_bind_group, &[]);
        pass.dispatch_workgroups(self.block_capacity.div_ceil(64), 1, self.emitter_count);
    }

    pub(crate) fn encode_render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        normal: &wgpu::TextureView,
        albedo: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        plane: &MeshBuffers,
    ) {
        let instance_count = self.emitter_count.saturating_mul(self.block_capacity);
        if instance_count == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aster gpu particle render"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: normal,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: albedo,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, plane.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(plane.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..plane.index_count, 0, 0..instance_count);
    }

    pub(crate) fn instance_count(&self) -> u32 {
        self.emitter_count.saturating_mul(self.block_capacity)
    }
}

fn create_emitter_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aster gpu particle emitters"),
        size: (capacity.max(1) * std::mem::size_of::<GpuParticleEmitter>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aster gpu particle instances"),
        size: (capacity.max(1) * std::mem::size_of::<GpuParticleInstance>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    })
}

fn create_compute_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    emitters: &wgpu::Buffer,
    instances: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("aster gpu particle compute bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: emitters.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instances.as_entire_binding(),
            },
        ],
    })
}

const PARTICLE_COMPUTE_SHADER: &str = r#"
struct Emitter {
    origin: vec4<f32>,
    gravity_lifetime: vec4<f32>,
    start_color: vec4<f32>,
    end_color: vec4<f32>,
    motion: vec4<f32>,
    emission: vec4<f32>,
    config: vec4<u32>,
};

struct ParticleInstance {
    offset: vec4<f32>,
    scale: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(0) @binding(1) var<storage, read_write> particles: array<ParticleInstance>;

fn hash(seed: u32) -> f32 {
    var value = seed;
    value = value ^ (value >> 16u);
    value = value * 0x7feb352du;
    value = value ^ (value >> 15u);
    value = value * 0x846ca68bu;
    value = value ^ (value >> 16u);
    return f32(value) / 4294967295.0;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let emitter_index = gid.z;
    let particle_index = gid.x;
    let emitter = emitters[emitter_index];
    let output_index = emitter_index * emitter.config.w + particle_index;
    if (particle_index >= emitter.config.w) { return; }
    if (particle_index >= emitter.config.x || emitter.emission.x <= 0.0 || emitter.gravity_lifetime.w <= 0.0) {
        particles[output_index].scale = vec4<f32>(0.0);
        particles[output_index].color = vec4<f32>(0.0);
        return;
    }
    let interval = 1.0 / emitter.emission.x;
    let live_capacity = min(emitter.config.x, u32(ceil(emitter.emission.x * emitter.gravity_lifetime.w)));
    let emitted = select(
        min(live_capacity, u32(floor(emitter.motion.w * emitter.emission.x))),
        live_capacity,
        emitter.config.z != 0u
    );
    if (particle_index >= emitted) {
        particles[output_index].scale = vec4<f32>(0.0);
        particles[output_index].color = vec4<f32>(0.0);
        return;
    }
    let live_window = interval * f32(max(live_capacity, 1u));
    let phase = select(emitter.motion.w, emitter.motion.w % live_window, emitter.config.z != 0u);
    var age = phase - f32(particle_index) * interval;
    if (emitter.config.z != 0u) {
        age = ((age % live_window) + live_window) % live_window;
    }
    if (age < 0.0 || age > emitter.gravity_lifetime.w) {
        particles[output_index].scale = vec4<f32>(0.0);
        particles[output_index].color = vec4<f32>(0.0);
        return;
    }
    let random0 = hash(emitter.config.y ^ particle_index * 0x9e3779b9u);
    let random1 = hash(emitter.config.y ^ particle_index * 0x85ebca6bu);
    let yaw = random0 * 6.283185307;
    let cone = random1 * emitter.emission.y;
    let direction = normalize(vec3<f32>(sin(cone) * cos(yaw), cos(cone), sin(cone) * sin(yaw)));
    let velocity = direction * emitter.motion.x;
    let position = emitter.origin.xyz + velocity * age + emitter.gravity_lifetime.xyz * (0.5 * age * age);
    let t = clamp(age / emitter.gravity_lifetime.w, 0.0, 1.0);
    let size = max(mix(emitter.motion.y, emitter.motion.z, t), 0.001);
    particles[output_index].offset = vec4<f32>(position, 1.0);
    particles[output_index].scale = vec4<f32>(size, size, 1.0, 0.0);
    particles[output_index].color = mix(emitter.start_color, emitter.end_color, t);
}
"#;

const PARTICLE_RENDER_SHADER: &str = r#"
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_forward: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) offset: vec4<f32>,
    @location(5) scale: vec4<f32>,
    @location(6) color: vec4<f32>,
};
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) albedo: vec4<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    let forward = normalize(camera.camera_forward.xyz);
    var right = normalize(cross(forward, vec3<f32>(0.0, 1.0, 0.0)));
    if (length(right) < 0.01) { right = vec3<f32>(1.0, 0.0, 0.0); }
    let up = normalize(cross(right, forward));
    let world = input.offset.xyz
        + right * input.position.x * input.scale.x
        + up * input.position.y * input.scale.y;
    var out: VsOut;
    out.position = camera.view_projection * vec4<f32>(world, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> FsOut {
    let centered = input.uv * 2.0 - 1.0;
    let radial = dot(centered, centered);
    let alpha = input.color.a * smoothstep(1.0, 0.35, radial);
    var out: FsOut;
    out.color = vec4<f32>(input.color.rgb * alpha, alpha);
    out.normal = vec4<f32>(0.5, 0.5, 1.0, 1.0);
    out.albedo = vec4<f32>(input.color.rgb, 0.0);
    return out;
}
"#;

impl WgpuRenderDevice {
    pub(crate) fn prepare_gpu_particles(&mut self, world: &engine_render::RenderWorld) {
        self.gpu_particles
            .prepare(&self.device, &self.queue, &world.particle_emitters);
    }

    pub(crate) fn encode_gpu_particle_compute(&self, encoder: &mut wgpu::CommandEncoder) {
        self.gpu_particles.encode_compute(encoder);
    }

    pub(crate) fn encode_gpu_particle_render(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(hdr) = self.hdr_target.as_ref() else {
            return;
        };
        let Some(plane) = self.mesh_cache.get("debug/plane") else {
            return;
        };
        self.gpu_particles.encode_render(
            encoder,
            &hdr.color_view,
            self.hdr_normal_view.as_ref().unwrap(),
            self.hdr_albedo_view.as_ref().unwrap(),
            hdr.depth_view.as_ref(),
            plane,
        );
    }
}
