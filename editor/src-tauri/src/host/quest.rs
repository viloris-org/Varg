use crate::*;

impl EditorHost {
    pub(crate) fn quest_list(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "quests": self.quest_store.list()? }))
    }

    pub(crate) fn quest_get(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        serde_json::to_value(self.quest_store.get(id)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_read_artifact(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let path = required_string(params, "path")?;
        let relative = normalize_relative_path(path)?;
        let root = self.quest_store.quest_path(id)?;
        let full_path = root.join(relative);
        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;
        Ok(serde_json::json!({ "content": content }))
    }

    pub(crate) fn prepare_quest_create_request(
        &mut self,
        params: &Value,
    ) -> EngineResult<PreparedQuestCreateRequest> {
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let goal = required_string(params, "goal")?.trim().to_owned();
        if goal.is_empty() {
            return Err(EngineError::config("Quest goal must not be empty"));
        }
        let mode = parse_quest_mode(params.get("mode"))?;
        let model_config = self.quest_model_config_from_params(params)?;
        let project = {
            let project = self
                .shell
                .project()
                .ok_or_else(|| EngineError::config("no project open"))?;
            QuestProject {
                name: project.name().to_owned(),
                path: project.root.clone(),
            }
        };
        let mut request = engine_ai::AiRequest::single_turn(
            "You are Varg Quest Mode. Create only the initial editable Markdown spec for an AI-led game-editor Quest. Prefer calling `create_or_update_spec` once. If the user's goal is underspecified and the missing choice materially changes the plan, call `ask_questions` to create an interactive question card instead of writing questions in prose. If tool calling is unavailable or awkward, return the editable Markdown spec directly as normal text. Do not create execution tasks yet; tasks are planned later after the user reviews and updates the spec. Do not force a generic workflow; choose the spec shape that best fits the user's goal.".to_owned(),
            serde_json::json!({}),
            format!("Quest goal:\n{goal}"),
        );
        request.tools = quest_creation_tool_definitions();
        let model_request = self.prepare_quest_model_request(&model_config, request)?;
        Ok(PreparedQuestCreateRequest {
            model_request,
            title,
            goal,
            project,
            mode,
            model_config,
        })
    }

    pub(crate) fn finish_quest_create(
        &mut self,
        generated: GeneratedQuestSpec,
        title: String,
        goal: String,
        project: QuestProject,
        mode: QuestMode,
        model_config: QuestModelConfig,
    ) -> EngineResult<Value> {
        let title = if title.is_empty() {
            generated.title
        } else {
            title
        };
        let detail = self.quest_store.create_with_config(
            title,
            goal,
            generated.spec,
            project,
            mode,
            model_config,
        )?;
        let has_question_cards = append_generated_question_cards(
            &self.quest_store,
            &detail.record.id,
            generated.question_cards,
        )?;
        let detail = if has_question_cards {
            self.quest_store
                .transition(&detail.record.id, QuestStatus::Clarifying)?
        } else {
            self.quest_store.get(&detail.record.id)?
        };
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_create_openai_realtime_transcription_session(
        &self,
        _params: &Value,
    ) -> EngineResult<Value> {
        if !matches!(
            self.copilot_settings.provider,
            engine_editor::CopilotProvider::OpenAI
        ) {
            return Err(EngineError::config(
                "Quest voice input requires the OpenAI API provider.",
            ));
        }
        let api_key = self.copilot_settings.api_key.as_deref().ok_or_else(|| {
            EngineError::config("OpenAI API key is required for Quest voice input")
        })?;
        let endpoint = self
            .copilot_settings
            .api_endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/');
        let url = format!("{endpoint}/realtime/client_secrets");
        let body = serde_json::json!({
            "session": {
                "type": "transcription",
                "audio": {
                    "input": {
                        "transcription": {
                            "model": "gpt-realtime-whisper",
                            "delay": "low"
                        }
                    }
                }
            }
        });
        let mut response = ureq::post(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|error| {
                EngineError::other(format!(
                    "OpenAI Realtime transcription session failed: {error}"
                ))
            })?;
        let json: Value = response.body_mut().read_json().map_err(|error| {
            EngineError::other(format!(
                "OpenAI Realtime transcription session response parse failed: {error}"
            ))
        })?;
        Ok(serde_json::json!({
            "session": json,
            "model": "gpt-realtime-whisper",
            "endpoint": endpoint,
            "realtime_url": format!("{endpoint}/realtime/calls"),
        }))
    }

    pub(crate) fn openai_realtime_transcription_config(&self) -> EngineResult<(String, String)> {
        if !matches!(
            self.copilot_settings.provider,
            engine_editor::CopilotProvider::OpenAI
        ) {
            return Err(EngineError::config(
                "Quest voice input requires the OpenAI API provider.",
            ));
        }
        let api_key = self.copilot_settings.api_key.clone().ok_or_else(|| {
            EngineError::config("OpenAI API key is required for Quest voice input")
        })?;
        let endpoint = self
            .copilot_settings
            .api_endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_owned();
        Ok((api_key, endpoint))
    }

    pub(crate) fn prepare_quest_rewrite_request(
        &mut self,
        params: &Value,
    ) -> EngineResult<PreparedQuestModelRequest> {
        let prompt = required_string(params, "prompt")?.trim();
        if prompt.is_empty() {
            return Err(EngineError::config("Prompt must not be empty"));
        }
        let model_config = self.quest_model_config_from_params(params)?;
        let request = engine_ai::AiRequest {
            system: "You rewrite rough Quest prompts into clear, actionable game-engine development tasks. Return only the rewritten prompt. Do not add markdown fences, titles, commentary, or multiple options.".to_owned(),
            context: serde_json::json!({}),
            messages: vec![engine_ai::ChatMessage::user(format!(
                "Rewrite this Quest prompt so an autonomous coding agent can execute it. Preserve the user's intent, concrete nouns, language, and constraints. Make it concise but specific.\n\nPrompt:\n{prompt}"
            ))],
            thinking_effort: parse_thinking_effort(&model_config.thinking_effort),
            tools: Vec::new(),
        };
        self.prepare_quest_model_request(&model_config, request)
    }

    pub(crate) fn finish_quest_rewrite(&mut self, response: String) -> EngineResult<Value> {
        let rewritten = response.trim().trim_matches('"').trim().to_owned();
        if rewritten.is_empty() {
            return Err(EngineError::other(
                "Prompt rewrite returned an empty result",
            ));
        }
        Ok(serde_json::json!({ "prompt": rewritten }))
    }

    pub(crate) fn quest_promote(&mut self, params: &Value) -> EngineResult<Value> {
        let prompt = required_string(params, "prompt")?.trim();
        if prompt.is_empty() {
            return Err(EngineError::config(
                "Promoted Quest prompt must not be empty",
            ));
        }
        let context = params
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let goal = if context.is_empty() {
            prompt.to_owned()
        } else {
            format!("{prompt}\n\nPromoted Editor context:\n{context}")
        };
        let generated = self.generate_quest_spec(&goal)?;
        let model_config = self.default_quest_model_config();
        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;
        let detail = self.quest_store.create_with_config(
            generated.title,
            goal.clone(),
            generated.spec,
            QuestProject {
                name: project.name().to_owned(),
                path: project.root.clone(),
            },
            QuestMode::Solo,
            model_config,
        )?;
        if !context.is_empty() {
            let promoted_intent = format!(
                "# {}\n\n## Goal\n\n{}\n\n## Promoted Editor Context\n\n{}\n",
                detail.record.title, prompt, context
            );
            self.quest_store
                .update_intent(&detail.record.id, &promoted_intent)?;
            self.quest_store.append_timeline_event(
                &detail.record.id,
                "context_attached",
                "Promoted Editor context into Quest intent",
                serde_json::json!({ "context_bytes": context.len() }),
            )?;
        }
        for task in generated.tasks {
            self.quest_store.append_timeline_event(
                &detail.record.id,
                "task_created",
                &task.title,
                serde_json::json!({
                    "summary": task.summary,
                    "acceptance": task.acceptance,
                    "source": "promoted_editor_context",
                }),
            )?;
        }
        let has_question_cards = append_generated_question_cards(
            &self.quest_store,
            &detail.record.id,
            generated.question_cards,
        )?;
        let detail = if has_question_cards {
            self.quest_store
                .transition(&detail.record.id, QuestStatus::Clarifying)?
        } else {
            self.quest_store.get(&detail.record.id)?
        };
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn generate_quest_spec(&mut self, goal: &str) -> EngineResult<GeneratedQuestSpec> {
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
                    "Quest creation requires a configured AI provider because the Quest spec and execution plan are AI-generated. Go to Settings → Copilot to configure an API key, OAuth provider, Ollama, or a custom endpoint.",
                ));
            }
        };
        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        let model = engine_ai::providers::create_provider(
            provider_str,
            &self.copilot_settings.model,
            self.copilot_settings.api_key.as_deref(),
            if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.as_deref()
            } else {
                None
            },
            self.copilot_settings.max_tokens,
            codex_oauth,
            if provider_str == "mimo" {
                Some(&self.copilot_settings.mimo_config)
            } else {
                None
            },
            if provider_str == "glm" {
                Some(&self.copilot_settings.glm_config)
            } else {
                None
            },
        )?;
        let mut request = engine_ai::AiRequest::single_turn(
            "You are Varg Quest Mode. Create only the initial editable Markdown spec for an AI-led game-editor Quest. Prefer calling `create_or_update_spec` once. If the user's goal is underspecified and the missing choice materially changes the plan, call `ask_questions` to create an interactive question card instead of writing questions in prose. If tool calling is unavailable or awkward, return the editable Markdown spec directly as normal text. Do not create execution tasks yet; tasks are planned later after the user reviews and updates the spec. Do not force a generic workflow; choose the spec shape that best fits the user's goal.".to_owned(),
            serde_json::json!({}),
            format!("Quest goal:\n{goal}"),
        );
        request.tools = quest_creation_tool_definitions();
        let response = model.chat(request)?;
        parse_generated_quest_response(&response.tool_calls, &response.content, goal)
    }

    pub(crate) fn quest_update_spec(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let spec = required_string(params, "spec")?;
        serde_json::to_value(self.quest_store.update_spec(id, spec)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_update_tasks(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let tasks_value = params
            .get("tasks")
            .cloned()
            .ok_or_else(|| EngineError::config("missing 'tasks' parameter"))?;
        let tasks: Vec<QuestTask> = serde_json::from_value(tasks_value)
            .map_err(|error| EngineError::config(format!("invalid Quest tasks: {error}")))?;
        serde_json::to_value(self.quest_store.replace_tasks(id, tasks)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_update_execution_config(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let mode = parse_quest_mode(params.get("mode"))?;
        let model_config = self.quest_model_config_from_params(params)?;
        let autonomy = params
            .get("autonomy")
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|error| {
                    EngineError::config(format!("invalid Quest autonomy config: {error}"))
                })
            })
            .transpose()?;
        serde_json::to_value(self.quest_store.update_execution_config(
            id,
            mode,
            model_config,
            autonomy,
        )?)
        .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_update_knowledge_context(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let knowledge_ids = params
            .get("knowledge_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| EngineError::config("missing 'knowledge_ids'"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        serde_json::to_value(
            self.quest_store
                .update_knowledge_context(id, knowledge_ids)?,
        )
        .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_update_intent(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let intent = required_string(params, "intent")?;
        serde_json::to_value(self.quest_store.update_intent(id, intent)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_add_note(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("message");
        let message = required_string(params, "message")?;
        serde_json::to_value(self.quest_store.add_user_note(id, kind, message)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_request_quick_fix(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let issue = required_string(params, "issue")?;
        serde_json::to_value(self.quest_store.request_quick_fix(id, issue)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_rename(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let title = required_string(params, "title")?;
        serde_json::to_value(self.quest_store.rename(id, title)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_branch(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let title = params.get("title").and_then(Value::as_str);
        serde_json::to_value(self.quest_store.branch(id, title)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_transition(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let status: QuestStatus = serde_json::from_value(
            params
                .get("status")
                .cloned()
                .ok_or_else(|| EngineError::config("missing 'status'"))?,
        )
        .map_err(|error| EngineError::config(error.to_string()))?;
        serde_json::to_value(self.quest_store.transition(id, status)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_delete(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        self.quest_store.delete(id)?;
        Ok(serde_json::json!({ "deleted": true }))
    }

    pub(crate) fn knowledge_list(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "entries": self.quest_store.list_knowledge()? }))
    }

    pub(crate) fn knowledge_propose(&mut self, params: &Value) -> EngineResult<Value> {
        let category = required_string(params, "category")?;
        let content = required_string(params, "content")?;
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("manual");
        Ok(serde_json::json!({
            "entries": self.quest_store.propose_knowledge(category, content, source)?
        }))
    }

    pub(crate) fn knowledge_approve(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.approve_knowledge(id)? }))
    }

    pub(crate) fn knowledge_reject(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.reject_knowledge(id)? }))
    }

    pub(crate) fn knowledge_revalidate(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "entries": self.quest_store.revalidate_knowledge()? }))
    }

    pub(crate) fn knowledge_remove(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.remove_knowledge(id)? }))
    }

    pub(crate) fn quest_execute(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?.to_owned();
        let started_at = Instant::now();
        let prepared = self.prepare_quest_execution(&id)?;
        match run_quest_execution(prepared) {
            Ok(value) => Ok(value),
            Err(error) => record_quest_execution_failure(&self.quest_store, &id, started_at, error),
        }
    }

    pub(crate) fn prepare_quest_execution(
        &mut self,
        id: &str,
    ) -> EngineResult<PreparedQuestExecution> {
        let detail = self.quest_store.get(id)?;
        let model_provider = self.prepare_quest_model_request(
            &detail.record.model_config,
            engine_ai::AiRequest::single_turn(String::new(), serde_json::json!({}), String::new()),
        )?;
        Ok(PreparedQuestExecution {
            quest_store: self.quest_store.clone(),
            quest_id: id.to_owned(),
            model_provider,
        })
    }

    pub(crate) fn quest_mock_execute(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        serde_json::to_value(self.quest_store.mock_execute(id)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_cancel(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Canceled Quest");
        serde_json::to_value(self.quest_store.cancel(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_reopen(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Reopened Quest");
        serde_json::to_value(self.quest_store.reopen(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_continue(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Continue Quest from current evidence");
        serde_json::to_value(self.quest_store.continue_quest(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_apply(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        if detail.record.status != QuestStatus::ReadyForReview {
            return Err(EngineError::config("Quest must be in review before apply"));
        }
        let review = detail
            .record
            .review
            .as_ref()
            .ok_or_else(|| EngineError::config("Quest has no review bundle"))?;
        ensure_review_project_is_current(review, &detail.record.project.path)?;
        let workspace_id = detail
            .record
            .workspace_id
            .as_deref()
            .ok_or_else(|| EngineError::config("Quest has no workspace"))?;
        let workspace_root = self
            .quest_store
            .quest_path(id)?
            .join("workspaces")
            .join(workspace_id);
        if !workspace_root.is_dir() {
            return Err(EngineError::config("Quest workspace is missing"));
        }

        let changed_files = selected_review_paths_from_params(review, params, "apply")?;
        let selected_paths: std::collections::HashSet<&str> =
            changed_files.iter().map(String::as_str).collect();
        let selected_review_files = review
            .changed_files
            .iter()
            .filter(|file| selected_paths.contains(file.path.as_str()))
            .collect::<Vec<_>>();
        if selected_review_files.len() != changed_files.len() {
            return Err(EngineError::config(
                "selected Quest file is not present in the review bundle",
            ));
        }

        let project_root = detail.record.project.path.clone();
        let mut applied = Vec::new();
        let rollback_id = format!("rollback-{}", unix_time_ms());
        let rollback_root = self
            .quest_store
            .quest_path(id)?
            .join("rollbacks")
            .join(&rollback_id);
        for file in selected_review_files {
            let relative = normalize_relative_path(&file.path)?;
            let source = workspace_root.join(&relative);
            let destination = project_root.join(&relative);
            snapshot_rollback_file(&rollback_root, &relative, &destination)?;
            if file.status == "deleted" {
                if destination.exists() {
                    std::fs::remove_file(&destination).map_err(|source| {
                        EngineError::Filesystem {
                            path: destination.clone(),
                            source,
                        }
                    })?;
                }
            } else {
                if !source.is_file() {
                    return Err(EngineError::config(format!(
                        "changed file is missing from Quest workspace: {}",
                        file.path
                    )));
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                std::fs::copy(&source, &destination).map_err(|source| EngineError::Filesystem {
                    path: destination.clone(),
                    source,
                })?;
            }
            applied.push(file.path.clone());
        }

        let total_changed = review.changed_files.len();
        let partial = applied.len() < total_changed;
        let summary = if partial {
            format!(
                "Partially applied {} of {} reviewed Quest file(s)",
                applied.len(),
                total_changed
            )
        } else {
            "Applied reviewed Quest bundle to active project".to_owned()
        };
        let applied_paths = applied
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let _ = self.quest_store.record_decision_with_rollback(
            id,
            if partial { "partial_apply" } else { "apply" },
            &summary,
            applied.clone(),
            Some(rollback_id.clone()),
        )?;
        let detail = if partial {
            let mut remaining_review = review.clone();
            remaining_review
                .changed_files
                .retain(|file| !applied_paths.contains(&file.path));
            for group in &mut remaining_review.transaction_groups {
                group.files.retain(|path| !applied_paths.contains(path));
            }
            remaining_review
                .transaction_groups
                .retain(|group| !group.files.is_empty());
            remaining_review.summary = format!(
                "{} {} reviewed file(s) remain pending.",
                summary,
                remaining_review.changed_files.len()
            );
            remaining_review.project_fingerprint = Some(project_fingerprint(&project_root)?);
            self.quest_store
                .set_review(id, QuestStatus::ReadyForReview, remaining_review)?
        } else {
            self.quest_store.transition(id, QuestStatus::Applying)?;
            let detail = self.quest_store.transition(id, QuestStatus::Completed)?;
            let _ = self.quest_store.propose_knowledge(
                "quest-completion",
                &format!(
                    "{} completed with {} applied file(s). Review validations before reusing this as project knowledge.",
                    detail.record.title,
                    detail
                        .record
                        .decisions
                        .last()
                        .map(|decision| decision.files.len())
                        .unwrap_or_default()
                ),
                id,
            );
            detail
        };
        self.quest_store.append_timeline_event(
            id,
            "apply_result",
            &summary,
            serde_json::json!({ "partial": partial }),
        )?;
        if detail.record.project.path == project_root {
            let _ = self.hub_open_project(&serde_json::json!({ "path": project_root }));
        }
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_discard(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        if detail.record.status != QuestStatus::ReadyForReview {
            return Err(EngineError::config(
                "Quest must be in review before discarding pending items",
            ));
        }
        let review = detail
            .record
            .review
            .as_ref()
            .ok_or_else(|| EngineError::config("Quest has no review bundle"))?;
        ensure_review_project_is_current(review, &detail.record.project.path)?;
        let discarded = selected_review_paths_from_params(review, params, "discard")?;
        let discarded_paths = discarded
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();

        let total_changed = review.changed_files.len();
        let partial = discarded.len() < total_changed;
        let summary = if partial {
            format!(
                "Discarded {} of {} pending Quest file(s)",
                discarded.len(),
                total_changed
            )
        } else {
            "Discarded remaining Quest review bundle".to_owned()
        };
        let _ = self
            .quest_store
            .record_decision(id, "discard", &summary, discarded.clone())?;
        let detail = if partial {
            let mut remaining_review = review.clone();
            remaining_review
                .changed_files
                .retain(|file| !discarded_paths.contains(&file.path));
            for group in &mut remaining_review.transaction_groups {
                group.files.retain(|path| !discarded_paths.contains(path));
            }
            remaining_review
                .transaction_groups
                .retain(|group| !group.files.is_empty());
            remaining_review.summary = format!(
                "{} {} reviewed file(s) remain pending.",
                summary,
                remaining_review.changed_files.len()
            );
            self.quest_store
                .set_review(id, QuestStatus::ReadyForReview, remaining_review)?
        } else {
            let detail = self.quest_store.transition(id, QuestStatus::Completed)?;
            let _ = self.quest_store.propose_knowledge(
                "quest-completion",
                &format!(
                    "{} completed by intentionally discarding {} reviewed file(s). Preserve this as a review decision before reusing the Quest result.",
                    detail.record.title,
                    discarded.len()
                ),
                id,
            );
            detail
        };
        self.quest_store.append_timeline_event(
            id,
            "discard_result",
            &summary,
            serde_json::json!({ "partial": partial, "files": discarded }),
        )?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_rollback(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let rollback_id = required_string(params, "rollback_id")?;
        let detail = self.quest_store.get(id)?;
        let decision = detail
            .record
            .decisions
            .iter()
            .find(|decision| decision.rollback_id.as_deref() == Some(rollback_id))
            .ok_or_else(|| EngineError::config("rollback snapshot is not linked to this Quest"))?;
        let rollback_root = self
            .quest_store
            .quest_path(id)?
            .join("rollbacks")
            .join(rollback_id);
        if !rollback_root.is_dir() {
            return Err(EngineError::config("rollback snapshot is missing"));
        }
        restore_rollback_files(
            &rollback_root,
            &detail.record.project.path,
            &decision.files,
            normalize_relative_path,
        )?;
        let files = decision.files.clone();
        let detail = self.quest_store.record_decision(
            id,
            "rollback",
            "Rolled back applied Quest files",
            files.clone(),
        )?;
        self.quest_store.append_timeline_event(
            id,
            "rollback",
            "Rolled back applied Quest files",
            serde_json::json!({ "rollback_id": rollback_id, "files": files }),
        )?;
        let _ = self
            .hub_open_project(&serde_json::json!({ "path": detail.record.project.path.clone() }));
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_export(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        let quest_dir = self.quest_store.quest_path(id)?;
        let export_root = detail
            .record
            .project
            .path
            .join(".aster")
            .join("quests")
            .join(id);
        std::fs::create_dir_all(&export_root).map_err(|source| EngineError::Filesystem {
            path: export_root.clone(),
            source,
        })?;
        for file_name in ["quest.json", "intent.md", "spec.md", "events.jsonl"] {
            let source = quest_dir.join(file_name);
            if source.is_file() {
                std::fs::copy(&source, export_root.join(file_name)).map_err(|source| {
                    EngineError::Filesystem {
                        path: export_root.join(file_name),
                        source,
                    }
                })?;
            }
        }
        let relative_export = format!(".aster/quests/{id}");
        let detail = self.quest_store.record_decision(
            id,
            "export",
            &format!("Exported Quest artifacts to {relative_export}"),
            vec![relative_export.clone()],
        )?;
        self.quest_store.append_timeline_event(
            id,
            "exported",
            "Exported Quest artifacts to project",
            serde_json::json!({ "path": relative_export }),
        )?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_reject(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Rejected reviewed Quest result");
        let _ = self
            .quest_store
            .record_decision(id, "reject", reason, Vec::new())?;
        let detail = self.quest_store.transition(id, QuestStatus::Archived)?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn quest_request_revision(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Requested Quest revision");
        let _ = self
            .quest_store
            .record_decision(id, "revise", reason, Vec::new())?;
        let detail = self.quest_store.transition(id, QuestStatus::Specified)?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    pub(crate) fn prepare_quest_model_request(
        &mut self,
        config: &QuestModelConfig,
        request: engine_ai::AiRequest,
    ) -> EngineResult<PreparedQuestModelRequest> {
        let provider = if config.provider == "inherit" {
            copilot_provider_str(&self.copilot_settings.provider)?.to_owned()
        } else if config.provider == "stub" {
            return Err(EngineError::config(
                "Quest execution requires a configured AI provider.",
            ));
        } else {
            config.provider.clone()
        };
        let provider_str = provider.as_str();
        let model = if config.model.trim().is_empty() {
            self.copilot_settings.model.clone()
        } else {
            config.model.clone()
        };
        let max_tokens = config.max_tokens.max(1);
        let endpoint = config.api_endpoint.clone().or_else(|| {
            if config.provider == "inherit"
                && self.copilot_settings.provider.endpoint_configurable()
            {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            }
        });
        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        let mimo_config =
            (provider_str == "mimo").then(|| self.copilot_settings.mimo_config.clone());
        let glm_config = (provider_str == "glm").then(|| self.copilot_settings.glm_config.clone());
        Ok(PreparedQuestModelRequest {
            request,
            provider,
            model,
            api_key: self.copilot_settings.api_key.clone(),
            endpoint,
            max_tokens,
            codex_oauth,
            mimo_config,
            glm_config,
        })
    }

    pub(crate) fn default_quest_model_config(&self) -> QuestModelConfig {
        QuestModelConfig {
            provider: copilot_provider_str(&self.copilot_settings.provider)
                .unwrap_or("inherit")
                .to_owned(),
            model: self.copilot_settings.model.clone(),
            api_endpoint: if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            },
            max_tokens: self.copilot_settings.max_tokens,
            thinking_effort: "medium".to_owned(),
        }
    }

    pub(crate) fn quest_model_config_from_params(
        &self,
        params: &Value,
    ) -> EngineResult<QuestModelConfig> {
        let mut config = self.default_quest_model_config();
        if let Some(value) = params.get("model_config") {
            config = serde_json::from_value(value.clone()).map_err(|error| {
                EngineError::config(format!("invalid Quest model config: {error}"))
            })?;
        }
        Ok(config)
    }
}
