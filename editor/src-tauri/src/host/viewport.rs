use crate::*;

impl EditorHost {
    // ── Viewport handlers ──

    /// Render the current scene to an offscreen buffer and return raw RGBA pixels.
    /// Returns `(width, height, rgba_bytes)`.
    /// If `last_version` param matches the current `scene_version`, skips rendering
    /// and returns `(0, 0, empty_vec)` as a no-change signal.
    fn render_viewport(&mut self, params: &Value) -> EngineResult<(u32, u32, Vec<u8>)> {
        use engine_core::math::{Transform, Vec3};
        use engine_render::{RenderCamera, RenderProjection};
        use runtime_min::extract_render_world;

        let play_mode = params
            .get("play_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Lazy rendering: if the scene version hasn't changed, skip the full pipeline
        if !play_mode {
            if let Some(last_ver) = params.get("last_version").and_then(|v| v.as_u64()) {
                if last_ver == self.scene_version {
                    return Ok((0, 0, Vec::new()));
                }
            }
        } else if let Some(last_ver) = params.get("last_version").and_then(|v| v.as_u64()) {
            if last_ver == self.play_version {
                return Ok((0, 0, Vec::new()));
            }
        }

        let (width, height) = (
            params.get("width").and_then(|v| v.as_u64()).unwrap_or(640) as u32,
            params.get("height").and_then(|v| v.as_u64()).unwrap_or(480) as u32,
        );

        tracing::debug!(
            target: "editor",
            width, height, play_mode,
            "render_viewport start"
        );

        // Extract render world from the scene
        let mut world = if play_mode {
            self.tick_play_runtime()?;
            let Some(runtime) = self.play_runtime.as_ref() else {
                return Err(EngineError::config("play mode is not running"));
            };
            extract_render_world(&runtime.scene)
        } else {
            let Some(project) = self.shell.project() else {
                return Err(EngineError::config("no project open"));
            };
            extract_render_world(&project.scene)
        };

        tracing::debug!(
            target: "editor",
            objects = world.objects.len(),
            lights = world.lights.len(),
            has_camera = world.camera.is_some(),
            "render world extracted"
        );

        // Scene View always uses an editor-controlled camera. Game View keeps
        // the camera extracted from the scene, including Camera2D.
        // If entity_id is provided, render from that entity's camera perspective.
        // If editor_camera is true (inline preview), use editor orbit camera on the game scene.
        let entity_id_str = params.get("entity_id").and_then(|v| v.as_str());
        let editor_camera = params
            .get("editor_camera")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !play_mode || editor_camera {
            let camera_yaw = params.get("yaw").and_then(|v| v.as_f64()).unwrap_or(-0.5) as f32;
            let camera_pitch = params.get("pitch").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;
            let camera_dist = params
                .get("distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(6.0) as f32;
            let target_x = params
                .get("target_x")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target_y = params
                .get("target_y")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target_z = params
                .get("target_z")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target = Vec3::new(target_x, target_y, target_z);
            let view_mode = params
                .get("view_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("3d");

            // If entity_id is provided, try to use that entity's camera component
            let use_entity_camera = if let Some(id_str) = entity_id_str {
                if let Some(project) = self.shell.project() {
                    let entity_id = engine_core::EntityId::from_u128(
                        u128::from_str_radix(id_str, 16).unwrap_or(0),
                    );
                    if let Some(entity) = project.scene.find_by_id(entity_id) {
                        if let Some(obj) = project.scene.object(entity) {
                            let has_camera = obj
                                .components
                                .iter()
                                .any(|c| matches!(c, engine_ecs::ComponentData::Camera(_)));
                            if has_camera {
                                let transform =
                                    project.scene.transforms().world(entity).unwrap_or_default();
                                let cam_comp = obj.components.iter().find_map(|c| {
                                    if let engine_ecs::ComponentData::Camera(cam) = c {
                                        Some(cam)
                                    } else {
                                        None
                                    }
                                });
                                if let Some(cam) = cam_comp {
                                    let object = world
                                        .camera
                                        .as_ref()
                                        .map(|camera| camera.object)
                                        .unwrap_or_else(|| engine_core::EntityId::from_u128(0));
                                    world.camera = Some(RenderCamera {
                                        object,
                                        transform: Transform {
                                            translation: transform.translation,
                                            rotation: transform.rotation,
                                            ..Transform::IDENTITY
                                        },
                                        projection: RenderProjection::Perspective,
                                        vertical_fov_degrees: cam.vertical_fov_degrees,
                                        near: cam.near,
                                        far: cam.far,
                                        look_at_target: None,
                                    });
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !use_entity_camera {
                let object = world
                    .camera
                    .as_ref()
                    .map(|camera| camera.object)
                    .unwrap_or_else(|| engine_core::EntityId::from_u128(0));
                let (translation, projection) = if view_mode == "2d" {
                    (
                        Vec3::new(target_x, target_y, target_z + camera_dist),
                        RenderProjection::Orthographic {
                            vertical_size: camera_dist * 2.0,
                        },
                    )
                } else {
                    (
                        Vec3::new(
                            target_x + camera_dist * camera_pitch.cos() * camera_yaw.sin(),
                            target_y + camera_dist * camera_pitch.sin(),
                            target_z + camera_dist * camera_pitch.cos() * camera_yaw.cos(),
                        ),
                        RenderProjection::Perspective,
                    )
                };
                world.camera = Some(RenderCamera {
                    object,
                    transform: Transform {
                        translation,
                        ..Transform::IDENTITY
                    },
                    projection,
                    vertical_fov_degrees: 60.0,
                    near: 0.01,
                    far: 1000.0,
                    look_at_target: Some(target),
                });
            }
        }

        // Lazily create the wgpu render device (with proper error handling)
        if self.render_device.is_none() {
            tracing::info!(target: "engine", width, height, "creating wgpu offscreen device");
            let config = WgpuOffscreenConfig {
                width: width.max(1),
                height: height.max(1),
                format: ImageFormat::Rgba8Srgb,
            };
            self.render_device = Some(WgpuRenderDevice::new_offscreen(config).map_err(|e| {
                tracing::error!(target: "engine", error = %e, "wgpu device creation failed");
                EngineError::other(format!("failed to create wgpu device: {e}"))
            })?);
        }
        let device = self.render_device.as_mut().unwrap();

        // Resize if needed
        let (cur_w, cur_h) = device.default_target_size();
        if cur_w != width || cur_h != height {
            device
                .resize_default_target(width.max(1), height.max(1))
                .map_err(|e| EngineError::other(format!("resize failed: {e}")))?;
        }
        let (cur_gw, cur_gh) = device.game_target_size();
        if cur_gw != width || cur_gh != height {
            device
                .resize_game_target(width.max(1), height.max(1))
                .map_err(|e| EngineError::other(format!("game resize failed: {e}")))?;
        }

        if play_mode {
            // Render to game target, readback from game target
            if let Err(e) = device.render_world_offscreen_game(&world) {
                tracing::error!(target: "engine", error = %e, "game render failed");
                return Err(e);
            }
            let (w, h, rgba) = device.readback_game_target()?;
            tracing::debug!(target: "editor", w, h, bytes = rgba.len(), "game readback ok");
            Ok((w, h, rgba))
        } else {
            // Render to default (scene) target
            if let Err(e) = device.render_world_offscreen(&world) {
                tracing::error!(target: "engine", error = %e, "scene render failed");
                return Err(e);
            }
            let (w, h, rgba) = device.readback_default_target()?;
            tracing::debug!(target: "editor", w, h, bytes = rgba.len(), "scene readback ok");
            Ok((w, h, rgba))
        }
    }

    /// Legacy JSON viewport readback — encodes as PNG + base64.
    /// Prefer `viewport_readback_raw` for performance.
    pub(crate) fn viewport_readback(&mut self, params: &Value) -> EngineResult<Value> {
        let (width, height, rgba) = self.render_viewport(params)?;

        // Encode as PNG
        use image::EncodableLayout;
        let img = image::RgbaImage::from_raw(width.max(1), height.max(1), rgba)
            .ok_or_else(|| EngineError::other("failed to create RGBA image"))?;
        let mut png_bytes = Vec::new();
        {
            use image::ImageEncoder;
            use image::codecs::png::PngEncoder;
            let encoder = PngEncoder::new(&mut png_bytes);
            encoder
                .write_image(
                    img.as_bytes(),
                    img.width(),
                    img.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| EngineError::other(format!("PNG encode failed: {e}")))?;
        }
        let b64 = base64_encode(&png_bytes);

        Ok(serde_json::json!({
            "width": width,
            "height": height,
            "png_base64": b64,
        }))
    }

    /// Binary viewport readback — returns raw RGBA bytes with
    /// [width: u32 LE][height: u32 LE][pixels...] layout.
    /// Frontend receives this as ArrayBuffer via Tauri binary IPC.
    pub(crate) fn viewport_readback_raw(&mut self, params: &Value) -> EngineResult<Vec<u8>> {
        let (width, height, rgba) = self.render_viewport(params)?;

        // Prepend dimensions as u32 LE headers, then raw RGBA pixels
        let mut result = Vec::with_capacity(8 + rgba.len());
        result.extend_from_slice(&(width as u32).to_le_bytes());
        result.extend_from_slice(&(height as u32).to_le_bytes());
        result.extend_from_slice(&rgba);
        Ok(result)
    }
}
