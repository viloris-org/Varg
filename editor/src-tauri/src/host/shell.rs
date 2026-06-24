use crate::*;

impl EditorHost {
    /// Increment the scene version counter so the frontend can skip redundant renders.
    pub(crate) fn bump_scene_version(&mut self) {
        self.scene_version = self.scene_version.wrapping_add(1);
    }

    // ── Shell handlers ──

    // ── Copilot handlers ──

    pub(crate) fn shell_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        let selected_entity = self
            .shell
            .selected_entity_id()
            .map(|id| format!("{:032x}", id.as_u128()));
        Ok(serde_json::json!({
            "has_project": self.shell.project().is_some(),
            "project_name": self.shell.project().map(|p| p.name()),
            "scene_dirty": self.shell.is_scene_dirty(),
            "can_undo": self.shell.undo_stack().can_undo(),
            "can_redo": self.shell.undo_stack().can_redo(),
            "scene_version": self.scene_version,
            "selected_entity": selected_entity,
            "desktop_integration": self.desktop_integration.as_json(),
        }))
    }

    pub(crate) fn shell_get_scene_tree(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Ok(serde_json::json!({ "objects": [] }));
        };
        let objects: Vec<Value> = project
            .scene
            .objects()
            .iter()
            .map(|(entity, obj)| {
                let transform = project
                    .scene
                    .transforms()
                    .world(*entity)
                    .unwrap_or_default();
                let parent = project.scene.transforms().parent(*entity);
                let parent_id = parent
                    .and_then(|p| project.scene.object(p))
                    .map(|o| format!("{:032x}", o.id.as_u128()));
                serde_json::json!({
                    "id": format!("{:032x}", obj.id.as_u128()),
                    "name": obj.name,
                    "tag": obj.tag,
                    "parent_id": parent_id,
                    "position": [
                        transform.translation.x,
                        transform.translation.y,
                        transform.translation.z,
                    ],
                })
            })
            .collect();
        Ok(serde_json::json!({ "objects": objects }))
    }

    pub(crate) fn shell_get_entity(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id' parameter"))?;
        let entity_id_val = u128::from_str_radix(id_str, 16)
            .map_err(|_| EngineError::config("invalid entity id"))?;
        let entity_id = engine_core::EntityId::from_u128(entity_id_val);

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;
        let Some(obj) = project.scene.object(entity) else {
            return Err(EngineError::config("entity not found"));
        };
        let transform = project.scene.transforms().world(entity).unwrap_or_default();
        let components: Vec<Value> = obj
            .components
            .iter()
            .filter_map(|c| {
                serde_json::to_value(c).ok().map(|val| {
                    let comp_type = val
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let data = val.get("data").cloned().unwrap_or(serde_json::Value::Null);
                    serde_json::json!({
                        "type": comp_type,
                        "data": data,
                    })
                })
            })
            .collect();

        Ok(serde_json::json!({
            "id": id_str,
            "name": obj.name,
            "tag": obj.tag,
            "transform": {
                "position": [transform.translation.x, transform.translation.y, transform.translation.z],
                "rotation": [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w],
                "scale": [transform.scale.x, transform.scale.y, transform.scale.z],
            },
            "components": components,
        }))
    }

    pub(crate) fn shell_select_entity(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params.get("id").and_then(|v| v.as_str());
        match id_str {
            Some(id) => {
                self.shell
                    .select_entity_id(engine_core::EntityId::from_u128(
                        u128::from_str_radix(id, 16)
                            .map_err(|_| EngineError::config("invalid entity id"))?,
                    ));
                Ok(serde_json::json!({ "selected": id }))
            }
            None => {
                self.shell.selection_mut().clear();
                Ok(serde_json::json!({ "selected": null }))
            }
        }
    }

    pub(crate) fn shell_save_scene(&mut self, _params: &Value) -> EngineResult<Value> {
        let path = self.shell.save_scene()?;
        self.drain_shell_console();
        Ok(serde_json::json!({ "path": path }))
    }

    /// Open a scene from an arbitrary JSON file path.
    /// Reads the file, parses it as a scene, and replaces the current project's scene.
    pub(crate) fn shell_open_scene(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let path = std::path::PathBuf::from(path_str);

        let text = std::fs::read_to_string(&path).map_err(|e| EngineError::Filesystem {
            path: path.clone(),
            source: e,
        })?;
        let new_scene = engine_ecs::Scene::from_json(&text)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        project.scene = new_scene;
        project.scene_path = path.clone();
        project.scene_dirty = false;
        self.bump_scene_version();

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".to_string(),
                file: None,
                line: None,
            },
            message: format!("opened scene {}", path.display()),
        });

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
        }))
    }

    /// Save the scene to a specified path (Save As).
    pub(crate) fn shell_save_scene_as(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let path = std::path::PathBuf::from(path_str);

        let display_path = self.shell.save_scene_as(&path)?;
        self.drain_shell_console();
        self.bump_scene_version();

        Ok(serde_json::json!({ "path": display_path }))
    }

    pub(crate) fn shell_undo(&mut self, _params: &Value) -> EngineResult<Value> {
        let ok = self.shell.undo_scene_command()?;
        self.drain_shell_console();
        self.bump_scene_version();
        Ok(serde_json::json!({ "applied": ok }))
    }

    pub(crate) fn shell_redo(&mut self, _params: &Value) -> EngineResult<Value> {
        let ok = self.shell.redo_scene_command()?;
        self.drain_shell_console();
        self.bump_scene_version();
        Ok(serde_json::json!({ "applied": ok }))
    }

    pub(crate) fn shell_close_project(&mut self, _params: &Value) -> EngineResult<Value> {
        self.stop_play_runtime();
        self.shell.close_project();
        self.durable_state = self.hub.durable_state();
        self.durable_state.last_open_project = None;
        self.hub = HubState::from_durable_state(self.durable_state.clone());
        self.persist_state();
        Ok(serde_json::json!({}))
    }

    // ── Scene Guides ──

    pub(crate) fn scene_get_guides(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Ok(serde_json::json!({ "guides": [] }));
        };

        let mut guides: Vec<Value> = Vec::new();

        for (entity, obj) in project.scene.objects() {
            let transform = project.scene.transforms().world(entity).unwrap_or_default();

            for comp in &obj.components {
                match comp {
                    engine_ecs::ComponentData::Camera(cam) => {
                        guides.push(serde_json::json!({
                            "id": format!("{:032x}", obj.id.as_u128()),
                            "position": [
                                transform.translation.x,
                                transform.translation.y,
                                transform.translation.z,
                            ],
                            "rotation": [0.0_f32, 0.0, 0.0],
                            "componentType": "Camera",
                            "fov": cam.vertical_fov_degrees,
                        }));
                    }
                    engine_ecs::ComponentData::Light(light) => {
                        guides.push(serde_json::json!({
                            "id": format!("{:032x}", obj.id.as_u128()),
                            "position": [
                                transform.translation.x,
                                transform.translation.y,
                                transform.translation.z,
                            ],
                            "rotation": [0.0_f32, 0.0, 0.0],
                            "componentType": "Light",
                            "lightKind": light.kind.as_str(),
                            "lightColor": light.color,
                        }));
                    }
                    _ => {}
                }
            }
        }

        Ok(serde_json::json!({ "guides": guides }))
    }

    // ── Scene CRUD ──

    pub(crate) fn shell_create_object(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("New Object");

        let object_id = self.shell.create_scene_object(name)?;

        if let Some(parent_id_str) = params.get("parent_id").and_then(|v| v.as_str()) {
            let parent_id = engine_core::EntityId::from_u128(
                u128::from_str_radix(parent_id_str, 16)
                    .map_err(|_| EngineError::config("invalid parent id"))?,
            );
            let before = self.scene_snapshot()?;
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };
            let entity = project
                .scene
                .find_by_id(object_id)
                .ok_or_else(|| EngineError::config("created entity not found"))?;
            let parent_entity = project
                .scene
                .find_by_id(parent_id)
                .ok_or_else(|| EngineError::config("parent entity not found"))?;
            project.scene.set_parent(entity, Some(parent_entity))?;
            project.scene_dirty = true;
            let after = self.scene_snapshot()?;
            self.shell
                .push_undo(UndoCommand::new("Reparent Object", name, before, after));
        }

        self.bump_scene_version();

        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;
        let entity = project
            .scene
            .find_by_id(object_id)
            .ok_or_else(|| EngineError::config("created entity not found"))?;
        let obj = project
            .scene
            .object(entity)
            .ok_or_else(|| EngineError::config("created entity metadata not found"))?;
        let transform = project.scene.transforms().world(entity).unwrap_or_default();

        Ok(serde_json::json!({
            "id": format!("{:032x}", obj.id.as_u128()),
            "name": obj.name,
            "tag": obj.tag,
            "position": [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
        }))
    }

    pub(crate) fn shell_rename_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let new_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        self.shell.select_entity_id(entity_id);
        self.shell.rename_selected_scene_object(new_name)?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        if project.scene.find_by_id(entity_id).is_none() {
            return Err(EngineError::config("entity not found"));
        }

        self.bump_scene_version();
        Ok(serde_json::json!({ "renamed": id_str, "name": new_name }))
    }

    pub(crate) fn shell_delete_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        self.shell.select_entity_id(entity_id);
        self.shell.delete_selected_scene_object()?;
        self.bump_scene_version();
        Ok(serde_json::json!({ "deleted": true }))
    }

    pub(crate) fn shell_duplicate_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        let new_entity = project.scene.clone_object(entity)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Duplicate Object", id_str, before, after));
        self.bump_scene_version();

        let project = self.shell.project().unwrap();
        let obj = project.scene.object(new_entity).unwrap();
        let transform = project
            .scene
            .transforms()
            .world(new_entity)
            .unwrap_or_default();

        Ok(serde_json::json!({
            "id": format!("{:032x}", obj.id.as_u128()),
            "name": obj.name,
            "tag": obj.tag,
            "position": [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
        }))
    }

    pub(crate) fn shell_reparent_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let parent_id_str = params.get("parent_id").and_then(|v| v.as_str());

        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        let parent_entity = match parent_id_str {
            Some(pid) => {
                let parent_eid = engine_core::EntityId::from_u128(
                    u128::from_str_radix(pid, 16)
                        .map_err(|_| EngineError::config("invalid parent id"))?,
                );
                Some(
                    project
                        .scene
                        .find_by_id(parent_eid)
                        .ok_or_else(|| EngineError::config("parent entity not found"))?,
                )
            }
            None => None,
        };

        project.scene.set_parent(entity, parent_entity)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Reparent Object", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "reparented": true }))
    }

    pub(crate) fn shell_update_transform(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_core::math::{Quat, Transform, Vec3};

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        // Read current transform as starting point
        let current = project.scene.transforms().local(entity).unwrap_or_default();

        let mut t = Transform {
            translation: current.translation,
            rotation: current.rotation,
            scale: current.scale,
        };

        if let Some(pos) = params.get("position").and_then(|v| v.as_array()) {
            let x = pos
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.x as f64) as f32;
            let y = pos
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.y as f64) as f32;
            let z = pos
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.z as f64) as f32;
            t.translation = Vec3::new(x, y, z);
        }
        if let Some(rot) = params.get("rotation").and_then(|v| v.as_array()) {
            let x = rot
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.x as f64) as f32;
            let y = rot
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.y as f64) as f32;
            let z = rot
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.z as f64) as f32;
            let w = rot
                .get(3)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.w as f64) as f32;
            t.rotation = Quat { x, y, z, w };
        }
        if let Some(scl) = params.get("scale").and_then(|v| v.as_array()) {
            let x = scl
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.x as f64) as f32;
            let y = scl
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.y as f64) as f32;
            let z = scl
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.z as f64) as f32;
            t.scale = Vec3::new(x, y, z);
        }

        let delta = t.translation - current.translation;
        self.shell
            .nudge_selected_scene_object(delta, "Update Transform")?;

        if t.rotation != current.rotation || t.scale != current.scale {
            let before = self.scene_snapshot()?;
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };
            let entity = project
                .scene
                .find_by_id(entity_id)
                .ok_or_else(|| EngineError::config("entity not found"))?;
            let mut transform = project.scene.transforms().local(entity).unwrap_or_default();
            transform.rotation = t.rotation;
            transform.scale = t.scale;
            project.scene.transforms_mut().set_local(entity, transform);
            project.scene_dirty = true;
            let after = self.scene_snapshot()?;
            if before != after {
                self.shell
                    .push_undo(UndoCommand::new("Update Transform", id_str, before, after));
            }
        }
        self.bump_scene_version();
        Ok(serde_json::json!({ "updated": true }))
    }

    pub(crate) fn shell_add_component(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_ecs::ComponentData;

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let component = match comp_type {
            "Camera" => ComponentData::Camera(Default::default()),
            "Light" => ComponentData::Light(Default::default()),
            "MeshRenderer" => ComponentData::MeshRenderer(Default::default()),
            "Rigidbody" => ComponentData::Rigidbody(Default::default()),
            "Collider" => ComponentData::Collider(Default::default()),
            "FluidVolume" => ComponentData::FluidVolume(Default::default()),
            "BuoyancyProbeSet" => ComponentData::BuoyancyProbeSet(Default::default()),
            "WindZone" => ComponentData::WindZone(Default::default()),
            "AudioSource" => ComponentData::AudioSource(Default::default()),
            "AudioListener" => ComponentData::AudioListener(Default::default()),
            "AcousticMaterial" => ComponentData::AcousticMaterial(Default::default()),
            "AcousticGeometry" => ComponentData::AcousticGeometry(Default::default()),
            "AcousticRoom" => ComponentData::AcousticRoom(Default::default()),
            "AcousticPortal" => ComponentData::AcousticPortal(Default::default()),
            "AudioZone" => ComponentData::AudioZone(Default::default()),
            "Script" => ComponentData::Script(engine_ecs::ScriptComponent::new(String::new())),
            _ => {
                return Err(EngineError::config(format!(
                    "unknown component type: {comp_type}"
                )));
            }
        };

        self.shell.select_entity_id(entity_id);
        self.shell
            .add_component_to_selected_scene_object(component)?;
        self.bump_scene_version();
        Ok(serde_json::json!({ "added": comp_type }))
    }

    pub(crate) fn shell_update_component(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_ecs::ComponentData;

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let field_data = params
            .get("data")
            .ok_or_else(|| EngineError::config("missing 'data'"))?;

        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        // Get the current component, merge with new data, and upsert
        let components = project
            .scene
            .components(entity)
            .ok_or_else(|| EngineError::config("entity has no components"))?;

        let current = components
            .iter()
            .find(|c| c.type_id() == comp_type)
            .ok_or_else(|| EngineError::config(format!("entity has no {comp_type} component")))?;

        // Serialize current data, merge fields, deserialize back
        let mut current_val =
            serde_json::to_value(current).map_err(|e| EngineError::other(e.to_string()))?;

        // Merge the new data into the existing component data
        if let Some(obj) = current_val.as_object_mut() {
            if let Some(data_obj) = obj.get_mut("data").and_then(|d| d.as_object_mut()) {
                if let Some(fields) = field_data.as_object() {
                    for (key, value) in fields {
                        data_obj.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        let component: ComponentData = serde_json::from_value(current_val)
            .map_err(|e| EngineError::config(format!("invalid component data: {e}")))?;

        project.scene.upsert_component(entity, component)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Update Component", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "updated": comp_type }))
    }

    pub(crate) fn shell_remove_component(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        self.shell.select_entity_id(entity_id);
        self.shell
            .remove_component_from_selected_scene_object(comp_type)?;
        self.bump_scene_version();
        Ok(serde_json::json!({ "removed": comp_type }))
    }

    pub(crate) fn scene_snapshot(&self) -> EngineResult<String> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        project.scene.to_json(project.name())
    }

    /// Forward console entries from the shell's console service to our shared one.
    pub(crate) fn drain_shell_console(&mut self) {
        for entry in self.shell.console().entries().iter() {
            self.console.push(entry.clone());
        }
    }
}
