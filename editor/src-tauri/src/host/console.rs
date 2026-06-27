use crate::*;

impl EditorHost {
    pub(crate) fn performance_get_snapshot(&mut self, _params: &Value) -> EngineResult<Value> {
        let project = self.shell.project();
        let (entity_count, component_count) = project
            .map(|project| {
                project.scene.objects().iter().fold(
                    (0usize, 0usize),
                    |(objects, components), (_, object)| {
                        (objects + 1, components + object.components.len())
                    },
                )
            })
            .unwrap_or((0, 0));
        let asset_count = project
            .map(|project| project.sorted_assets().len())
            .unwrap_or(0);
        let problem_count = self
            .console
            .entries()
            .iter()
            .filter(|entry| matches!(entry.level, ConsoleLevel::Warn | ConsoleLevel::Error))
            .count();

        Ok(serde_json::json!({
            "entity_count": entity_count,
            "component_count": component_count,
            "asset_count": asset_count,
            "problem_count": problem_count,
            "scene_version": self.scene_version,
            "play_version": self.play_version,
            "playing": self.play_runtime.is_some(),
        }))
    }

    pub(crate) fn console_get_entries(&mut self, _params: &Value) -> EngineResult<Value> {
        let entries: Vec<Value> = self
            .console
            .entries()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "timestamp": e.timestamp,
                    "level": format!("{:?}", e.level).to_lowercase(),
                    "subsystem": e.source.subsystem,
                    "file": e.source.file.as_ref().map(|f| f.to_string_lossy()),
                    "line": e.source.line,
                    "message": e.message,
                })
            })
            .collect();
        Ok(serde_json::json!({ "entries": entries }))
    }

    pub(crate) fn console_clear(&mut self, _params: &Value) -> EngineResult<Value> {
        self.console.clear();
        Ok(serde_json::json!({}))
    }

    pub(crate) fn console_push_entry(&mut self, params: &Value) -> EngineResult<Value> {
        let level = match params
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
        {
            "trace" => ConsoleLevel::Trace,
            "debug" => ConsoleLevel::Debug,
            "warn" => ConsoleLevel::Warn,
            "error" => ConsoleLevel::Error,
            _ => ConsoleLevel::Info,
        };
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let subsystem = params
            .get("subsystem")
            .and_then(|v| v.as_str())
            .unwrap_or("editor")
            .to_owned();
        self.console.push(ConsoleEntry {
            timestamp: timestamp_now(),
            level,
            source: engine_editor::ConsoleSource {
                subsystem,
                file: params
                    .get("file")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from),
                line: params
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|l| l as u32),
            },
            message,
        });
        Ok(serde_json::json!({}))
    }
}
