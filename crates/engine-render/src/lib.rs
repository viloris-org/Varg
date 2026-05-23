#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Render abstraction only. Concrete backends live outside `runtime-min`.

use engine_core::{math::Transform, EngineError, EngineResult, EntityId, Handle};

pub mod graph;
pub mod pipeline;
pub mod resource;
pub mod target;

#[cfg(feature = "editor")]
pub mod egui_convert;

pub use graph::{PassId, RenderGraph, RenderGraphBuilder, RenderPass};
pub use pipeline::{
    GuiDrawList, GuiTextureId, MaterialHandle, PipelineDesc, ShaderHandle, ShaderStage,
};
pub use resource::{
    BufferDesc, BufferHandle, BufferUsage, ImageDesc, ImageFormat, ImageHandle, ImageUsage,
    SamplerDesc, SamplerHandle, TextureCache,
};
pub use target::{RenderTarget, RenderTargetDesc, ViewKind};

/// Render API selected by a concrete backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderApi {
    /// No rendering backend.
    Headless,
    /// Vulkan backend.
    Vulkan,
    /// Metal backend.
    Metal,
    /// Direct3D 12 backend.
    D3D12,
    /// WebGPU backend.
    WebGpu,
}

/// Render frame context passed to backends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderFrame {
    /// Frame index.
    pub frame_index: u64,
}

/// Camera data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderCamera {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Projection used to render this camera.
    pub projection: RenderProjection,
    /// Vertical field of view in degrees.
    pub vertical_fov_degrees: f32,
    /// Near clipping plane.
    pub near: f32,
    /// Far clipping plane.
    pub far: f32,
}

/// Camera projection used by a render view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderProjection {
    /// Perspective projection using [`RenderCamera::vertical_fov_degrees`].
    Perspective,
    /// Orthographic projection with the given vertical world-space size.
    Orthographic {
        /// Vertical visible size in world units.
        vertical_size: f32,
    },
}

impl Default for RenderProjection {
    fn default() -> Self {
        Self::Perspective
    }
}

/// Mesh draw data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderObject {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Mesh identifier, either a built-in name or asset label.
    pub mesh: String,
    /// Material identifier, either a built-in name or asset label.
    pub material: String,
}

/// Light data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLight {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Light kind.
    pub kind: String,
    /// RGB light color.
    pub color: engine_core::math::Vec3,
    /// Light intensity.
    pub intensity: f32,
    /// Light range for point and spot lights.
    pub range: f32,
    /// Spot cone angle in degrees for spot lights.
    pub spot_angle: f32,
}

/// Particle draw data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderParticle {
    /// Source emitter scene object.
    pub object: EntityId,
    /// Particle world-space transform.
    pub transform: Transform,
    /// Particle RGBA color.
    pub color: [f32; 4],
    /// Particle normalized age from birth to death.
    pub age_fraction: f32,
}

/// Minimal render queue shared by runtime, editor Scene View, and Game View.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWorld {
    /// Active camera.
    pub camera: Option<RenderCamera>,
    /// Queued mesh renderers.
    pub objects: Vec<RenderObject>,
    /// Queued lights.
    pub lights: Vec<RenderLight>,
    /// Queued particle instances.
    pub particles: Vec<RenderParticle>,
}

impl RenderWorld {
    /// Returns true when there is visible geometry and a camera.
    pub fn is_visible(&self) -> bool {
        self.camera.is_some() && (!self.objects.is_empty() || !self.particles.is_empty())
    }

    /// Extracts renderable data from a [`Scene`](engine_ecs::Scene).
    ///
    /// Iterates all active game objects. For each object, extracts:
    /// - `Camera` → [`RenderCamera`] (stored as `self.camera`)
    /// - `MeshRenderer` + transform → [`RenderObject`]
    /// - `Light` → [`RenderLight`]
    ///
    /// Inactive objects and objects without any renderable component are skipped.
    pub fn extract(scene: &engine_ecs::Scene) -> Self {
        let mut world = RenderWorld::default();

        for (entity, obj) in scene.iter_objects() {
            if !obj.active {
                continue;
            }

            let transform = scene.transforms().local(entity).unwrap_or_default();

            for component in &obj.components {
                match component {
                    engine_ecs::ComponentData::Camera(cam) => {
                        world.camera = Some(RenderCamera {
                            object: obj.id,
                            transform,
                            projection: RenderProjection::Perspective,
                            vertical_fov_degrees: cam.vertical_fov_degrees,
                            near: cam.near,
                            far: cam.far,
                        });
                    }
                    engine_ecs::ComponentData::MeshRenderer(mesh) => {
                        let mesh_name = mesh
                            .builtin_mesh
                            .clone()
                            .or_else(|| mesh.mesh.map(|id| format!("asset:{:016x}", id.as_u128())))
                            .unwrap_or_default();
                        let material_name = mesh
                            .material
                            .builtin
                            .clone()
                            .or_else(|| {
                                mesh.material
                                    .asset
                                    .map(|id| format!("asset:{:016x}", id.as_u128()))
                            })
                            .unwrap_or_default();
                        world.objects.push(RenderObject {
                            object: obj.id,
                            transform,
                            mesh: mesh_name,
                            material: material_name,
                        });
                    }
                    engine_ecs::ComponentData::Light(light) => {
                        world.lights.push(RenderLight {
                            object: obj.id,
                            transform,
                            kind: light.kind.clone(),
                            color: light.color,
                            intensity: light.intensity,
                            range: light.range,
                            spot_angle: light.spot_angle,
                        });
                    }
                    engine_ecs::ComponentData::ParticleEmitter(emitter) => {
                        world.particles.extend(
                            engine_ecs::ParticleSystem::sample(emitter, transform)
                                .into_iter()
                                .map(|particle| {
                                    let mut particle_transform = Transform::IDENTITY;
                                    particle_transform.translation = particle.position;
                                    particle_transform.scale = engine_core::math::Vec3::new(
                                        particle.size,
                                        particle.size,
                                        particle.size,
                                    );
                                    RenderParticle {
                                        object: obj.id,
                                        transform: particle_transform,
                                        color: particle.color,
                                        age_fraction: particle.age_fraction,
                                    }
                                }),
                        );
                    }
                    _ => {}
                }
            }
        }

        world
    }
}

/// Render backend abstraction.
pub trait RenderDevice {
    /// Returns the concrete API exposed by this device.
    fn api(&self) -> RenderApi;

    /// Renders one frame using the compiled graph.
    fn render(&mut self, frame: RenderFrame) -> EngineResult<()>;

    /// Submits a scene extraction to the backend for rendering.
    fn submit_render_world(&mut self, world: &RenderWorld, frame: RenderFrame) -> EngineResult<()> {
        let _ = world;
        self.render(frame)
    }

    /// Executes a compiled render graph.
    fn execute_graph(&mut self, graph: &RenderGraph, frame: RenderFrame) -> EngineResult<()>;

    /// Creates a render target.
    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget>;

    /// Destroys a render target, queuing GPU cleanup.
    fn destroy_render_target(&mut self, target: RenderTarget);

    /// Creates a GPU image.
    fn create_image(&mut self, desc: ImageDesc) -> EngineResult<ImageHandle>;

    /// Creates a GPU image and uploads tightly packed pixel data into mip 0.
    fn upload_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<ImageHandle> {
        let _ = data;
        self.create_image(desc)
    }

    /// Destroys a GPU image.
    fn destroy_image(&mut self, handle: ImageHandle);

    /// Creates a GPU buffer.
    fn create_buffer(&mut self, desc: BufferDesc) -> EngineResult<BufferHandle>;

    /// Destroys a GPU buffer.
    fn destroy_buffer(&mut self, handle: BufferHandle);

    /// Uploads a GUI texture and returns its backend id.
    fn upload_gui_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<GuiTextureId>;

    /// Submits a GUI draw list for rendering.
    fn draw_gui(&mut self, draw_list: &GuiDrawList) -> EngineResult<()>;

    /// Uploads a mesh to the GPU with vertex data and indices.
    ///
    /// The default implementation is a no-op; backends that support mesh
    /// rendering should override this.
    fn upload_mesh_data(
        &mut self,
        _mesh_name: &str,
        _positions: &[[f32; 3]],
        _normals: &[[f32; 3]],
        _texcoords: &[[f32; 2]],
        _indices: &[u32],
    ) -> EngineResult<()> {
        Ok(())
    }

    /// Flushes the delayed destruction queue for the given frame.
    fn flush_destroy_queue(&mut self, frame_index: u64);

    /// Draws a batch of 2D quads.
    fn draw_2d_batch(
        &mut self,
        _vertices: &[[f32; 8]],
        _texture: ImageHandle,
    ) -> EngineResult<()> {
        Ok(())
    }

    /// Uploads bone matrices for GPU skinning.
    fn upload_bone_matrices(
        &mut self,
        _matrices: &[[f32; 16]],
    ) -> EngineResult<BufferHandle> {
        Ok(BufferHandle(engine_core::Handle::new(
            0,
            engine_core::Generation::FIRST,
        )))
    }

    /// Draws a skinned mesh with bone matrices applied.
    fn draw_skinned_mesh(
        &mut self,
        _mesh_name: &str,
        _material_name: &str,
        _bone_buffer: BufferHandle,
        _bone_count: u32,
    ) -> EngineResult<()> {
        Ok(())
    }
}

/// Null renderer used by minimal runtime builds.
#[derive(Clone, Debug, Default)]
pub struct HeadlessRenderDevice {
    frame_index: u64,
}

impl RenderDevice for HeadlessRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::Headless
    }

    fn render(&mut self, frame: RenderFrame) -> EngineResult<()> {
        self.frame_index = frame.frame_index;
        Ok(())
    }

    fn execute_graph(&mut self, _graph: &RenderGraph, frame: RenderFrame) -> EngineResult<()> {
        self.frame_index = frame.frame_index;
        Ok(())
    }

    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        Ok(RenderTarget {
            handle: Handle::new(0, engine_core::Generation::FIRST),
            desc,
        })
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {}

    fn create_image(&mut self, _desc: ImageDesc) -> EngineResult<ImageHandle> {
        Ok(ImageHandle(Handle::new(0, engine_core::Generation::FIRST)))
    }

    fn upload_texture(&mut self, desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
        self.create_image(desc)
    }

    fn destroy_image(&mut self, _handle: ImageHandle) {}

    fn create_buffer(&mut self, _desc: BufferDesc) -> EngineResult<BufferHandle> {
        Ok(BufferHandle(Handle::new(0, engine_core::Generation::FIRST)))
    }

    fn destroy_buffer(&mut self, _handle: BufferHandle) {}

    fn upload_gui_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        Ok(GuiTextureId(0))
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Ok(())
    }

    fn flush_destroy_queue(&mut self, _frame_index: u64) {}
}

/// Placeholder for profiles that request a concrete backend before one is linked.
#[derive(Clone, Debug, Default)]
pub struct MissingRenderDevice;

impl RenderDevice for MissingRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::Headless
    }

    fn render(&mut self, _frame: RenderFrame) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn execute_graph(&mut self, _graph: &RenderGraph, _frame: RenderFrame) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn create_render_target(&mut self, _desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {}

    fn create_image(&mut self, _desc: ImageDesc) -> EngineResult<ImageHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn upload_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_image(&mut self, _handle: ImageHandle) {}

    fn create_buffer(&mut self, _desc: BufferDesc) -> EngineResult<BufferHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_buffer(&mut self, _handle: BufferHandle) {}

    fn upload_gui_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn flush_destroy_queue(&mut self, _frame_index: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_ecs::{ComponentData, Scene};

    #[test]
    fn headless_renderer_accepts_frame() {
        let mut renderer = HeadlessRenderDevice::default();
        renderer.render(RenderFrame { frame_index: 0 }).unwrap();
        assert_eq!(renderer.api(), RenderApi::Headless);
    }

    #[test]
    fn headless_executes_empty_graph() {
        let mut renderer = HeadlessRenderDevice::default();
        let graph = RenderGraphBuilder::new().build();
        renderer
            .execute_graph(&graph, RenderFrame { frame_index: 1 })
            .unwrap();
    }

    #[test]
    fn extract_scene_with_camera_and_meshes() {
        let mut scene = Scene::new();

        // Camera entity
        let cam_entity = scene.create_object("Main Camera").unwrap();
        scene
            .upsert_component(
                cam_entity,
                ComponentData::Camera(engine_ecs::CameraComponentData {
                    vertical_fov_degrees: 60.0,
                    near: 0.1,
                    far: 100.0,
                    aspect_ratio: None,
                    primary: true,
                    clear_color: engine_core::math::Vec3::new(0.1, 0.1, 0.1),
                }),
            )
            .unwrap();

        // First mesh entity
        let mesh1 = scene.create_object("Cube").unwrap();
        scene
            .upsert_component(
                mesh1,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    mesh: None,
                    builtin_mesh: Some("debug/cube".to_string()),
                    material: engine_ecs::MaterialRef::debug(),
                    casts_shadows: true,
                    receive_shadows: true,
                }),
            )
            .unwrap();

        // Second mesh entity
        let mesh2 = scene.create_object("Sphere").unwrap();
        scene
            .upsert_component(
                mesh2,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    mesh: None,
                    builtin_mesh: Some("debug/sphere".to_string()),
                    material: engine_ecs::MaterialRef::debug(),
                    casts_shadows: false,
                    receive_shadows: true,
                }),
            )
            .unwrap();

        // Inactive entity — should be skipped
        let inactive = scene.create_object("Inactive").unwrap();
        scene
            .upsert_component(
                inactive,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData::default()),
            )
            .unwrap();
        scene.object_mut(inactive).unwrap().active = false;

        let world = RenderWorld::extract(&scene);

        assert!(world.camera.is_some(), "should have a camera");
        assert_eq!(world.objects.len(), 2, "should have 2 active mesh objects");
        assert_eq!(world.lights.len(), 0, "should have no lights");

        let cam = world.camera.unwrap();
        assert_eq!(cam.projection, RenderProjection::Perspective);
        assert_eq!(cam.vertical_fov_degrees, 60.0);
        assert_eq!(cam.near, 0.1);
        assert_eq!(cam.far, 100.0);

        // Verify mesh names
        let cube = world
            .objects
            .iter()
            .find(|o| o.mesh == "debug/cube")
            .unwrap();
        let sphere = world
            .objects
            .iter()
            .find(|o| o.mesh == "debug/sphere")
            .unwrap();
        assert_eq!(cube.material, "debug/default");
        assert_eq!(sphere.material, "debug/default");
        assert_ne!(sphere.object.as_u128(), 0);
    }

    #[test]
    fn extracts_particle_emitters() {
        let mut scene = Scene::new();
        let camera = scene.create_object("Camera").unwrap();
        scene
            .upsert_component(
                camera,
                ComponentData::Camera(engine_ecs::CameraComponentData::default()),
            )
            .unwrap();
        let emitter = scene.create_object("Emitter").unwrap();
        scene
            .upsert_component(
                emitter,
                ComponentData::ParticleEmitter(engine_ecs::ParticleEmitterComponentData {
                    elapsed: 0.5,
                    ..engine_ecs::ParticleEmitterComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);

        assert!(world.is_visible());
        assert!(!world.particles.is_empty());
    }
}
