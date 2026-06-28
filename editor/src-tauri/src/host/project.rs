use crate::*;

impl EditorHost {
    pub(crate) fn app_open_folder(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }

        Ok(serde_json::json!({ "opened": true }))
    }

    // ── Hub handlers ──

    pub(crate) fn hub_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "page": match self.hub.page() {
                engine_editor::ui_state::HubPage::Projects => "projects",
                engine_editor::ui_state::HubPage::Installs => "installs",
                engine_editor::ui_state::HubPage::Settings => "settings",
            },
            "theme": match self.hub.preferences().theme {
                ThemePreference::Dark => "dark",
                ThemePreference::Light => "light",
                ThemePreference::System => "system",
            },
            "recent_projects": self.hub.filtered_projects().iter().map(|p| serde_json::json!({
                "name": p.name,
                "path": p.path.to_string_lossy(),
                "last_touched": p.last_touched,
                "toolchain_version": p.toolchain_version,
            })).collect::<Vec<_>>(),
            "locale": locale_code(self.hub.preferences().locale),
            "installs": self.hub.installs().iter().map(|i| serde_json::json!({
                "version": i.version,
                "path": i.path.to_string_lossy(),
                "editor_available": i.editor_available,
                "runtime_available": i.runtime_available,
            })).collect::<Vec<_>>(),
            "open_project": self.shell.project().map(|p| p.root.to_string_lossy()),
            "last_open_project": self.durable_state.last_open_project.as_ref().map(|p| p.to_string_lossy()),
            "reopen_last_project": self.hub.preferences().reopen_last_project,
            "desktop_integration": self.desktop_integration.as_json(),
        }))
    }

    pub(crate) fn hub_list_projects(&mut self, _params: &Value) -> EngineResult<Value> {
        let projects: Vec<Value> = self
            .hub
            .filtered_projects()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "path": p.path.to_string_lossy(),
                    "last_touched": p.last_touched,
                    "toolchain_version": p.toolchain_version,
                })
            })
            .collect();
        Ok(serde_json::json!({ "projects": projects }))
    }

    pub(crate) fn hub_open_project(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path' parameter"))?;
        let project_path = PathBuf::from(path);

        // Load the project into the editor shell
        self.shell.open_project(&project_path)?;

        // Mark as recent
        let name = self
            .shell
            .project()
            .map(|p| p.name().to_owned())
            .unwrap_or_else(|| {
                project_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        let metadata = ProjectMetadata::new(&name, &project_path, timestamp_now(), "0.1.0");
        self.hub.upsert_project(metadata);

        // Persist state
        self.hub.mark_project_open(project_path.clone());
        self.sync_durable_state();

        // Forward console entries from shell open
        self.drain_shell_console();

        Ok(serde_json::json!({
            "name": name,
            "path": project_path.to_string_lossy(),
        }))
    }

    pub(crate) fn hub_create_project(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name' parameter"))?;
        let location = params
            .get("location")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'location' parameter"))?;

        let request = engine_editor::NewProjectRequest {
            name: name.to_owned(),
            location: Some(PathBuf::from(location)),
            template_id: params
                .get("template_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
            toolchain_version: params
                .get("toolchain_version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
        };

        let plan = self.hub.create_project_plan(&request)?;
        self.hub.create_project_files(&plan)?;

        let metadata = ProjectMetadata::new(
            &plan.name,
            &plan.path,
            timestamp_now(),
            &plan.toolchain_version,
        );
        self.hub.upsert_project(metadata);
        self.sync_durable_state();

        Ok(serde_json::json!({
            "name": plan.name,
            "path": plan.path.to_string_lossy(),
        }))
    }

    pub(crate) fn hub_delete_project(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path' parameter"))?;
        let confirmed = params
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let project_path = PathBuf::from(path);
        let decision = self.hub.request_project_deletion(
            &project_path,
            ProjectDeletionMode::RemoveRecent,
            confirmed,
        );

        match decision {
            ProjectDeletionDecision::RemovedFromRecent { .. } => {
                self.sync_durable_state();
                Ok(serde_json::json!({ "status": "removed" }))
            }
            ProjectDeletionDecision::NeedsConfirmation { .. } => {
                Ok(serde_json::json!({ "status": "needs_confirmation" }))
            }
            ProjectDeletionDecision::RefusedOpenProject { .. } => {
                Err(EngineError::config("cannot delete an open project"))
            }
            ProjectDeletionDecision::DeleteFilesApproved { .. } => Err(EngineError::config(
                "file deletion not supported through IPC",
            )),
        }
    }

    pub(crate) fn hub_set_theme(&mut self, params: &Value) -> EngineResult<Value> {
        let theme = params
            .get("theme")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'theme' parameter"))?;
        let pref = match theme {
            "light" => ThemePreference::Light,
            "dark" => ThemePreference::Dark,
            _ => ThemePreference::System,
        };
        self.hub.set_theme(pref);
        self.sync_durable_state();
        Ok(serde_json::json!({ "theme": theme }))
    }

    pub(crate) fn hub_set_page(&mut self, params: &Value) -> EngineResult<Value> {
        let page = params
            .get("page")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'page' parameter"))?;
        use engine_editor::ui_state::HubPage;
        let p = match page {
            "installs" => HubPage::Installs,
            "settings" => HubPage::Settings,
            _ => HubPage::Projects,
        };
        self.hub.set_page(p);
        self.sync_durable_state();
        Ok(serde_json::json!({ "page": page }))
    }

    pub(crate) fn hub_get_translations(&mut self, params: &Value) -> EngineResult<Value> {
        let requested_locale = params.get("locale").and_then(Value::as_str);
        let translations;
        let active_translations = if requested_locale.is_some() {
            translations = Translations::load(parse_locale(requested_locale));
            &translations
        } else {
            &self.translations
        };
        let entries: Vec<serde_json::Value> = active_translations
            .entries()
            .into_iter()
            .map(|(k, v)| serde_json::json!({ "key": k, "value": v }))
            .collect();
        Ok(serde_json::json!({
            "locale": locale_code(active_translations.locale()),
            "entries": entries,
        }))
    }

    pub(crate) fn hub_set_locale(&mut self, params: &Value) -> EngineResult<Value> {
        let locale_str = params
            .get("locale")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'locale' parameter"))?;
        let locale = parse_locale(Some(locale_str));
        self.hub.set_locale(locale);
        // Reload translations for the new locale
        self.translations = Translations::load(locale);
        self.sync_durable_state();
        Ok(serde_json::json!({ "locale": locale_code(locale) }))
    }

    // ── Project handlers ──

    pub(crate) fn project_list_assets(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let entries: Vec<Value> = project
            .database
            .iter_entries()
            .map(|entry| {
                serde_json::json!({
                    "guid": entry.guid.to_string(),
                    "path": entry.path.to_string_lossy(),
                    "kind": format!("{:?}", entry.kind),
                })
            })
            .collect();

        // Also get assets from ProjectContext.sorted_assets() for richer metadata
        let assets: Vec<Value> = project
            .sorted_assets()
            .iter()
            .map(|meta| {
                serde_json::json!({
                    "guid": meta.guid.to_string(),
                    "source_path": meta.source_path.to_string_lossy(),
                    "kind": format!("{:?}", meta.kind),
                    "importer": meta.importer,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "entries": entries,
            "assets": assets,
        }))
    }

    pub(crate) fn project_list_files(&mut self, params: &Value) -> EngineResult<Value> {
        let include_hidden = params
            .get("include_hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_entries = params
            .get("max_entries")
            .and_then(Value::as_u64)
            .unwrap_or(2_000) as usize;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let root = project
            .root
            .canonicalize()
            .map_err(|source| EngineError::Filesystem {
                path: project.root.clone(),
                source,
            })?;
        let asset_root = project
            .root
            .join(&project.manifest.asset_root)
            .canonicalize()
            .ok();
        let mut stack = vec![root.clone()];
        let mut files = Vec::new();

        while let Some(dir) = stack.pop() {
            if files.len() >= max_entries {
                break;
            }
            let entries = std::fs::read_dir(&dir).map_err(|source| EngineError::Filesystem {
                path: dir.clone(),
                source,
            })?;
            let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|source| {
                EngineError::Filesystem {
                    path: dir.clone(),
                    source,
                }
            })?;
            entries.sort_by_key(|entry| entry.path());

            for entry in entries {
                if files.len() >= max_entries {
                    break;
                }
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();
                let hidden = file_name.starts_with('.');
                if hidden && !include_hidden {
                    continue;
                }
                let metadata = entry.metadata().map_err(|source| EngineError::Filesystem {
                    path: path.clone(),
                    source,
                })?;
                let is_dir = metadata.is_dir();
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let extension = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let asset_path = asset_root.as_ref().and_then(|asset_root| {
                    path.strip_prefix(asset_root)
                        .ok()
                        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                });
                let text = is_text_project_file(&extension, &file_name);

                files.push(serde_json::json!({
                    "path": relative,
                    "name": file_name,
                    "kind": if is_dir { "directory" } else { "file" },
                    "hidden": hidden,
                    "text": text,
                    "asset_path": asset_path,
                }));

                if is_dir {
                    stack.push(path);
                }
            }
        }

        Ok(serde_json::json!({
            "root": root.to_string_lossy(),
            "truncated": files.len() >= max_entries,
            "files": files,
        }))
    }

    pub(crate) fn project_import_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        project.import_file(std::path::PathBuf::from(path))?;
        self.console.push(engine_editor::ConsoleEntry {
            timestamp: "now".into(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Imported file: {path}"),
        });

        Ok(serde_json::json!({"imported": path}))
    }

    pub(crate) fn project_create_script(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let script_root = project.root.join(project.manifest.primary_script_root());
        std::fs::create_dir_all(&script_root).map_err(|source| EngineError::Filesystem {
            path: script_root.clone(),
            source,
        })?;

        let script_path = format!("{}/{name}.varg", project.manifest.primary_script_root());
        let full_path = project.root.join(&script_path);

        let template = format!(
            r#"script {name} {{
    @export var speed: Float = 6.0

    func start() {{
        log("{name} ready")
    }}

    func update(_ dt: Float) {{
    }}
}}
"#
        );

        // Check if parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        std::fs::write(&full_path, template).map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: "now".into(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: Some(full_path.clone()),
                line: None,
            },
            message: format!("Created script: {}", full_path.display()),
        });

        Ok(serde_json::json!({
            "path": script_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_create_material(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_material_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("materials/{name}.vasset"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "material", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_create_animation(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_animation_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("animations/{name}.vasset"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "animation", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_create_audio_bus(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_audio_bus_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("audio/{name}.vasset"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "audio bus", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_create_prefab(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_prefab_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("prefabs/{name}.vscene"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "prefab", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_get_settings_summary(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let build_path = project.root.join(&project.manifest.build_config);
        let build = std::fs::read_to_string(&build_path)
            .ok()
            .and_then(|text| toml::from_str::<engine_ecs::BuildConfiguration>(&text).ok());

        Ok(serde_json::json!({
            "project": {
                "name": project.manifest.name.clone(),
                "root": project.root.to_string_lossy(),
                "asset_root": project.manifest.asset_root.clone(),
                "default_scene": project.manifest.default_scene.clone(),
                "script_roots": project.manifest.script_roots.clone(),
                "build_config": project.manifest.build_config.clone(),
            },
            "build": build.map(|build| serde_json::json!({
                "target": build.target,
                "release": build.release,
                "features": build.features,
                "render": {
                    "quality": build.render.quality,
                    "upscaler": build.render.upscaler,
                    "dynamic_resolution": build.render.dynamic_resolution,
                    "target_fps": build.render.target_fps,
                    "min_render_scale_percent": build.render.min_render_scale_percent,
                    "max_render_scale_percent": build.render.max_render_scale_percent,
                    "sharpness_percent": build.render.sharpness_percent,
                    "anti_aliasing": build.render.anti_aliasing,
                }
            })),
        }))
    }

    pub(crate) fn project_version_control_status(
        &mut self,
        _params: &Value,
    ) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let output = Command::new("git")
            .arg("-C")
            .arg(&project.root)
            .arg("status")
            .arg("--porcelain=v1")
            .output();

        let Ok(output) = output else {
            return Ok(serde_json::json!({
                "available": false,
                "branch": null,
                "entries": [],
            }));
        };

        let branch = Command::new("git")
            .arg("-C")
            .arg(&project.root)
            .arg("branch")
            .arg("--show-current")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|branch| branch.trim().to_owned())
            .filter(|branch| !branch.is_empty());

        let stdout = String::from_utf8_lossy(&output.stdout);
        let entries = stdout
            .lines()
            .filter_map(|line| {
                if line.len() < 4 {
                    return None;
                }
                let status = line[..2].trim();
                let path = line[3..].to_owned();
                Some(serde_json::json!({
                    "status": if status.is_empty() { "modified" } else { status },
                    "path": path,
                }))
            })
            .collect::<Vec<_>>();

        Ok(serde_json::json!({
            "available": output.status.success(),
            "branch": branch,
            "entries": entries,
        }))
    }

    pub(crate) fn project_create_scene(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_scene_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("scenes/{name}.vscene"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "scene", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    pub(crate) fn project_list_asset_references(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        project.rescan_assets()?;

        let asset_path = normalize_relative_path(path_str)?;
        let guid = project.database.guid_for_path(&asset_path).ok();
        let mut rows = Vec::new();

        if let Some(guid) = guid {
            for dependency in project.database.dependencies().dependencies(guid) {
                rows.push(asset_reference_row(
                    "dependency",
                    "Asset dependency",
                    resolve_asset_reference_label(project, dependency),
                ));
            }
            for dependent in project.database.dependencies().dependents(guid) {
                rows.push(asset_reference_row(
                    "dependent",
                    "Used by asset",
                    resolve_asset_reference_label(project, dependent),
                ));
            }
        }

        for (_entity, object) in project.scene.objects() {
            for component in &object.components {
                if let Some(guid) = guid {
                    collect_component_asset_references(&mut rows, &object.name, component, guid);
                }
                if let engine_ecs::ComponentData::Script(script) = component {
                    if script.source == path_str {
                        rows.push(asset_reference_row(
                            "scene",
                            "Script component",
                            format!("{} -> {}", object.name, script.source),
                        ));
                    }
                }
            }
            for script in &object.scripts {
                if script.source == path_str {
                    rows.push(asset_reference_row(
                        "scene",
                        "Legacy script",
                        format!("{} -> {}", object.name, script.source),
                    ));
                }
            }
        }

        rows.sort_by(|left, right| {
            left["kind"]
                .as_str()
                .cmp(&right["kind"].as_str())
                .then_with(|| left["label"].as_str().cmp(&right["label"].as_str()))
                .then_with(|| left["detail"].as_str().cmp(&right["detail"].as_str()))
        });
        rows.dedup();

        Ok(serde_json::json!({
            "guid": guid.map(|guid| guid.to_string()),
            "path": asset_path.to_string_lossy(),
            "references": rows,
        }))
    }

    pub(crate) fn project_rename_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let old_path_str = params
            .get("old_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'old_path'"))?;
        let new_name = params
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'new_name'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        validate_file_name(new_name)?;
        let asset_root = project.root.join(&project.manifest.asset_root);
        let old_path = resolve_existing_relative_path(&asset_root, old_path_str)?;
        let parent = old_path
            .parent()
            .ok_or_else(|| EngineError::config("cannot rename root directory"))?;
        let ext = old_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let new_path = parent.join(format!("{}{}", new_name, ext));
        let canonical_asset_root =
            asset_root
                .canonicalize()
                .map_err(|source| EngineError::Filesystem {
                    path: asset_root.clone(),
                    source,
                })?;
        if !new_path.starts_with(&canonical_asset_root) {
            return Err(EngineError::config("path is outside the project"));
        }

        std::fs::rename(&old_path, &new_path).map_err(|source| EngineError::Filesystem {
            path: old_path.clone(),
            source,
        })?;

        // Also rename the .meta file if it exists
        let old_meta = asset_meta_path_for_source(&old_path);
        if old_meta.exists() {
            let new_meta = asset_meta_path_for_source(&new_path);
            std::fs::rename(&old_meta, &new_meta).ok();
        }

        // Rescan to update the database
        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: Some(new_path.clone()),
                line: None,
            },
            message: format!("Renamed asset: {} → {}", old_path_str, new_path.display()),
        });

        Ok(serde_json::json!({ "new_path": new_path.to_string_lossy() }))
    }

    pub(crate) fn project_delete_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let asset_root = project.root.join(&project.manifest.asset_root);
        let path = resolve_existing_relative_path(&asset_root, path_str)?;

        // Delete the file
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
        } else {
            std::fs::remove_file(&path).map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
            // Also delete the .meta file
            let meta_path = asset_meta_path_for_source(&path);
            if meta_path.exists() {
                std::fs::remove_file(&meta_path).ok();
            }
        }

        // Rescan to update the database
        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Deleted asset: {path_str}"),
        });

        Ok(serde_json::json!({ "deleted": true }))
    }

    pub(crate) fn project_reimport_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let reimport_all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        if reimport_all {
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };

            let asset_root = project.root.join(&project.manifest.asset_root);
            let mut stack = vec![asset_root.clone()];
            while let Some(path) = stack.pop() {
                let entries = match std::fs::read_dir(&path) {
                    Ok(entries) => entries,
                    Err(source) => {
                        return Err(EngineError::Filesystem { path, source });
                    }
                };
                for entry in entries {
                    let entry = entry.map_err(|source| EngineError::Filesystem {
                        path: asset_root.clone(),
                        source,
                    })?;
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        stack.push(entry_path);
                    } else if entry_path.extension().is_some_and(|ext| ext == "meta") {
                        std::fs::remove_file(&entry_path).ok();
                    }
                }
            }

            project.rescan_assets()?;
            self.console.push(engine_editor::ConsoleEntry {
                timestamp: timestamp_now(),
                level: engine_editor::ConsoleLevel::Info,
                source: engine_editor::ConsoleSource {
                    subsystem: "editor".into(),
                    file: None,
                    line: None,
                },
                message: "Reimported all assets".into(),
            });

            return Ok(serde_json::json!({ "reimported": true }));
        }

        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        // Delete existing meta file to force reimport
        let asset_root = project.root.join(&project.manifest.asset_root);
        let path = resolve_existing_relative_path(&asset_root, path_str)?;
        let meta_path = asset_meta_path_for_source(&path);
        if meta_path.exists() {
            std::fs::remove_file(&meta_path).ok();
        }

        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Reimported asset: {path_str}"),
        });

        Ok(serde_json::json!({ "reimported": true }))
    }

    pub(crate) fn project_read_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let asset_root = project.root.join(&project.manifest.asset_root);
        let full_path = resolve_existing_relative_path(&asset_root, path_str)?;

        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;

        Ok(serde_json::json!({ "content": content }))
    }

    pub(crate) fn project_read_project_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let full_path = resolve_existing_relative_path(&project.root, path_str)?;
        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;

        Ok(serde_json::json!({ "content": content }))
    }

    pub(crate) fn project_write_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'content'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let full_path = resolve_writable_project_source_path(project, path_str)?;

        let extension = full_path
            .extension()
            .and_then(|extension| extension.to_str());
        if matches!(extension, Some("varg" | "vscene" | "vasset")) {
            let diagnostics = engine_script_varg::diagnose_source(&full_path, content);
            if !diagnostics.is_empty() {
                return Err(EngineError::config(format_varg_diagnostics(
                    path_str,
                    &diagnostics,
                )));
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        std::fs::write(&full_path, content).map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

        Ok(serde_json::json!({ "saved": true }))
    }

    pub(crate) fn project_write_project_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'content'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let full_path = resolve_writable_project_source_path(project, path_str)?;
        let extension = full_path
            .extension()
            .and_then(|extension| extension.to_str());
        if matches!(extension, Some("varg" | "vscene" | "vasset")) {
            let diagnostics = engine_script_varg::diagnose_source(&full_path, content);
            if !diagnostics.is_empty() {
                return Err(EngineError::config(format_varg_diagnostics(
                    path_str,
                    &diagnostics,
                )));
            }
        }

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&full_path, content).map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

        project.rescan_assets()?;

        Ok(serde_json::json!({ "saved": true }))
    }

    pub(crate) fn project_check_script(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'source'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let full_path = resolve_writable_project_source_path(project, path_str)?;
        let extension = full_path
            .extension()
            .and_then(|extension| extension.to_str());
        let (diagnostics, ast) = if matches!(extension, Some("varg" | "vscene" | "vasset")) {
            let (ast, diagnostics) = engine_script_varg::parse_source(&full_path, source);
            let diagnostics = diagnostics
                .into_iter()
                .map(|diagnostic| {
                    serde_json::json!({
                        "code": diagnostic.code,
                        "severity": match diagnostic.severity {
                            engine_script_varg::VargDiagnosticSeverity::Error => "error",
                            engine_script_varg::VargDiagnosticSeverity::Warning => "warning",
                        },
                        "line": diagnostic.line,
                        "column": diagnostic.column,
                        "message": diagnostic.message,
                        "suggestion": diagnostic.suggestion,
                        "source_line": diagnostic.source_line,
                    })
                })
                .collect::<Vec<_>>();
            let ast = ast
                .map(|ast| serde_json::to_value(ast).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            (diagnostics, ast)
        } else {
            (
                vec![serde_json::json!({
                    "code": "VARG0000",
                    "severity": "error",
                    "line": null,
                    "column": null,
                    "message": "unsupported script file extension",
                    "suggestion": "Use .varg for runtime scripts, .vscene for scenes, or .vasset for assets.",
                    "source_line": null,
                })],
                serde_json::Value::Null,
            )
        };
        Ok(serde_json::json!({
            "valid": diagnostics.is_empty(),
            "diagnostics": diagnostics,
            "ast": ast,
        }))
    }

    pub(crate) fn project_package(&mut self, params: &Value) -> EngineResult<Value> {
        let target = params
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("native");
        let format = params
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("folder");
        let channel = params
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("release");
        let optimize_assets = params
            .get("optimize_assets")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_debug_symbols = params
            .get("include_debug_symbols")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let project_root = {
            let Some(project) = self.shell.project() else {
                return Err(EngineError::config("no project open"));
            };
            project.root.clone()
        };

        if self
            .shell
            .project()
            .is_some_and(|project| project.scene_dirty)
        {
            self.shell_save_scene(&serde_json::json!({}))?;
        }

        let output = package_project(&PackageRequest {
            project: project_root,
            repo_root: varg_repo_root(),
            target: PackageTarget::parse(target)?,
            format: PackageFormat::parse(format)?,
            channel: PackageChannel::parse(channel)?,
            optimize_assets,
            include_debug_symbols,
            output_dir: None,
        })?;

        self.console.push(ConsoleEntry {
            timestamp: timestamp_now(),
            level: ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "build".to_owned(),
                file: None,
                line: None,
            },
            message: format!(
                "Packaged {} for {}/{} at {}",
                output.project,
                output.target,
                output.channel,
                output.path.display()
            ),
        });

        Ok(serde_json::json!({
            "project": output.project,
            "target": output.target,
            "format": output.format,
            "channel": output.channel,
            "path": output.path.to_string_lossy(),
            "binary": output.binary.map(|path| path.to_string_lossy().to_string()),
            "launcher": output.launcher.map(|path| path.to_string_lossy().to_string()),
            "assets_manifest": output.assets_manifest.to_string_lossy(),
            "asset_count": output.asset_count,
        }))
    }

    // ── Console handlers ──
}

fn resolve_writable_project_source_path(
    project: &engine_editor::ProjectContext,
    path: &str,
) -> EngineResult<PathBuf> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str());
    if matches!(extension, Some("varg")) {
        resolve_writable_relative_path(&project.root, path)
    } else {
        let asset_root = project.root.join(&project.manifest.asset_root);
        resolve_writable_relative_path(&asset_root, path)
    }
}
