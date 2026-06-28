//! Persistent Quest runtime skeleton.
//!
//! This module provides engine-level turn, event, and artifact primitives that
//! can back the editor Quest surface without tying execution state to Tauri UI
//! structs.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

/// Persistent runtime for Quest execution events and artifacts.
#[derive(Clone, Debug)]
pub struct QuestRuntime {
    root: PathBuf,
}

impl QuestRuntime {
    /// Creates a Quest runtime rooted at a directory controlled by the host.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Starts a new persistent Quest turn.
    pub fn start_turn(&self, quest_id: &str, goal: &str) -> EngineResult<QuestTurn> {
        validate_runtime_id(quest_id, "Quest id")?;
        let goal = goal.trim();
        if goal.is_empty() {
            return Err(EngineError::config("Quest turn goal must not be empty"));
        }
        let timestamp_ms = unix_time_ms();
        let turn = QuestTurn {
            id: format!(
                "turn-{timestamp_ms}-{}",
                NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed)
            ),
            quest_id: quest_id.to_owned(),
            goal: goal.to_owned(),
            status: QuestTurnStatus::Running,
            started_at_ms: timestamp_ms,
            completed_at_ms: None,
        };
        self.write_turn(&turn)?;
        self.record_event(
            quest_id,
            Some(&turn.id),
            "turn_started",
            "Quest turn started",
            serde_json::json!({ "goal": goal }),
        )?;
        Ok(turn)
    }

    /// Starts a durable Quest run.
    pub fn start_run(&self, quest_id: &str, objective: &str) -> EngineResult<QuestRun> {
        validate_runtime_id(quest_id, "Quest id")?;
        let objective = objective.trim();
        if objective.is_empty() {
            return Err(EngineError::config("Quest run objective must not be empty"));
        }
        let timestamp_ms = unix_time_ms();
        let run = QuestRun {
            id: format!(
                "run-{timestamp_ms}-{}",
                NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed)
            ),
            quest_id: quest_id.to_owned(),
            objective: objective.to_owned(),
            status: QuestRunStatus::Active,
            active_step: None,
            workspace_id: None,
            token_budget: None,
            tokens_used: 0,
            elapsed_ms: 0,
            blocked_count: 0,
            last_blocker: None,
            loaded_tools: Vec::new(),
            grants: Vec::new(),
            evidence: Vec::new(),
            started_at_ms: timestamp_ms,
            updated_at_ms: timestamp_ms,
            completed_at_ms: None,
        };
        self.write_run(&run)?;
        self.record_event(
            quest_id,
            None,
            "run_started",
            "Quest run started",
            serde_json::json!({ "run_id": run.id, "objective": objective }),
        )?;
        Ok(run)
    }

    /// Appends an updated durable Quest run snapshot.
    pub fn update_run(&self, run: &mut QuestRun) -> EngineResult<()> {
        validate_runtime_id(&run.quest_id, "Quest id")?;
        validate_runtime_id(&run.id, "Quest run id")?;
        run.updated_at_ms = unix_time_ms();
        if run.status.is_terminal() && run.completed_at_ms.is_none() {
            run.completed_at_ms = Some(run.updated_at_ms);
        }
        self.write_run(run)?;
        self.record_event(
            &run.quest_id,
            None,
            "run_updated",
            "Quest run updated",
            serde_json::json!({
                "run_id": run.id,
                "status": run.status,
                "active_step": run.active_step,
                "workspace_id": run.workspace_id,
                "loaded_tools": run.loaded_tools,
                "evidence_count": run.evidence.len(),
            }),
        )?;
        Ok(())
    }

    /// Completes a persistent Quest turn.
    pub fn complete_turn(&self, turn: &mut QuestTurn, status: QuestTurnStatus) -> EngineResult<()> {
        turn.status = status;
        turn.completed_at_ms = Some(unix_time_ms());
        self.write_turn(turn)?;
        let _ = self.record_event(
            &turn.quest_id,
            Some(&turn.id),
            "turn_completed",
            "Quest turn completed",
            serde_json::json!({ "status": turn.status }),
        )?;
        Ok(())
    }

    /// Records a structured runtime event.
    pub fn record_event(
        &self,
        quest_id: &str,
        turn_id: Option<&str>,
        kind: &str,
        summary: &str,
        details: Value,
    ) -> EngineResult<QuestRuntimeEvent> {
        validate_runtime_id(quest_id, "Quest id")?;
        if let Some(turn_id) = turn_id {
            validate_runtime_id(turn_id, "Quest turn id")?;
        }
        let kind = kind.trim();
        if kind.is_empty() {
            return Err(EngineError::config("Quest event kind must not be empty"));
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(EngineError::config("Quest event summary must not be empty"));
        }
        let timestamp_ms = unix_time_ms();
        let event = QuestRuntimeEvent {
            id: format!(
                "event-{timestamp_ms}-{}",
                NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed)
            ),
            quest_id: quest_id.to_owned(),
            turn_id: turn_id.map(str::to_owned),
            timestamp_ms,
            kind: kind.to_owned(),
            summary: summary.to_owned(),
            details,
        };
        append_jsonl(
            &self.quest_runtime_dir(quest_id).join("events.jsonl"),
            &event,
        )?;
        Ok(event)
    }

    /// Writes an artifact under the Quest runtime root and records an event.
    pub fn write_artifact(
        &self,
        quest_id: &str,
        turn_id: Option<&str>,
        artifact: QuestArtifactWrite,
    ) -> EngineResult<QuestRuntimeArtifact> {
        validate_runtime_id(quest_id, "Quest id")?;
        if let Some(turn_id) = turn_id {
            validate_runtime_id(turn_id, "Quest turn id")?;
        }
        artifact.validate()?;
        let relative = sanitize_relative_path(&artifact.path)?;
        let path = self
            .quest_runtime_dir(quest_id)
            .join("artifacts")
            .join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, artifact.content.as_bytes()).map_err(|source| {
            EngineError::Filesystem {
                path: path.clone(),
                source,
            }
        })?;
        let saved = QuestRuntimeArtifact {
            quest_id: quest_id.to_owned(),
            turn_id: turn_id.map(str::to_owned),
            kind: artifact.kind,
            label: artifact.label,
            path: relative.to_string_lossy().to_string(),
            bytes: artifact.content.len(),
        };
        self.record_event(
            quest_id,
            turn_id,
            "artifact_written",
            &format!("Quest artifact written: {}", saved.label),
            serde_json::to_value(&saved).map_err(|error| EngineError::other(error.to_string()))?,
        )?;
        Ok(saved)
    }

    /// Reads all runtime events for a Quest.
    pub fn events(&self, quest_id: &str) -> EngineResult<Vec<QuestRuntimeEvent>> {
        validate_runtime_id(quest_id, "Quest id")?;
        read_jsonl(&self.quest_runtime_dir(quest_id).join("events.jsonl"))
    }

    /// Reads latest known durable Quest run snapshots.
    pub fn runs(&self, quest_id: &str) -> EngineResult<Vec<QuestRun>> {
        validate_runtime_id(quest_id, "Quest id")?;
        let snapshots: Vec<QuestRun> =
            read_jsonl(&self.quest_runtime_dir(quest_id).join("runs.jsonl"))?;
        let mut by_id = BTreeMap::new();
        for run in snapshots {
            by_id.insert(run.id.clone(), run);
        }
        Ok(by_id.into_values().collect())
    }

    fn write_turn(&self, turn: &QuestTurn) -> EngineResult<()> {
        append_jsonl(
            &self.quest_runtime_dir(&turn.quest_id).join("turns.jsonl"),
            turn,
        )
    }

    fn write_run(&self, run: &QuestRun) -> EngineResult<()> {
        append_jsonl(
            &self.quest_runtime_dir(&run.quest_id).join("runs.jsonl"),
            run,
        )
    }

    fn quest_runtime_dir(&self, quest_id: &str) -> PathBuf {
        self.root.join(quest_id).join("runtime")
    }
}

/// One persistent Quest execution turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestTurn {
    /// Stable turn id.
    pub id: String,
    /// Owning Quest id.
    pub quest_id: String,
    /// Turn objective.
    pub goal: String,
    /// Current turn status.
    pub status: QuestTurnStatus,
    /// Start timestamp.
    pub started_at_ms: u64,
    /// Completion timestamp.
    #[serde(default)]
    pub completed_at_ms: Option<u64>,
}

/// Quest turn lifecycle status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestTurnStatus {
    /// Turn is currently running.
    Running,
    /// Turn completed successfully.
    Completed,
    /// Turn is blocked on policy, context, credentials, or user input.
    Blocked,
    /// Turn was canceled.
    Canceled,
}

/// One durable Quest execution run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestRun {
    /// Stable run id.
    pub id: String,
    /// Owning Quest id.
    pub quest_id: String,
    /// Durable execution objective.
    pub objective: String,
    /// Current run status.
    pub status: QuestRunStatus,
    /// Current step label or id.
    #[serde(default)]
    pub active_step: Option<String>,
    /// Isolated workspace id used by the run.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Optional token budget.
    #[serde(default)]
    pub token_budget: Option<u64>,
    /// Accounted model tokens used by this run.
    #[serde(default)]
    pub tokens_used: u64,
    /// Accounted elapsed wall time in milliseconds.
    #[serde(default)]
    pub elapsed_ms: u64,
    /// Count of repeated blocker observations.
    #[serde(default)]
    pub blocked_count: u32,
    /// Last blocker summary.
    #[serde(default)]
    pub last_blocker: Option<String>,
    /// Tool names whose schemas were loaded for this run.
    #[serde(default)]
    pub loaded_tools: Vec<String>,
    /// Capability grants associated with this run.
    #[serde(default)]
    pub grants: Vec<QuestRunGrant>,
    /// Evidence artifacts produced by this run.
    #[serde(default)]
    pub evidence: Vec<QuestRunEvidenceRef>,
    /// Start timestamp.
    pub started_at_ms: u64,
    /// Last update timestamp.
    pub updated_at_ms: u64,
    /// Completion timestamp.
    #[serde(default)]
    pub completed_at_ms: Option<u64>,
}

/// Durable Quest run lifecycle status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestRunStatus {
    /// Run is actively progressing.
    Active,
    /// Run is waiting for user input, credentials, policy, or manual intervention.
    WaitingForUser,
    /// Run hit its configured budget.
    BudgetLimited,
    /// Run produced a reviewable bundle.
    ReadyForReview,
    /// Run is genuinely blocked.
    Blocked,
    /// Run was canceled.
    Canceled,
    /// Run completed.
    Completed,
}

impl QuestRunStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Blocked | Self::Canceled | Self::Completed)
    }
}

/// Capability grant associated with a Quest run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestRunGrant {
    /// Stable grant id.
    pub id: String,
    /// Granted capability string.
    pub capability: String,
    /// Scope such as `run`, `quest`, or `project`.
    pub scope: String,
    /// Human-readable reason for the grant.
    pub reason: String,
    /// Grant creation timestamp.
    pub created_at_ms: u64,
}

/// Evidence artifact produced by a Quest run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestRunEvidenceRef {
    /// Evidence kind, such as `plan`, `validation`, `diff`, or `review`.
    pub kind: String,
    /// Human-readable label.
    pub label: String,
    /// Artifact path relative to the Quest runtime root.
    pub path: String,
    /// Evidence creation timestamp.
    pub created_at_ms: u64,
}

/// Structured Quest runtime event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuestRuntimeEvent {
    /// Event id.
    pub id: String,
    /// Owning Quest id.
    pub quest_id: String,
    /// Optional turn id.
    #[serde(default)]
    pub turn_id: Option<String>,
    /// Timestamp.
    pub timestamp_ms: u64,
    /// Machine-readable event kind.
    pub kind: String,
    /// User-facing summary.
    pub summary: String,
    /// Structured event details.
    pub details: Value,
}

/// Request to write a runtime artifact.
#[derive(Clone, Debug)]
pub struct QuestArtifactWrite {
    /// Artifact kind, such as `plan`, `validation`, `review`, or `diff`.
    pub kind: String,
    /// Human-readable artifact label.
    pub label: String,
    /// Runtime-artifact-relative path.
    pub path: PathBuf,
    /// Complete artifact content.
    pub content: String,
}

impl QuestArtifactWrite {
    fn validate(&self) -> EngineResult<()> {
        if self.kind.trim().is_empty() {
            return Err(EngineError::config("Quest artifact kind must not be empty"));
        }
        if self.label.trim().is_empty() {
            return Err(EngineError::config(
                "Quest artifact label must not be empty",
            ));
        }
        if self.content.is_empty() {
            return Err(EngineError::config(
                "Quest artifact content must not be empty",
            ));
        }
        Ok(())
    }
}

/// Saved runtime artifact metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestRuntimeArtifact {
    /// Owning Quest id.
    pub quest_id: String,
    /// Optional turn id.
    #[serde(default)]
    pub turn_id: Option<String>,
    /// Artifact kind.
    pub kind: String,
    /// Artifact label.
    pub label: String,
    /// Runtime-artifact-relative path.
    pub path: String,
    /// Content byte length.
    pub bytes: usize,
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> EngineResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;
    let line =
        serde_json::to_string(value).map_err(|error| EngineError::other(error.to_string()))?;
    writeln!(file, "{line}").map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> EngineResult<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    BufReader::new(file)
        .lines()
        .filter_map(|line| match line {
            Ok(line) if line.trim().is_empty() => None,
            result => Some(result),
        })
        .map(|line| {
            let line = line.map_err(|source| EngineError::Filesystem {
                path: path.to_path_buf(),
                source,
            })?;
            serde_json::from_str(&line).map_err(|error| EngineError::config(error.to_string()))
        })
        .collect()
}

fn sanitize_relative_path(path: &Path) -> EngineResult<PathBuf> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(EngineError::config(
                    "Quest artifact path must stay inside runtime artifacts",
                ));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(EngineError::config("Quest artifact path must not be empty"));
    }
    Ok(relative)
}

fn validate_runtime_id(value: &str, label: &str) -> EngineResult<()> {
    if !value.trim().is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Ok(())
    } else {
        Err(EngineError::config(format!("{label} is invalid")))
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quest_runtime_records_turn_events_and_artifacts() {
        let root = std::env::temp_dir().join(format!("varg-quest-runtime-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runtime = QuestRuntime::new(&root);

        let mut turn = runtime
            .start_turn("quest-runtime-test", "Build a playable prototype")
            .unwrap();
        let mut run = runtime
            .start_run("quest-runtime-test", "Build a playable prototype")
            .unwrap();
        run.active_step = Some("plan".into());
        run.loaded_tools.push("write_script".into());
        run.evidence.push(QuestRunEvidenceRef {
            kind: "plan".into(),
            label: "Initial plan".into(),
            path: "artifacts/plans/initial.md".into(),
            created_at_ms: unix_time_ms(),
        });
        runtime.update_run(&mut run).unwrap();
        let artifact = runtime
            .write_artifact(
                "quest-runtime-test",
                Some(&turn.id),
                QuestArtifactWrite {
                    kind: "plan".into(),
                    label: "Initial plan".into(),
                    path: PathBuf::from("plans/initial.md"),
                    content: "# Plan\n\nShip a thin slice.".into(),
                },
            )
            .unwrap();
        runtime
            .complete_turn(&mut turn, QuestTurnStatus::Completed)
            .unwrap();

        assert_eq!(turn.status, QuestTurnStatus::Completed);
        assert_eq!(artifact.path, "plans/initial.md");
        let events = runtime.events("quest-runtime-test").unwrap();
        assert_eq!(events.len(), 5);
        let runs = runtime.runs("quest-runtime-test").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].loaded_tools, vec!["write_script"]);
        assert!(events.iter().any(|event| event.kind == "turn_started"));
        assert!(events.iter().any(|event| event.kind == "run_started"));
        assert!(events.iter().any(|event| event.kind == "run_updated"));
        assert!(events.iter().any(|event| event.kind == "artifact_written"));
        assert!(events.iter().any(|event| event.kind == "turn_completed"));
        assert!(
            root.join("quest-runtime-test/runtime/artifacts/plans/initial.md")
                .is_file()
        );

        let _ = fs::remove_dir_all(root);
    }
}
