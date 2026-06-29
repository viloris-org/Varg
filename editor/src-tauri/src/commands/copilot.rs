use serde_json::Value;
use tauri::{Emitter, State};

use engine_core::{TaskPriority, shared_task_runtime};

use crate::state::EditorHostState;
use crate::{CompletedCopilotRequest, CopilotRequestState};

#[tauri::command]
pub(crate) fn start_copilot_plan(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, CopilotRequestState>,
    request_id: String,
    params: Value,
) -> Result<(), String> {
    let prepared = state
        .with_host(|host| host.prepare_copilot_request(&params))
        .map_err(|error| error.to_string())?;
    let requests = requests.requests.clone();
    shared_task_runtime().spawn("editor.copilot.plan", TaskPriority::Background, move || {
        let original_prompt = prepared.original_prompt.clone();
        let knowledge_entries_used = prepared.knowledge_entries_used;
        let approval_mode = prepared.approval_mode;
        let continuation_request = prepared.request.clone();
        let provider = prepared.provider.clone();
        let model_name = prepared.model.clone();
        let api_key = prepared.api_key.clone();
        let endpoint = prepared.endpoint.clone();
        let max_tokens = prepared.max_tokens;
        let codex_oauth = prepared.codex_oauth.clone();
        let mimo_config = prepared.mimo_config.clone();
        let glm_config = prepared.glm_config.clone();
        let result = engine_ai::providers::create_provider(
            &prepared.provider,
            &prepared.model,
            prepared.api_key.as_deref(),
            prepared.endpoint.as_deref(),
            prepared.max_tokens,
            prepared.codex_oauth,
            prepared.mimo_config.as_ref(),
            prepared.glm_config.as_ref(),
        )
        .and_then(|model| {
            let mut session = engine_ai::AgentSession::new(prepared.cached_context)?;
            let response = session.respond_with_tool_results_streaming(
                model.as_ref(),
                prepared.request,
                approval_mode.planning_policy(),
                &mut |delta| {
                    if requests
                        .lock()
                        .expect("poisoned lock")
                        .cancelled
                        .contains(&request_id)
                    {
                        return;
                    }
                    let delta_payload = match &delta {
                        engine_ai::AiStreamDelta::ToolCallDelta(tc) => {
                            serde_json::to_string(tc).unwrap_or_default()
                        }
                        _ => delta.text().to_owned(),
                    };
                    let _ = app.emit(
                        "copilot-stream",
                        serde_json::json!({
                            "request_id": request_id,
                            "kind": delta.kind(),
                            "delta": delta_payload,
                        }),
                    );
                },
            )?;
            Ok((response, session.context))
        });
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "copilot-stream-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        let (content_result, tool_calls, resolved_operations, cached_context) = match result {
            Ok((response, cached_context)) => (
                Ok(response.content),
                response.tool_calls,
                response.resolved_operations,
                Some(cached_context),
            ),
            Err(e) => (Err(e.to_string()), Vec::new(), Vec::new(), None),
        };
        request_state.completed.insert(
            request_id.clone(),
            CompletedCopilotRequest {
                original_prompt,
                response: content_result,
                tool_calls,
                resolved_operations,
                cached_context,
                knowledge_entries_used,
                approval_mode,
                request: Some(continuation_request),
                provider,
                model: model_name,
                api_key,
                endpoint,
                max_tokens,
                codex_oauth,
                mimo_config,
                glm_config,
            },
        );
        drop(request_state);
        let _ = app.emit(
            "copilot-stream-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn finish_copilot_plan(
    state: State<'_, EditorHostState>,
    requests: State<'_, CopilotRequestState>,
    request_id: String,
) -> Result<Value, String> {
    let completed = requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "copilot request has not completed".to_owned())?;
    let response = completed.response?;
    let cached_context = completed
        .cached_context
        .ok_or_else(|| "copilot request did not retain project context".to_owned())?;
    state
        .with_host(|host| {
            host.finish_copilot_response_with_tools(
                &completed.original_prompt,
                &response,
                &completed.tool_calls,
                &completed.resolved_operations,
                cached_context,
                completed.knowledge_entries_used,
                completed.approval_mode,
                completed.request.map(|request| crate::CopilotContinuation {
                    request,
                    provider: completed.provider,
                    model: completed.model,
                    api_key: completed.api_key,
                    endpoint: completed.endpoint,
                    max_tokens: completed.max_tokens,
                    codex_oauth: completed.codex_oauth,
                    mimo_config: completed.mimo_config,
                    glm_config: completed.glm_config,
                    knowledge_entries_used: completed.knowledge_entries_used,
                    approval_mode: completed.approval_mode,
                }),
            )
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cancel_copilot_plan(
    requests: State<'_, CopilotRequestState>,
    request_id: String,
) -> Result<(), String> {
    requests
        .requests
        .lock()
        .expect("poisoned lock")
        .cancelled
        .insert(request_id);
    Ok(())
}
