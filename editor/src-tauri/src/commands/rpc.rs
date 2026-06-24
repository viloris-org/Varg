use std::time::Duration;

use serde_json::Value;
use tauri::{Emitter, State};

use crate::state::EditorHostState;

#[tauri::command]
pub(crate) fn rpc(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    method: String,
    params: Value,
) -> Result<Value, String> {
    state.with_host(|host| {
        let result = if method == "copilot/plan" {
            let request_id = params
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            host.copilot_plan_streaming(&params, &mut |delta| {
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
            })
        } else {
            host.handle(&method, &params)
        };
        result.map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub(crate) async fn create_openai_realtime_transcription_session(
    state: State<'_, EditorHostState>,
) -> Result<Value, String> {
    let (api_key, endpoint) = state
        .with_host(|host| host.openai_realtime_transcription_config())
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
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
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(12)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .build()
            .into();
        let mut response = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|error| format!("OpenAI Realtime transcription session failed: {error}"))?;
        let json: Value = response.body_mut().read_json().map_err(|error| {
            format!("OpenAI Realtime transcription session response parse failed: {error}")
        })?;
        Ok(serde_json::json!({
            "session": json,
            "model": "gpt-realtime-whisper",
            "endpoint": endpoint,
            "realtime_url": format!("{endpoint}/realtime/calls"),
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}
