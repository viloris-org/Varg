use std::time::Instant;

use serde_json::Value;
use tauri::{Emitter, State};

use crate::state::EditorHostState;
use crate::{QuestExecutionRequestState, record_quest_execution_failure, run_quest_execution};

#[tauri::command]
pub(crate) fn start_quest_execution(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestExecutionRequestState>,
    request_id: String,
    id: String,
) -> Result<(), String> {
    let started_at = Instant::now();
    let prepared = state
        .with_host(|host| host.prepare_quest_execution(&id))
        .map_err(|error| error.to_string())?;
    let quest_store = prepared.quest_store.clone();
    let requests = requests.requests.clone();
    std::thread::spawn(move || {
        let result = run_quest_execution(prepared)
            .or_else(|error| record_quest_execution_failure(&quest_store, &id, started_at, error));
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "quest-execution-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        request_state.completed.insert(
            request_id.clone(),
            result.map_err(|error| error.to_string()),
        );
        drop(request_state);
        let _ = app.emit(
            "quest-execution-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn finish_quest_execution(
    requests: State<'_, QuestExecutionRequestState>,
    request_id: String,
) -> Result<Value, String> {
    requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "Quest execution has not completed".to_owned())?
}

#[tauri::command]
pub(crate) fn cancel_quest_execution(
    requests: State<'_, QuestExecutionRequestState>,
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
