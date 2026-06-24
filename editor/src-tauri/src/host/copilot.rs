use crate::*;

impl EditorHost {
    pub(crate) fn build_agent_context(
        &self,
        scene: engine_ecs::Scene,
    ) -> EngineResult<engine_editor::ProjectContext> {
        use engine_assets::AssetDatabase;

        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;

        let manifest = project.manifest.clone();
        let asset_root = project.root.join(&project.manifest.asset_root);
        let builtin_root = project.root.join("builtin");
        let database = AssetDatabase::new(asset_root, builtin_root);

        Ok(engine_editor::ProjectContext {
            scene,
            manifest,
            database,
            registry: engine_assets::AssetRegistry::default(),
            assets: Vec::new(),
            asset_imports: Vec::new(),
            scene_dirty: false,
            root: project.root.clone(),
            scene_path: project.scene_path.clone(),
        })
    }

    pub(crate) fn scene_clone_for_agent(&self) -> EngineResult<engine_ecs::Scene> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        // Round-trip clone via JSON
        let scene_json = project.scene.to_json(project.name())?;
        engine_ecs::Scene::from_json(&scene_json)
    }

    pub(crate) fn get_copilot_settings(&self, _params: &Value) -> EngineResult<Value> {
        let mut value = serde_json::to_value(&self.copilot_settings).unwrap_or_default();
        if matches!(
            self.copilot_settings.provider,
            engine_editor::CopilotProvider::Stub
        ) {
            value["model"] = serde_json::json!("none");
        }
        value["has_api_key"] = serde_json::json!(self.copilot_settings.api_key.is_some());
        Ok(value)
    }

    pub(crate) fn update_copilot_settings(&mut self, params: &Value) -> EngineResult<Value> {
        let mut settings: engine_editor::CopilotSettings =
            serde_json::from_value(params.clone())
                .map_err(|e| EngineError::config(format!("invalid copilot settings: {e}")))?;

        // Preserve existing API key when not explicitly provided in the request
        if !params
            .as_object()
            .map_or(false, |m| m.contains_key("api_key"))
        {
            settings.api_key = self.copilot_settings.api_key.clone();
        }
        if !params
            .as_object()
            .map_or(false, |m| m.contains_key("allowed_commands"))
        {
            settings.allowed_commands = self.copilot_settings.allowed_commands.clone();
        }
        if !settings.provider.endpoint_configurable() {
            settings.api_endpoint = None;
        }
        if !matches!(
            settings.provider,
            engine_editor::CopilotProvider::Anthropic
                | engine_editor::CopilotProvider::OpenAI
                | engine_editor::CopilotProvider::Gemini
                | engine_editor::CopilotProvider::Custom
                | engine_editor::CopilotProvider::Mimo
                | engine_editor::CopilotProvider::DeepSeek
                | engine_editor::CopilotProvider::Glm
        ) {
            settings.api_key = None;
        }
        if matches!(settings.provider, engine_editor::CopilotProvider::Stub) {
            settings.model = "none".to_owned();
        }

        // Persist non-secret settings into durable state
        let mut settings_for_state = settings.clone();
        settings_for_state.api_key = None; // Never store key in main state file
        self.durable_state.preferences.copilot = settings_for_state;
        self.sync_durable_state();

        self.copilot_settings = settings;
        self.persist_credentials()?;
        Ok(Value::Null)
    }

    pub(crate) fn copilot_allow_command(&mut self, params: &Value) -> EngineResult<Value> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| EngineError::config("missing 'command'"))?;

        if !self
            .copilot_settings
            .allowed_commands
            .iter()
            .any(|allowed| allowed == command)
        {
            self.copilot_settings
                .allowed_commands
                .push(command.to_owned());
            self.copilot_settings.allowed_commands.sort();
            self.copilot_settings.allowed_commands.dedup();

            let mut settings_for_state = self.copilot_settings.clone();
            settings_for_state.api_key = None;
            self.durable_state.preferences.copilot = settings_for_state;
            self.sync_durable_state();
        }

        Ok(serde_json::json!({ "allowed": true, "command": command }))
    }

    pub(crate) fn persist_credentials(&self) -> EngineResult<()> {
        let Some(path) = self
            .store
            .path()
            .parent()
            .map(|parent| parent.join("credentials.toml"))
        else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let text = toml::to_string_pretty(&CredentialsFile {
            copilot_api_key: self.copilot_settings.api_key.clone(),
            codex_oauth: self.codex_oauth.clone(),
        })
        .map_err(|error| EngineError::other(format!("failed to encode credentials: {error}")))?;
        std::fs::write(&path, text).map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |source| EngineError::Filesystem {
                    path: path.clone(),
                    source,
                },
            )?;
        }
        Ok(())
    }

    pub(crate) fn codex_oauth_status(&self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "connected": self.codex_oauth.is_some(),
            "account_id": self.codex_oauth.as_ref().and_then(|auth| auth.account_id.as_deref()),
        }))
    }

    pub(crate) fn codex_oauth_start(&mut self, _params: &Value) -> EngineResult<Value> {
        self.pending_codex_oauth = None;
        let listener = TcpListener::bind(CODEX_OAUTH_CALLBACK_BIND).map_err(|error| {
            EngineError::other(format!(
                "failed to listen for Codex OAuth callback on {CODEX_OAUTH_CALLBACK_BIND}: {error}"
            ))
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            EngineError::other(format!("failed to configure Codex OAuth callback: {error}"))
        })?;
        let code_verifier = random_urlsafe_string(64)?;
        let code_challenge = codex_pkce_challenge(&code_verifier);
        let state = random_hex_string(16)?;
        let url = codex_authorize_url(&code_challenge, &state);
        let pending = PendingCodexOAuth {
            state,
            code_verifier,
            listener,
        };
        let result = serde_json::json!({
            "url": url,
            "user_code": Value::Null,
            "interval_seconds": 1,
            "method": "browser",
        });
        self.pending_codex_oauth = Some(pending);
        Ok(result)
    }

    pub(crate) fn codex_oauth_poll(&mut self, _params: &Value) -> EngineResult<Value> {
        let code = {
            let pending = self.pending_codex_oauth.as_mut().ok_or_else(|| {
                EngineError::config("no Codex authorization is currently pending")
            })?;
            loop {
                match pending.listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some(code) = read_codex_oauth_callback(&mut stream, &pending.state)?
                        {
                            break code;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        return Ok(serde_json::json!({ "status": "pending" }));
                    }
                    Err(error) => {
                        return Err(EngineError::other(format!(
                            "Codex authorization callback failed: {error}"
                        )));
                    }
                }
            }
        };
        let code_verifier = self
            .pending_codex_oauth
            .as_ref()
            .map(|pending| pending.code_verifier.clone())
            .ok_or_else(|| EngineError::config("no Codex authorization is currently pending"))?;

        let tokens = exchange_codex_token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", CODEX_OAUTH_REDIRECT_URI),
            ("client_id", CODEX_OAUTH_CLIENT_ID),
            ("code_verifier", &code_verifier),
        ])?;
        self.codex_oauth = Some(codex_credential_from_tokens(tokens));
        self.pending_codex_oauth = None;
        self.persist_credentials()?;
        Ok(serde_json::json!({ "status": "connected" }))
    }

    pub(crate) fn codex_oauth_logout(&mut self, _params: &Value) -> EngineResult<Value> {
        self.codex_oauth = None;
        self.pending_codex_oauth = None;
        self.persist_credentials()?;
        Ok(serde_json::json!({ "connected": false }))
    }

    pub(crate) fn ensure_codex_oauth(
        &mut self,
    ) -> EngineResult<engine_ai::providers::CodexOAuthCredentials> {
        let needs_refresh = self
            .codex_oauth
            .as_ref()
            .map(|auth| auth.expires_at_ms <= unix_time_ms().saturating_add(60_000))
            .unwrap_or(false);
        if needs_refresh {
            let refresh_token = self
                .codex_oauth
                .as_ref()
                .map(|auth| auth.refresh_token.clone())
                .unwrap_or_default();
            let tokens = exchange_codex_token(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
                ("client_id", CODEX_OAUTH_CLIENT_ID),
            ])?;
            self.codex_oauth = Some(codex_credential_from_tokens(tokens));
            self.persist_credentials()?;
        }
        let auth = self.codex_oauth.as_ref().ok_or_else(|| {
            EngineError::config("Codex OAuth is not connected. Sign in with ChatGPT first.")
        })?;
        Ok(engine_ai::providers::CodexOAuthCredentials {
            access_token: auth.access_token.clone(),
            account_id: auth.account_id.clone(),
        })
    }

    pub(crate) fn detect_models(&self, params: &Value) -> EngineResult<Value> {
        let provider_str = params
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'provider'"))?;
        let provider_kind = match provider_str {
            "anthropic" => engine_ai::registry::ProviderKind::Anthropic,
            "openai" | "open_a_i" => engine_ai::registry::ProviderKind::OpenAI,
            "codex_oauth" => engine_ai::registry::ProviderKind::CodexOAuth,
            "gemini" => engine_ai::registry::ProviderKind::Gemini,
            "ollama" => engine_ai::registry::ProviderKind::Ollama,
            "custom" => engine_ai::registry::ProviderKind::Custom,
            "mimo" => engine_ai::registry::ProviderKind::Mimo,
            "deepseek" => engine_ai::registry::ProviderKind::DeepSeek,
            "glm" => engine_ai::registry::ProviderKind::Glm,
            other => {
                return Err(EngineError::config(format!(
                    "unknown provider for detection: {other}"
                )));
            }
        };

        let config = model_detection_config(params, &self.copilot_settings, &provider_kind);

        let models = engine_ai::registry::detect_available_models(&provider_kind, &config)?;
        Ok(serde_json::to_value(&models).unwrap_or_default())
    }

    pub(crate) fn get_model_registry(&self, params: &Value) -> EngineResult<Value> {
        let registry = engine_ai::registry::ModelRegistry::new();

        let result = if let Some(provider_str) = params.get("provider").and_then(|v| v.as_str()) {
            let provider_kind = match provider_str {
                "anthropic" => engine_ai::registry::ProviderKind::Anthropic,
                "openai" | "open_a_i" => engine_ai::registry::ProviderKind::OpenAI,
                "codex_oauth" => engine_ai::registry::ProviderKind::CodexOAuth,
                "gemini" => engine_ai::registry::ProviderKind::Gemini,
                "ollama" => engine_ai::registry::ProviderKind::Ollama,
                "custom" => engine_ai::registry::ProviderKind::Custom,
                "mimo" => engine_ai::registry::ProviderKind::Mimo,
                "deepseek" => engine_ai::registry::ProviderKind::DeepSeek,
                "glm" => engine_ai::registry::ProviderKind::Glm,
                _ => {
                    return Ok(serde_json::json!({ "models": [] }));
                }
            };
            let models: Vec<_> = registry.builtin_for(&provider_kind).into_iter().collect();
            serde_json::json!({ "models": models })
        } else {
            // Return all providers and their builtin models
            let all: Vec<_> = engine_ai::registry::ProviderKind::builtin_providers()
                .iter()
                .map(|p| {
                    let models: Vec<_> = registry.builtin_for(p).into_iter().collect();
                    serde_json::json!({
                        "provider": p,
                        "display_name": p.display_name(),
                        "requires_api_key": p.requires_api_key(),
                        "requires_endpoint": p.requires_endpoint(),
                        "endpoint_configurable": p.endpoint_configurable(),
                        "default_endpoint": p.default_endpoint(),
                        "models": models,
                    })
                })
                .collect();
            serde_json::json!({ "providers": all })
        };

        Ok(result)
    }

    pub(crate) fn copilot_plan(&mut self, params: &Value) -> EngineResult<Value> {
        self.copilot_plan_streaming(params, &mut |_| {})
    }

    pub(crate) fn copilot_plan_streaming(
        &mut self,
        params: &Value,
        on_delta: &mut dyn FnMut(engine_ai::AiStreamDelta),
    ) -> EngineResult<Value> {
        let prepared = self.prepare_copilot_request(params)?;
        let model = engine_ai::providers::create_provider(
            &prepared.provider,
            &prepared.model,
            prepared.api_key.as_deref(),
            prepared.endpoint.as_deref(),
            prepared.max_tokens,
            prepared.codex_oauth,
            prepared.mimo_config.as_ref(),
            prepared.glm_config.as_ref(),
        )?;
        let response = model.chat_stream(prepared.request, on_delta)?;
        self.finish_copilot_response_with_tools(
            &prepared.original_prompt,
            &response.content,
            &response.tool_calls,
            prepared.cached_context,
            prepared.knowledge_entries_used,
            prepared.approval_mode,
        )
    }

    pub(crate) fn prepare_copilot_request(
        &mut self,
        params: &Value,
    ) -> EngineResult<PreparedCopilotRequest> {
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'prompt'"))?;

        // Update copilot settings if provided in the request
        if let Some(settings) = params.get("settings") {
            if let Ok(parsed) =
                serde_json::from_value::<engine_editor::CopilotSettings>(settings.clone())
            {
                self.copilot_settings = parsed;
            }
        }

        // Parse thinking_effort from request
        let thinking_effort = params.get("thinking_effort").and_then(|v| {
            let s = v.as_str()?;
            match s {
                "off" => Some(engine_ai::ThinkingEffort::Off),
                "low" => Some(engine_ai::ThinkingEffort::Low),
                "medium" => Some(engine_ai::ThinkingEffort::Medium),
                "high" => Some(engine_ai::ThinkingEffort::High),
                _ => None,
            }
        });

        let selected_knowledge_ids = params
            .get("knowledge_ids")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let attached_knowledge = self.selected_approved_knowledge(&selected_knowledge_ids)?;
        let knowledge_context = format_editor_knowledge_context(&attached_knowledge);
        let editor_context = params
            .get("editor_context")
            .map(|context| {
                format!(
                    "\n\n[Editor Context]\n{}",
                    serde_json::to_string_pretty(context).unwrap_or_default()
                )
            })
            .unwrap_or_default();

        // Build enriched prompt with explicit editor, entity, and Knowledge context.
        let enriched_prompt = if let Some(entity) = params.get("selected_entity") {
            format!(
                "{}{}{}\n\n[Selected Entity Context]\n{}",
                prompt,
                editor_context,
                knowledge_context,
                serde_json::to_string_pretty(entity).unwrap_or_default()
            )
        } else {
            format!("{prompt}{editor_context}{knowledge_context}")
        };

        let scene = self.scene_clone_for_agent()?;
        let ctx = self.build_agent_context(scene)?;

        let session = AgentSession::new(ctx)?;

        // Create the AI model from settings, falling back to a helpful error message
        let provider_str = match self.copilot_settings.provider {
            engine_editor::CopilotProvider::Anthropic => "anthropic",
            engine_editor::CopilotProvider::Ollama => "ollama",
            engine_editor::CopilotProvider::OpenAI => "openai",
            engine_editor::CopilotProvider::CodexOAuth => "codex_oauth",
            engine_editor::CopilotProvider::Gemini => "gemini",
            engine_editor::CopilotProvider::Custom => "custom",
            engine_editor::CopilotProvider::Mimo => "mimo",
            engine_editor::CopilotProvider::DeepSeek => "deepseek",
            engine_editor::CopilotProvider::Glm => "glm",
            engine_editor::CopilotProvider::Stub => {
                return Err(EngineError::config(
                    "Copilot is in stub mode. Go to Settings → Copilot to configure a real provider.",
                ));
            }
        };

        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        Ok(PreparedCopilotRequest {
            request: session.prepare_request(
                &enriched_prompt,
                &self.copilot_conversation,
                thinking_effort,
            ),
            original_prompt: prompt.to_string(),
            provider: provider_str.to_owned(),
            model: self.copilot_settings.model.clone(),
            api_key: self.copilot_settings.api_key.clone(),
            endpoint: if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            },
            max_tokens: self.copilot_settings.max_tokens,
            codex_oauth,
            mimo_config: if provider_str == "mimo" {
                Some(self.copilot_settings.mimo_config.clone())
            } else {
                None
            },
            glm_config: if provider_str == "glm" {
                Some(self.copilot_settings.glm_config.clone())
            } else {
                None
            },
            cached_context: session.context,
            knowledge_entries_used: attached_knowledge.len(),
            approval_mode: CopilotApprovalMode::from_params(params),
        })
    }

    pub(crate) fn selected_approved_knowledge(
        &self,
        selected_ids: &[String],
    ) -> EngineResult<Vec<quest::KnowledgeEntry>> {
        if selected_ids.is_empty() {
            return Ok(Vec::new());
        }
        let entries = self.quest_store.list_knowledge()?;
        let approved_by_id = entries
            .iter()
            .filter(|entry| entry.status == "approved")
            .map(|entry| (entry.id.as_str(), entry))
            .collect::<std::collections::HashMap<_, _>>();
        let mut selected = Vec::new();
        for id in selected_ids {
            if selected
                .iter()
                .any(|entry: &quest::KnowledgeEntry| entry.id == *id)
            {
                continue;
            }
            let entry = approved_by_id.get(id.as_str()).ok_or_else(|| {
                EngineError::config(
                    "Editor AI can only attach approved Knowledge entries to requests",
                )
            })?;
            selected.push((*entry).clone());
        }
        Ok(selected)
    }

    pub(crate) fn finish_copilot_response_with_tools(
        &mut self,
        original_prompt: &str,
        response: &str,
        tool_calls: &[engine_ai::ToolCall],
        cached_context: engine_editor::ProjectContext,
        knowledge_entries_used: usize,
        approval_mode: CopilotApprovalMode,
    ) -> EngineResult<Value> {
        let mut session = AgentSession::new(cached_context)?;
        let planning_policy = approval_mode.planning_policy();

        let mut plan = if !tool_calls.is_empty() {
            session.plan_from_tool_calls(tool_calls, response, planning_policy)?
        } else {
            session.plan_from_response(response, planning_policy)?
        };

        let assistant_message = plan
            .operations
            .iter()
            .find_map(|planned| match &planned.operation {
                engine_ai::AgentOperation::Complete { summary } => summary.clone(),
                _ => None,
            })
            .unwrap_or_default();
        plan.operations.retain(|planned| {
            !matches!(
                &planned.operation,
                engine_ai::AgentOperation::Complete { .. }
            )
        });
        plan.read_only = plan.operations.iter().all(|op| !op.requires_write);
        plan.requires_write = plan.operations.iter().any(|op| op.requires_write);

        let operations: Vec<serde_json::Value> = plan
            .operations
            .iter()
            .enumerate()
            .map(|(i, op)| {
                let command = match &op.operation {
                    engine_ai::AgentOperation::ExecuteCommand { command, .. }
                    | engine_ai::AgentOperation::RunCommand { command, .. } => {
                        Some(command.as_str())
                    }
                    _ => None,
                };
                let permission_kind = if command.is_some() {
                    "command"
                } else if op.requires_write {
                    "write"
                } else {
                    "read"
                };
                let permanently_allowed = command.is_some_and(|command| {
                    self.copilot_settings
                        .allowed_commands
                        .iter()
                        .any(|allowed| allowed == command)
                });
                serde_json::json!({
                    "index": i,
                    "preview": op.preview,
                    "requires_write": op.requires_write,
                    "permission_kind": permission_kind,
                    "requires_approval": if command.is_some() {
                        !permanently_allowed && !approval_mode.auto_approves_command()
                    } else if op.requires_write {
                        !approval_mode.auto_approves_write()
                    } else {
                        false
                    },
                    "command": command,
                    "permanently_allowed": permanently_allowed,
                })
            })
            .collect();

        self.copilot_conversation
            .push(engine_ai::ChatMessage::user(original_prompt));

        let history_message = if assistant_message.is_empty() {
            let plan_summary: Vec<String> = plan
                .operations
                .iter()
                .map(|op| op.preview.clone())
                .collect();
            format!(
                "Proposed {} operation(s):\n{}",
                plan.operations.len(),
                plan_summary.join("\n")
            )
        } else {
            assistant_message.clone()
        };
        self.copilot_conversation
            .push(engine_ai::ChatMessage::assistant(history_message));

        // Trim conversation to prevent unbounded growth
        while self.copilot_conversation.len() > MAX_COPILOT_CONVERSATION_MESSAGES {
            self.copilot_conversation.remove(0);
        }

        self.last_copilot_plan = Some(plan);

        Ok(serde_json::json!({
            "message": assistant_message,
            "operations": operations,
            "read_only": operations.iter().all(|o| !o["requires_write"].as_bool().unwrap_or(true)),
            "requires_write": operations.iter().any(|o| o["requires_write"].as_bool().unwrap_or(false)),
            "knowledge_entries_used": knowledge_entries_used,
        }))
    }

    pub(crate) fn copilot_apply(&mut self, params: &Value) -> EngineResult<Value> {
        let approval_mode = CopilotApprovalMode::from_params(params);
        let approved_indices: Vec<usize> = params
            .get("approved_indices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|i| i as usize)
                    .collect()
            })
            .ok_or_else(|| EngineError::config("missing 'approved_indices' array"))?;

        let plan = self.last_copilot_plan.take().ok_or_else(|| {
            EngineError::config("no pending copilot plan — call copilot/plan first")
        })?;

        // Filter the plan to only approved operations
        let filtered_ops: Vec<_> = plan
            .operations
            .into_iter()
            .enumerate()
            .filter(|(i, _)| approved_indices.contains(i))
            .map(|(_, op)| op)
            .collect();
        let applied_read_only = filtered_ops.iter().all(|op| !op.requires_write);

        if filtered_ops.is_empty() {
            return Ok(serde_json::json!({
                "operations_performed": 0,
                "completed": false,
                "trace_entries": [],
                "console_entries": [],
                "summary": null
            }));
        }

        let before_snapshot = self.scene_snapshot().ok();
        let scene = self.scene_clone_for_agent()?;
        let ctx = self.build_agent_context(scene)?;

        let mut session = AgentSession::new(ctx)?;

        let apply_plan = AgentPlan {
            operations: filtered_ops,
            read_only: false,
            requires_write: true,
            policy: approval_mode.apply_policy(),
        };

        let outcome = session.apply_plan(&apply_plan)?;

        // Write the modified scene back to the real project
        if let Some(project) = self.shell.project_mut() {
            project.scene = session.context.scene;
            project.scene_dirty = true;
            project.asset_imports.extend(session.context.asset_imports);
            for entry in session.console.entries().iter() {
                self.console.push(entry.clone());
            }
        }

        let after_snapshot = self.scene_snapshot().ok();
        let undo_available = if !applied_read_only {
            if let (Some(before), Some(after)) = (before_snapshot, after_snapshot) {
                if before != after {
                    self.shell.push_undo(UndoCommand::new(
                        "AI scoped edit",
                        "copilot",
                        before,
                        after,
                    ));
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        self.bump_scene_version();

        // Record execution results in conversation history so the model has
        // context about what happened when the user follows up.
        let console_results: Vec<String> = session
            .console
            .entries()
            .iter()
            .filter(|entry| entry.source.subsystem == "ai-agent")
            .map(|entry| entry.message.clone())
            .collect();
        let trace_statuses: Vec<String> = outcome
            .trace_entries
            .iter()
            .map(|t| format!("{}: {}", t.tool, t.result))
            .collect();
        let execution_summary = copilot_execution_summary(
            outcome.operations_performed,
            outcome.summary.as_deref(),
            &trace_statuses,
            &console_results,
        );
        self.copilot_conversation
            .push(engine_ai::ChatMessage::assistant(execution_summary));

        // Trim conversation to prevent unbounded growth
        while self.copilot_conversation.len() > MAX_COPILOT_CONVERSATION_MESSAGES {
            self.copilot_conversation.remove(0);
        }

        let trace_entries: Vec<serde_json::Value> = outcome
            .trace_entries
            .iter()
            .map(|t| {
                serde_json::json!({
                    "tool": t.tool,
                    "result": t.result,
                    "recovery_hint": t.recovery_hint,
                })
            })
            .collect();

        let console_entries: Vec<serde_json::Value> = outcome
            .console_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "level": format!("{:?}", e.level).to_lowercase(),
                    "message": e.message,
                    "subsystem": e.source.subsystem,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "operations_performed": outcome.operations_performed,
            "completed": outcome.completed,
            "summary": outcome.summary,
            "trace_entries": trace_entries,
            "console_entries": console_entries,
            "undo_available": undo_available,
            "undo_label": if undo_available { Some("AI scoped edit") } else { None::<&str> },
            "needs_continuation": should_continue_copilot(applied_read_only, outcome.completed),
        }))
    }

    pub(crate) fn copilot_undo_last(&mut self, _params: &Value) -> EngineResult<Value> {
        let applied = self.shell.undo_scene_command()?;
        if applied {
            self.drain_shell_console();
            self.bump_scene_version();
            self.copilot_conversation
                .push(engine_ai::ChatMessage::assistant(
                    "Undid the last AI scoped edit through the editor undo stack.".to_owned(),
                ));
        }
        Ok(serde_json::json!({
            "applied": applied,
            "summary": if applied {
                "Undid the last AI scoped edit."
            } else {
                "No undoable AI scoped edit was available."
            },
            "trace_entries": [{
                "tool": "editor.undo",
                "result": if applied { "applied" } else { "skipped" },
                "recovery_hint": null
            }]
        }))
    }

    pub(crate) fn copilot_clear_conversation(&mut self, _params: &Value) -> EngineResult<Value> {
        self.copilot_conversation.clear();
        self.last_copilot_plan = None;
        Ok(Value::Null)
    }

    pub(crate) fn copilot_get_conversation_length(&self, _params: &Value) -> EngineResult<Value> {
        // Return the number of turns (pairs) in the conversation
        let turns = self.copilot_conversation.len() / 2;
        Ok(serde_json::json!({ "turns": turns, "messages": self.copilot_conversation.len() }))
    }
}
