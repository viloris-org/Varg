use serde_json::Value;
use tauri::{Emitter, State};

use engine_core::{TaskPriority, shared_task_runtime};

use crate::state::EditorHostState;
use crate::{
    CompletedQuestAiRequest, EngineError, EngineResult, PreparedQuestAiRequest,
    PreparedQuestCreateRequest, PreparedQuestModelRequest, QuestAiRequestState,
    parse_generated_quest_response,
};

fn run_prepared_quest_model(
    prepared: PreparedQuestModelRequest,
    on_delta: &mut dyn FnMut(engine_ai::AiStreamDelta),
) -> EngineResult<engine_ai::AiResponse> {
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
    model.chat_stream(prepared.request, on_delta)
}

#[tauri::command]
pub(crate) fn start_quest_ai_request(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestAiRequestState>,
    request_id: String,
    kind: String,
    params: Value,
) -> Result<(), String> {
    let prepared = state
        .with_host(|host| match kind.as_str() {
            "create" => host
                .prepare_quest_create_request(&params)
                .map(PreparedQuestAiRequest::Create),
            "rewrite" => host
                .prepare_quest_rewrite_request(&params)
                .map(PreparedQuestAiRequest::Rewrite),
            _ => Err(EngineError::config(format!(
                "unknown Quest AI request kind: {kind}"
            ))),
        })
        .map_err(|error| error.to_string())?;
    let requests = requests.requests.clone();
    shared_task_runtime().spawn("editor.quest.ai", TaskPriority::Background, move || {
        let emit_delta = &mut |delta: engine_ai::AiStreamDelta| {
            if requests
                .lock()
                .expect("poisoned lock")
                .cancelled
                .contains(&request_id)
            {
                return;
            }
            let delta_payload = match &delta {
                engine_ai::AiStreamDelta::ToolCallDelta(tool_call) => {
                    serde_json::to_string(tool_call).unwrap_or_default()
                }
                _ => delta.text().to_owned(),
            };
            let _ = app.emit(
                "quest-ai-stream",
                serde_json::json!({
                    "request_id": request_id,
                    "kind": delta.kind(),
                    "delta": delta_payload,
                }),
            );
        };
        let completed = match prepared {
            PreparedQuestAiRequest::Create(prepared) => {
                let PreparedQuestCreateRequest {
                    model_request,
                    title,
                    goal,
                    project,
                    mode,
                    model_config,
                } = prepared;
                let generated = run_prepared_quest_model(model_request, emit_delta)
                    .and_then(|response| {
                        parse_generated_quest_response(
                            &response.tool_calls,
                            &response.content,
                            &goal,
                        )
                    })
                    .map_err(|error| error.to_string());
                CompletedQuestAiRequest::Create {
                    generated,
                    title,
                    goal,
                    project,
                    mode,
                    model_config,
                }
            }
            PreparedQuestAiRequest::Rewrite(prepared) => {
                let rewritten = run_prepared_quest_model(prepared, emit_delta)
                    .map(|response| response.content)
                    .map_err(|error| error.to_string());
                CompletedQuestAiRequest::Rewrite(rewritten)
            }
        };
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "quest-ai-stream-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        request_state
            .completed
            .insert(request_id.clone(), completed);
        drop(request_state);
        let _ = app.emit(
            "quest-ai-stream-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn finish_quest_ai_request(
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestAiRequestState>,
    request_id: String,
) -> Result<Value, String> {
    let completed = requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "Quest AI request has not completed".to_owned())?;
    state
        .with_host(|host| match completed {
            CompletedQuestAiRequest::Create {
                generated,
                title,
                goal,
                project,
                mode,
                model_config,
            } => host.finish_quest_create(
                generated.map_err(EngineError::other)?,
                title,
                goal,
                project,
                mode,
                model_config,
            ),
            CompletedQuestAiRequest::Rewrite(response) => {
                host.finish_quest_rewrite(response.map_err(EngineError::other)?)
            }
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cancel_quest_ai_request(
    requests: State<'_, QuestAiRequestState>,
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
