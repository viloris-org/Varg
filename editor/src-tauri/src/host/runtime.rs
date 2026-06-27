use crate::*;

impl EditorHost {
    // ── Play handlers ──

    pub(crate) fn play_start(&mut self, _params: &Value) -> EngineResult<Value> {
        self.start_play_runtime()?;
        Ok(serde_json::json!({
            "playing": true,
            "play_version": self.play_version,
        }))
    }

    pub(crate) fn play_stop(&mut self, _params: &Value) -> EngineResult<Value> {
        self.stop_play_runtime();
        Ok(serde_json::json!({ "playing": false }))
    }

    pub(crate) fn play_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "playing": self.play_runtime.is_some(),
            "play_version": self.play_version,
        }))
    }

    // ── Helpers ──

    pub(crate) fn sync_durable_state(&mut self) {
        // HubState owns the general editor preferences, while copilot settings are
        // updated through their own RPC. Preserve them when rebuilding durable state.
        let copilot_settings = self.durable_state.preferences.copilot.clone();
        self.durable_state = self.hub.durable_state();
        self.durable_state.preferences.copilot = copilot_settings;
        if let Some(project) = self.shell.project() {
            self.durable_state.last_open_project = Some(project.root.clone());
        }
        self.persist_state();
    }

    pub(crate) fn persist_state(&self) {
        self.store.save(&self.durable_state).ok();
    }

    pub(crate) fn reopen_last_project_if_needed(&mut self) {
        if !self.hub.preferences().reopen_last_project {
            return;
        }
        let Some(path) = self.durable_state.last_open_project.clone() else {
            return;
        };
        if self.shell.open_project(&path).is_ok() {
            self.hub.mark_project_open(path);
            self.drain_shell_console();
        }
    }

    fn create_play_runtime(&self) -> EngineResult<RuntimeServices> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        let mut runtime =
            headless_services_from_scene(config, project.root.clone(), &project.scene)?;
        runtime.enable_default_audio_output();
        runtime.set_script_roots(
            project
                .manifest
                .script_roots
                .iter()
                .map(|root| PathBuf::from(root.as_str())),
        );
        runtime.load_project_assets(project.root.join(&project.manifest.asset_root))?;
        Ok(runtime)
    }

    pub(crate) fn create_game_runtime_snapshot(
        &self,
    ) -> EngineResult<game_window::GameRuntimeSnapshot> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        Ok(game_window::GameRuntimeSnapshot::new(
            config,
            project.root.clone(),
            project
                .manifest
                .script_roots
                .iter()
                .map(|root| PathBuf::from(root.as_str()))
                .collect(),
            project.root.join(&project.manifest.asset_root),
            project.scene.to_scene_file(project.name())?,
        ))
    }

    pub(crate) fn create_scene_runtime_snapshot(
        &self,
    ) -> EngineResult<scene_window::SceneRuntimeSnapshot> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        Ok(scene_window::SceneRuntimeSnapshot::new(
            config,
            project.root.clone(),
            project
                .manifest
                .script_roots
                .iter()
                .map(|root| PathBuf::from(root.as_str()))
                .collect(),
            project.root.join(&project.manifest.asset_root),
            project.scene.to_scene_file(project.name())?,
        ))
    }

    fn start_play_runtime(&mut self) -> EngineResult<()> {
        self.play_runtime = Some(self.create_play_runtime()?);
        self.play_last_frame = Some(Instant::now());
        self.play_version = self.play_version.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn stop_play_runtime(&mut self) {
        self.play_runtime = None;
        self.play_last_frame = None;
        self.play_version = self.play_version.wrapping_add(1);
    }

    pub(crate) fn tick_play_runtime(&mut self) -> EngineResult<()> {
        if self.play_runtime.is_none() {
            self.start_play_runtime()?;
        }
        let now = Instant::now();
        let delta = self
            .play_last_frame
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or_else(|| Duration::from_secs_f32(1.0 / 60.0));
        self.play_last_frame = Some(now);
        if let Some(runtime) = self.play_runtime.as_mut() {
            runtime.tick_game_frame(delta.min(Duration::from_millis(100)), false)?;
            self.play_version = self.play_version.wrapping_add(1);
        }
        Ok(())
    }

    /// Polls events from the native game window and handles close/error.
    pub(crate) fn poll_game_window(&mut self) {
        let Some(gw) = self.game_window.as_ref() else {
            return;
        };

        for event in gw.poll_events() {
            match event {
                game_window::GameEvent::Closed => {
                    tracing::debug!(target: "editor", "game window closed");
                }
                game_window::GameEvent::Error(msg) => {
                    tracing::error!(target: "editor", "game window error: {msg}");
                }
            }
        }
    }

    /// Polls events from the native scene window and handles close/error.
    pub(crate) fn poll_scene_window(&mut self) {
        let Some(scene_window) = self.scene_window.as_ref() else {
            return;
        };

        let mut closed = false;
        for event in scene_window.poll_events() {
            match event {
                scene_window::SceneEvent::Closed => {
                    tracing::debug!(target: "editor", "scene window closed");
                }
                scene_window::SceneEvent::Error(msg) => {
                    tracing::error!(target: "editor", "scene window error: {msg}");
                    closed = true;
                }
            }
        }
        if closed {
            self.scene_window = None;
        }
    }
}
