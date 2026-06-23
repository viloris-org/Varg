use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

static NEXT_QUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestStatus {
    Draft,
    Clarifying,
    #[serde(alias = "ready")]
    Specified,
    Planning,
    Prepared,
    Running,
    #[serde(alias = "paused")]
    WaitingForUser,
    Validating,
    Repairing,
    #[serde(alias = "review")]
    ReadyForReview,
    Applying,
    Completed,
    Blocked,
    Canceled,
    #[serde(alias = "rejected")]
    Archived,
}

impl QuestStatus {
    fn can_transition_to(self, next: Self) -> bool {
        use QuestStatus::*;
        matches!(
            (self, next),
            (Draft, Clarifying)
                | (Draft, Specified)
                | (Draft, Canceled)
                | (Draft, Archived)
                | (Clarifying, Specified)
                | (Clarifying, WaitingForUser)
                | (Clarifying, Canceled)
                | (Clarifying, Archived)
                | (Specified, Planning)
                | (Specified, Prepared)
                | (Specified, Running)
                | (Specified, Canceled)
                | (Specified, Archived)
                | (Planning, Specified)
                | (Planning, Prepared)
                | (Planning, WaitingForUser)
                | (Planning, Canceled)
                | (Planning, Archived)
                | (Prepared, Running)
                | (Prepared, WaitingForUser)
                | (Prepared, Canceled)
                | (Prepared, Archived)
                | (Running, Validating)
                | (Running, WaitingForUser)
                | (Running, ReadyForReview)
                | (Running, Blocked)
                | (Running, Canceled)
                | (Running, Archived)
                | (WaitingForUser, Specified)
                | (WaitingForUser, Planning)
                | (WaitingForUser, Prepared)
                | (WaitingForUser, Running)
                | (WaitingForUser, Canceled)
                | (WaitingForUser, Archived)
                | (Validating, Repairing)
                | (Validating, ReadyForReview)
                | (Validating, WaitingForUser)
                | (Validating, Blocked)
                | (Validating, Canceled)
                | (Validating, Archived)
                | (Repairing, Running)
                | (Repairing, Validating)
                | (Repairing, WaitingForUser)
                | (Repairing, Blocked)
                | (Repairing, Canceled)
                | (Repairing, Archived)
                | (ReadyForReview, Applying)
                | (ReadyForReview, Specified)
                | (ReadyForReview, Planning)
                | (ReadyForReview, Running)
                | (ReadyForReview, Completed)
                | (ReadyForReview, Canceled)
                | (ReadyForReview, Archived)
                | (Applying, Completed)
                | (Applying, Blocked)
                | (Applying, Archived)
                | (Blocked, Specified)
                | (Blocked, Planning)
                | (Blocked, Prepared)
                | (Blocked, Running)
                | (Blocked, Canceled)
                | (Blocked, Archived)
                | (Canceled, Specified)
                | (Canceled, Archived)
                | (Completed, Archived)
                | (Completed, Specified)
                | (Archived, Draft)
                | (Archived, Clarifying)
                | (Archived, Specified)
                | (Archived, Planning)
                | (Archived, Prepared)
                | (Archived, WaitingForUser)
                | (Archived, ReadyForReview)
                | (Archived, Completed)
                | (Archived, Blocked)
                | (Archived, Canceled)
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestNextAction {
    pub label: String,
    pub reason: String,
}

impl Default for QuestNextAction {
    fn default() -> Self {
        Self {
            label: "Review Quest state".to_owned(),
            reason: "Varg has not selected the next workflow action yet.".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestProject {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestMode {
    #[default]
    Solo,
    Extra,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_endpoint: Option<String>,
    pub max_tokens: u32,
    #[serde(default = "default_quest_thinking_effort")]
    pub thinking_effort: String,
}

impl Default for QuestModelConfig {
    fn default() -> Self {
        Self {
            provider: "inherit".to_owned(),
            model: String::new(),
            api_endpoint: None,
            max_tokens: 4096,
            thinking_effort: default_quest_thinking_effort(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestAutonomyPolicy {
    #[serde(default = "default_true")]
    pub workspace_writes_automatic: bool,
    #[serde(default = "default_true")]
    pub active_project_apply_requires_approval: bool,
    #[serde(default)]
    pub allowlisted_commands_automatic: bool,
    #[serde(default)]
    pub high_risk_requires_confirmation: bool,
}

impl Default for QuestAutonomyPolicy {
    fn default() -> Self {
        Self {
            workspace_writes_automatic: true,
            active_project_apply_requires_approval: true,
            allowlisted_commands_automatic: true,
            high_risk_requires_confirmation: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub status: String,
    #[serde(default)]
    pub diff: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestTransactionGroup {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub risk: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestExplorationAttempt {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub outcome: String,
    pub artifact_path: String,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestReviewFinding {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub severity: String,
    #[serde(default)]
    pub artifact_path: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationResult {
    pub name: String,
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub policy_approved: bool,
    #[serde(default)]
    pub log: String,
}

impl ValidationResult {
    pub fn new(
        name: impl Into<String>,
        status: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: status.into(),
            summary: summary.into(),
            command_id: None,
            command: None,
            policy_approved: false,
            log: String::new(),
        }
    }

    pub fn with_policy_command(
        mut self,
        command_id: impl Into<String>,
        command: impl Into<String>,
        log: impl Into<String>,
    ) -> Self {
        self.command_id = Some(command_id.into());
        self.command = Some(command.into());
        self.policy_approved = true;
        self.log = log.into();
        self
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QuestReviewMetrics {
    #[serde(default)]
    pub intent_to_first_action_ms: Option<u64>,
    #[serde(default)]
    pub tool_call_latency_ms: Option<u64>,
    #[serde(default)]
    pub validator_turnaround_ms: Option<u64>,
    #[serde(default)]
    pub context_relevance_score: Option<f32>,
    #[serde(default)]
    pub failed_action_recovery_rate: Option<f32>,
    #[serde(default)]
    pub review_evidence_quality_score: Option<f32>,
    #[serde(default)]
    pub isolated_attempt_count: u32,
    #[serde(default)]
    pub validation_count: u32,
    #[serde(default)]
    pub validation_failure_count: u32,
    #[serde(default)]
    pub baseline_changed_file_count: u32,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QuestReview {
    pub summary: String,
    pub changed_files: Vec<ChangedFile>,
    #[serde(default)]
    pub transaction_groups: Vec<QuestTransactionGroup>,
    #[serde(default)]
    pub exploration_attempts: Vec<QuestExplorationAttempt>,
    #[serde(default)]
    pub findings: Vec<QuestReviewFinding>,
    pub validations: Vec<ValidationResult>,
    pub unresolved_issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<QuestReviewAction>,
    #[serde(default)]
    pub project_fingerprint: Option<String>,
    #[serde(default)]
    pub metrics: QuestReviewMetrics,
    pub risk: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestReviewAction {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub target: Option<String>,
}

impl QuestReviewAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: kind.into(),
            target: None,
        }
    }

    pub fn with_target(
        id: impl Into<String>,
        label: impl Into<String>,
        kind: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: kind.into(),
            target: Some(target.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestArtifactLink {
    pub kind: String,
    pub label: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestDecision {
    pub kind: String,
    pub summary: String,
    pub files: Vec<String>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub rollback_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestCheckpoint {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub artifact_path: Option<String>,
    #[serde(default)]
    pub project_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub status: String,
    pub category: String,
    pub content: String,
    pub source: String,
    #[serde(default = "default_knowledge_reference_status")]
    pub reference_status: String,
    #[serde(default = "default_knowledge_reference_summary")]
    pub reference_summary: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestRecord {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub goal: String,
    pub status: QuestStatus,
    pub project: QuestProject,
    #[serde(default)]
    pub mode: QuestMode,
    #[serde(default)]
    pub model_config: QuestModelConfig,
    #[serde(default)]
    pub autonomy: QuestAutonomyPolicy,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub workspace_id: Option<String>,
    #[serde(default = "default_intent_path")]
    pub intent_path: String,
    #[serde(default = "default_spec_path")]
    pub spec_path: Option<String>,
    #[serde(default = "default_trace_path")]
    pub trace_path: String,
    #[serde(default)]
    pub artifact_links: Vec<QuestArtifactLink>,
    #[serde(default)]
    pub attached_knowledge_ids: Vec<String>,
    #[serde(default)]
    pub branch_of: Option<String>,
    #[serde(default)]
    pub branch_ids: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<QuestDecision>,
    #[serde(default)]
    pub checkpoints: Vec<QuestCheckpoint>,
    #[serde(default)]
    pub tasks: Vec<QuestTask>,
    #[serde(default)]
    pub next_action: QuestNextAction,
    pub review: Option<QuestReview>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestEvent {
    pub id: String,
    pub quest_id: String,
    pub timestamp_ms: u64,
    pub kind: String,
    pub summary: String,
    pub details: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct QuestDetail {
    #[serde(flatten)]
    pub record: QuestRecord,
    pub intent: String,
    pub spec: String,
    pub attached_knowledge: Vec<KnowledgeEntry>,
    pub events: Vec<QuestEvent>,
}

#[derive(Clone, Debug)]
pub struct QuestStore {
    root: PathBuf,
}

impl QuestStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn list(&self) -> EngineResult<Vec<QuestRecord>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut quests: Vec<QuestRecord> = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|source| EngineError::Filesystem {
            path: self.root.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| EngineError::Filesystem {
                path: self.root.clone(),
                source,
            })?;
            let snapshot = entry.path().join("quest.json");
            if snapshot.is_file() {
                let mut record: QuestRecord = read_json(&snapshot)?;
                normalize_record_metadata(&mut record);
                quests.push(record);
            }
        }
        quests.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        Ok(quests)
    }

    pub fn list_knowledge(&self) -> EngineResult<Vec<KnowledgeEntry>> {
        let mut entries = read_knowledge_entries(&self.knowledge_path())?;
        for entry in &mut entries {
            normalize_knowledge_reference(entry, &self.root);
        }
        entries.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        Ok(entries)
    }

    pub fn propose_knowledge(
        &self,
        category: &str,
        content: &str,
        source: &str,
    ) -> EngineResult<Vec<KnowledgeEntry>> {
        let category = category.trim();
        let content = content.trim();
        if category.is_empty() {
            return Err(EngineError::config("Knowledge category must not be empty"));
        }
        if content.is_empty() {
            return Err(EngineError::config("Knowledge content must not be empty"));
        }
        let mut entries = read_knowledge_entries(&self.knowledge_path())?;
        let now = unix_time_ms();
        let normalized = normalize_knowledge_content(content);
        if let Some(existing) = entries.iter_mut().find(|entry| {
            entry.category == category && normalize_knowledge_content(&entry.content) == normalized
        }) {
            existing.source = source.to_owned();
            normalize_knowledge_reference(existing, &self.root);
            existing.updated_at_ms = now;
        } else {
            let mut entry = KnowledgeEntry {
                id: format!(
                    "knowledge-{now}-{}",
                    NEXT_QUEST_ID.fetch_add(1, Ordering::Relaxed)
                ),
                status: "pending".to_owned(),
                category: category.to_owned(),
                content: content.to_owned(),
                source: source.to_owned(),
                reference_status: String::new(),
                reference_summary: String::new(),
                created_at_ms: now,
                updated_at_ms: now,
            };
            normalize_knowledge_reference(&mut entry, &self.root);
            entries.push(KnowledgeEntry { ..entry });
        }
        write_knowledge_entries(&self.knowledge_path(), &entries)?;
        self.list_knowledge()
    }

    pub fn revalidate_knowledge(&self) -> EngineResult<Vec<KnowledgeEntry>> {
        let mut entries = read_knowledge_entries(&self.knowledge_path())?;
        let now = unix_time_ms();
        for entry in &mut entries {
            let previous_status = entry.reference_status.clone();
            let previous_summary = entry.reference_summary.clone();
            normalize_knowledge_reference(entry, &self.root);
            if entry.reference_status != previous_status
                || entry.reference_summary != previous_summary
            {
                entry.updated_at_ms = now;
            }
        }
        write_knowledge_entries(&self.knowledge_path(), &entries)?;
        self.list_knowledge()
    }

    pub fn approve_knowledge(&self, id: &str) -> EngineResult<Vec<KnowledgeEntry>> {
        self.update_knowledge_status(id, "approved")
    }

    pub fn reject_knowledge(&self, id: &str) -> EngineResult<Vec<KnowledgeEntry>> {
        self.update_knowledge_status(id, "rejected")
    }

    pub fn remove_knowledge(&self, id: &str) -> EngineResult<Vec<KnowledgeEntry>> {
        let mut entries = read_knowledge_entries(&self.knowledge_path())?;
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        if entries.len() == before {
            return Err(EngineError::config("Knowledge entry does not exist"));
        }
        write_knowledge_entries(&self.knowledge_path(), &entries)?;
        self.list_knowledge()
    }

    #[cfg(test)]
    pub fn create(
        &self,
        title: String,
        goal: String,
        spec: String,
        project: QuestProject,
    ) -> EngineResult<QuestDetail> {
        self.create_with_config(
            title,
            goal,
            spec,
            project,
            QuestMode::default(),
            QuestModelConfig::default(),
        )
    }

    pub fn create_with_config(
        &self,
        title: String,
        goal: String,
        spec: String,
        project: QuestProject,
        mode: QuestMode,
        model_config: QuestModelConfig,
    ) -> EngineResult<QuestDetail> {
        let now = unix_time_ms();
        let id = format!(
            "quest-{now}-{}",
            NEXT_QUEST_ID.fetch_add(1, Ordering::Relaxed)
        );
        let title = title.trim();
        if title.is_empty() {
            return Err(EngineError::config("Quest title must not be empty"));
        }
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(EngineError::config("Quest spec must not be empty"));
        }
        let mut record = QuestRecord {
            schema_version: 1,
            id: id.clone(),
            title: title.to_owned(),
            goal: goal.clone(),
            status: QuestStatus::Draft,
            project,
            mode,
            model_config,
            autonomy: QuestAutonomyPolicy::default(),
            created_at_ms: now,
            updated_at_ms: now,
            workspace_id: None,
            intent_path: default_intent_path(),
            spec_path: default_spec_path(),
            trace_path: default_trace_path(),
            artifact_links: default_artifact_links(),
            attached_knowledge_ids: Vec::new(),
            branch_of: None,
            branch_ids: Vec::new(),
            decisions: Vec::new(),
            checkpoints: Vec::new(),
            tasks: Vec::new(),
            next_action: QuestNextAction::default(),
            review: None,
        };
        refresh_next_action(&mut record);
        self.save_snapshot(&record)?;
        write_text(
            &self.quest_dir(&id).join("intent.md"),
            &intent_markdown(title, &goal),
        )?;
        write_text(&self.quest_dir(&id).join("spec.md"), spec)?;
        self.append_event(
            &id,
            "created",
            "AI generated Quest spec",
            serde_json::json!({
                "status": "draft",
                "mode": record.mode,
                "model_config": record.model_config,
                "autonomy": record.autonomy,
                "intent_path": "intent.md",
                "spec_path": "spec.md",
                "spec_bytes": spec.len()
            }),
        )?;
        self.get(&id)
    }

    pub fn branch(&self, id: &str, title: Option<&str>) -> EngineResult<QuestDetail> {
        validate_id(id)?;
        let source = self.get(id)?;
        let now = unix_time_ms();
        let branch_id = format!(
            "quest-{now}-{}",
            NEXT_QUEST_ID.fetch_add(1, Ordering::Relaxed)
        );
        let branch_title = title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} branch", source.record.title));
        let mut branch_record = QuestRecord {
            schema_version: source.record.schema_version,
            id: branch_id.clone(),
            title: branch_title.clone(),
            goal: source.record.goal.clone(),
            status: QuestStatus::Draft,
            project: source.record.project.clone(),
            mode: source.record.mode,
            model_config: source.record.model_config.clone(),
            autonomy: source.record.autonomy.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            workspace_id: None,
            intent_path: default_intent_path(),
            spec_path: default_spec_path(),
            trace_path: default_trace_path(),
            artifact_links: default_artifact_links(),
            attached_knowledge_ids: source.record.attached_knowledge_ids.clone(),
            branch_of: Some(source.record.id.clone()),
            branch_ids: Vec::new(),
            decisions: Vec::new(),
            checkpoints: Vec::new(),
            tasks: source.record.tasks.clone(),
            next_action: QuestNextAction::default(),
            review: None,
        };
        refresh_next_action(&mut branch_record);
        self.save_snapshot(&branch_record)?;
        write_text(
            &self.quest_dir(&branch_id).join("intent.md"),
            &source.intent,
        )?;
        write_text(&self.quest_dir(&branch_id).join("spec.md"), &source.spec)?;
        self.append_event(
            &branch_id,
            "branched",
            "Created Quest branch from existing Quest",
            serde_json::json!({
                "source_quest_id": source.record.id,
                "source_title": source.record.title,
            }),
        )?;

        let mut parent_record = source.record.clone();
        if !parent_record
            .branch_ids
            .iter()
            .any(|value| value == &branch_id)
        {
            parent_record.branch_ids.push(branch_id.clone());
        }
        parent_record.updated_at_ms = now;
        normalize_record_metadata(&mut parent_record);
        refresh_next_action(&mut parent_record);
        self.save_snapshot(&parent_record)?;
        self.append_event(
            id,
            "branch_created",
            "Created Quest branch",
            serde_json::json!({
                "branch_quest_id": branch_id,
                "branch_title": branch_title,
            }),
        )?;
        self.get(&branch_id)
    }

    pub fn quest_path(&self, id: &str) -> EngineResult<PathBuf> {
        validate_id(id)?;
        Ok(self.quest_dir(id))
    }

    pub fn get(&self, id: &str) -> EngineResult<QuestDetail> {
        validate_id(id)?;
        let dir = self.quest_dir(id);
        let mut record: QuestRecord = read_json(&dir.join("quest.json"))?;
        normalize_record_metadata(&mut record);
        refresh_next_action(&mut record);
        let intent = read_text_with_fallback(&dir.join("intent.md"), || {
            intent_markdown(&record.title, &record.goal)
        })?;
        let spec =
            fs::read_to_string(dir.join("spec.md")).map_err(|source| EngineError::Filesystem {
                path: dir.join("spec.md"),
                source,
            })?;
        let attached_knowledge = self.attached_knowledge_entries(&record)?;
        let events = read_events(&dir.join("events.jsonl"))?;
        Ok(QuestDetail {
            record,
            intent,
            spec,
            attached_knowledge,
            events,
        })
    }

    pub fn update_knowledge_context(
        &self,
        id: &str,
        knowledge_ids: Vec<String>,
    ) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let approved = read_knowledge_entries(&self.knowledge_path())?
            .into_iter()
            .filter(|entry| entry.status == "approved")
            .collect::<Vec<_>>();
        let mut normalized = Vec::new();
        for knowledge_id in knowledge_ids {
            if normalized.iter().any(|existing| existing == &knowledge_id) {
                continue;
            }
            if !approved.iter().any(|entry| entry.id == knowledge_id) {
                return Err(EngineError::config(
                    "Only approved Knowledge entries can be attached to a Quest",
                ));
            }
            normalized.push(knowledge_id);
        }
        detail.record.attached_knowledge_ids = normalized.clone();
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "knowledge_context_updated",
            "Quest Knowledge context updated",
            serde_json::json!({ "knowledge_ids": normalized }),
        )?;
        self.get(id)
    }

    pub fn update_execution_config(
        &self,
        id: &str,
        mode: QuestMode,
        model_config: QuestModelConfig,
        autonomy: Option<QuestAutonomyPolicy>,
    ) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        if !matches!(
            detail.record.status,
            QuestStatus::Draft
                | QuestStatus::Clarifying
                | QuestStatus::Specified
                | QuestStatus::Planning
                | QuestStatus::WaitingForUser
                | QuestStatus::Blocked
        ) {
            return Err(EngineError::config(
                "Quest mode and model can only be changed before execution, while waiting, or while blocked",
            ));
        }
        detail.record.mode = mode;
        detail.record.model_config = normalize_model_config(model_config);
        if let Some(autonomy) = autonomy {
            detail.record.autonomy = autonomy;
        }
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "execution_config_updated",
            "Quest execution mode and model updated",
            serde_json::json!({
                "mode": detail.record.mode,
                "model_config": detail.record.model_config,
                "autonomy": detail.record.autonomy,
            }),
        )?;
        self.get(id)
    }

    pub fn update_intent(&self, id: &str, intent: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        if !matches!(
            detail.record.status,
            QuestStatus::Draft
                | QuestStatus::Clarifying
                | QuestStatus::Specified
                | QuestStatus::Planning
                | QuestStatus::WaitingForUser
                | QuestStatus::Blocked
        ) {
            return Err(EngineError::config(
                "Quest intent can only be edited before execution, while waiting, or while blocked",
            ));
        }
        write_text(&self.quest_dir(id).join("intent.md"), intent)?;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "intent_updated",
            "Quest intent updated",
            serde_json::json!({ "bytes": intent.len(), "path": "intent.md" }),
        )?;
        self.get(id)
    }

    pub fn update_spec(&self, id: &str, spec: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        if !matches!(
            detail.record.status,
            QuestStatus::Draft
                | QuestStatus::Clarifying
                | QuestStatus::Specified
                | QuestStatus::Planning
                | QuestStatus::WaitingForUser
                | QuestStatus::Blocked
        ) {
            return Err(EngineError::config(
                "Quest spec can only be edited before execution, while waiting, or while blocked",
            ));
        }
        write_text(&self.quest_dir(id).join("spec.md"), spec)?;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "spec_updated",
            "Quest spec updated",
            serde_json::json!({ "bytes": spec.len() }),
        )?;
        self.get(id)
    }

    pub fn replace_tasks(&self, id: &str, tasks: Vec<QuestTask>) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let mut normalized = normalize_quest_tasks(tasks);
        detail.record.tasks.clear();
        detail.record.tasks.append(&mut normalized);
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "tasks_updated",
            "Quest task list updated",
            serde_json::json!({
                "task_count": detail.record.tasks.len(),
                "done_count": detail.record.tasks.iter().filter(|task| task.done).count(),
            }),
        )?;
        self.get(id)
    }

    pub fn rename(&self, id: &str, title: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(EngineError::config("Quest title must not be empty"));
        }
        detail.record.title = title.to_owned();
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "renamed",
            "Quest renamed",
            serde_json::json!({ "title": title }),
        )?;
        self.get(id)
    }

    pub fn transition(&self, id: &str, next: QuestStatus) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let current = detail.record.status;
        if current == next {
            return Ok(detail);
        }
        if !current.can_transition_to(next) {
            return Err(EngineError::config(format!(
                "invalid Quest transition: {current:?} -> {next:?}"
            )));
        }
        detail.record.status = next;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        let event_kind = if matches!(next, QuestStatus::Completed | QuestStatus::Archived) {
            "user_decision"
        } else {
            "status_changed"
        };
        self.append_event(
            id,
            event_kind,
            &format!("Quest moved from {current:?} to {next:?}"),
            serde_json::json!({ "from": current, "to": next }),
        )?;
        self.get(id)
    }

    pub fn delete(&self, id: &str) -> EngineResult<()> {
        validate_id(id)?;
        let dir = self.quest_dir(id);
        if !dir.exists() {
            return Err(EngineError::config("Quest does not exist"));
        }
        fs::remove_dir_all(&dir).map_err(|source| EngineError::Filesystem { path: dir, source })
    }

    pub fn mock_execute(&self, id: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        if detail.record.status == QuestStatus::Draft {
            detail = self.transition(id, QuestStatus::Specified)?;
        }
        if !matches!(
            detail.record.status,
            QuestStatus::Specified | QuestStatus::WaitingForUser | QuestStatus::Blocked
        ) {
            return Err(EngineError::config(
                "Quest must be specified, waiting, or blocked before execution",
            ));
        }
        let detail = self.transition(id, QuestStatus::Prepared)?;
        let mut detail = self.transition(&detail.record.id, QuestStatus::Running)?;
        let workspace_id = format!("workspace-{}", detail.record.id);
        detail.record.workspace_id = Some(workspace_id.clone());
        detail.record.updated_at_ms = unix_time_ms();
        self.save_snapshot(&detail.record)?;
        let mut detail = self.record_checkpoint(
            id,
            "mock-workspace",
            "Mock workspace checkpoint",
            "Captured the mocked isolated workspace before producing a review bundle.",
            Some(workspace_id),
            None,
        )?;

        self.append_event(
            id,
            "plan",
            "Execution plan prepared",
            serde_json::json!({ "slices": 3, "mocked": true }),
        )?;
        self.append_event(
            id,
            "file_read",
            "Inspected editor shell and Quest integration points",
            serde_json::json!({ "files": ["editor/src/renderer/App.tsx", "editor/src-tauri/src/lib.rs"] }),
        )?;
        self.append_event(
            id,
            "file_edit",
            "Prepared isolated workspace changes",
            serde_json::json!({ "path": "editor/src/renderer/pages/QuestPage.tsx", "additions": 248, "deletions": 0 }),
        )?;
        self.append_event(
            id,
            "validation",
            "Frontend type-check and Rust tests passed",
            serde_json::json!({ "status": "passed", "mocked": true }),
        )?;

        detail.record.review = Some(QuestReview {
            summary: "Mock execution produced a reviewable Quest shell change bundle.".to_owned(),
            changed_files: vec![
                ChangedFile {
                    path: "editor/src/renderer/pages/QuestPage.tsx".to_owned(),
                    additions: 248,
                    deletions: 0,
                    status: "added".to_owned(),
                    diff: "--- a/editor/src/renderer/pages/QuestPage.tsx\n+++ b/editor/src/renderer/pages/QuestPage.tsx\n+mock Quest page changes\n".to_owned(),
                },
                ChangedFile {
                    path: "editor/src/renderer/App.tsx".to_owned(),
                    additions: 24,
                    deletions: 4,
                    status: "modified".to_owned(),
                    diff: "--- a/editor/src/renderer/App.tsx\n+++ b/editor/src/renderer/App.tsx\n+mock App integration changes\n".to_owned(),
                },
            ],
            transaction_groups: vec![
                QuestTransactionGroup {
                    id: "quest-ui".to_owned(),
                    label: "Quest workspace UI".to_owned(),
                    summary: "Adds the Quest cockpit page and review surfaces.".to_owned(),
                    files: vec!["editor/src/renderer/pages/QuestPage.tsx".to_owned()],
                    risk: "low".to_owned(),
                },
                QuestTransactionGroup {
                    id: "app-integration".to_owned(),
                    label: "Editor shell integration".to_owned(),
                    summary: "Wires Quest navigation into the main editor app.".to_owned(),
                    files: vec!["editor/src/renderer/App.tsx".to_owned()],
                    risk: "low".to_owned(),
                },
            ],
            exploration_attempts: vec![
                QuestExplorationAttempt {
                    id: "simple-cockpit".to_owned(),
                    label: "Simple cockpit path".to_owned(),
                    summary: "Prioritizes durable Quest state, timeline, and review before richer branching.".to_owned(),
                    outcome: "selected".to_owned(),
                    artifact_path: "explorations/simple-cockpit.md".to_owned(),
                    selected: true,
                },
                QuestExplorationAttempt {
                    id: "full-orchestrator".to_owned(),
                    label: "Full orchestrator path".to_owned(),
                    summary: "Defers multi-agent orchestration until isolated workspace validation is reliable.".to_owned(),
                    outcome: "deferred".to_owned(),
                    artifact_path: "explorations/full-orchestrator.md".to_owned(),
                    selected: false,
                },
            ],
            findings: Vec::new(),
            validations: vec![
                ValidationResult::new(
                    "Frontend build",
                    "passed",
                    "TypeScript and Vite build completed.",
                ),
                ValidationResult::new(
                    "Quest lifecycle",
                    "passed",
                    "Legal transitions and persistence were verified.",
                ),
            ],
            unresolved_issues: vec![
                "Execution is mocked; isolated agent workspace integration belongs to Phase 2."
                    .to_owned(),
            ],
            next_actions: vec![
                QuestReviewAction::new(
                    "apply-selected",
                    "Apply selected changes",
                    "apply_selected",
                ),
                QuestReviewAction::with_target(
                    "quick-fix-mock-gap",
                    "Request quick fix",
                    "quick_fix",
                    "Execution is mocked; isolated agent workspace integration belongs to Phase 2.",
                ),
                QuestReviewAction::new(
                    "request-revision",
                    "Request revision",
                    "revise",
                ),
                QuestReviewAction::new("branch-result", "Branch from result", "branch"),
            ],
            project_fingerprint: None,
            metrics: QuestReviewMetrics {
                intent_to_first_action_ms: Some(0),
                tool_call_latency_ms: Some(0),
                validator_turnaround_ms: Some(0),
                context_relevance_score: Some(0.75),
                failed_action_recovery_rate: Some(1.0),
                review_evidence_quality_score: Some(0.7),
                isolated_attempt_count: 2,
                validation_count: 2,
                validation_failure_count: 0,
                baseline_changed_file_count: 0,
                notes: vec!["Mock execution reports synthetic capability-extraction metrics.".to_owned()],
            },
            risk: "low".to_owned(),
        });
        self.write_exploration_attempt(
            id,
            "simple-cockpit",
            "Simple cockpit path",
            "Selected because it produces inspectable Quest state, review, and apply controls with lower integration risk.",
            true,
        )?;
        self.write_exploration_attempt(
            id,
            "full-orchestrator",
            "Full orchestrator path",
            "Deferred because it needs stronger workspace validation, richer command tools, and branch comparison before active-project apply.",
            false,
        )?;
        detail.record.status = QuestStatus::ReadyForReview;
        detail.record.updated_at_ms = unix_time_ms();
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "review_ready",
            "Quest is ready for final review",
            serde_json::json!({ "risk": "low" }),
        )?;
        self.get(id)
    }

    pub fn set_workspace_id(&self, id: &str, workspace_id: String) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        detail.record.workspace_id = Some(workspace_id);
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.get(id)
    }

    pub fn write_exploration_attempt(
        &self,
        id: &str,
        attempt_id: &str,
        label: &str,
        summary: &str,
        selected: bool,
    ) -> EngineResult<()> {
        validate_id(id)?;
        validate_artifact_id(attempt_id)?;
        let path = format!("explorations/{attempt_id}.md");
        let content = format!(
            "# {label}\n\n## Summary\n\n{summary}\n\n## Decision\n\n{}\n",
            if selected {
                "Selected for the review bundle."
            } else {
                "Preserved as an alternative attempt."
            }
        );
        write_text(&self.quest_dir(id).join(&path), &content)?;
        let mut detail = self.get(id)?;
        if !detail
            .record
            .artifact_links
            .iter()
            .any(|artifact| artifact.kind == "exploration" && artifact.path == path)
        {
            detail.record.artifact_links.push(QuestArtifactLink {
                kind: "exploration".to_owned(),
                label: label.to_owned(),
                path: path.clone(),
            });
            detail.record.updated_at_ms = unix_time_ms();
            normalize_record_metadata(&mut detail.record);
            refresh_next_action(&mut detail.record);
            self.save_snapshot(&detail.record)?;
        }
        self.append_event(
            id,
            "alternative",
            label,
            serde_json::json!({
                "attempt_id": attempt_id,
                "path": path,
                "selected": selected,
                "summary": summary,
            }),
        )?;
        Ok(())
    }

    pub fn record_checkpoint(
        &self,
        id: &str,
        checkpoint_id: &str,
        label: &str,
        summary: &str,
        workspace_id: Option<String>,
        project_fingerprint: Option<String>,
    ) -> EngineResult<QuestDetail> {
        validate_id(id)?;
        validate_artifact_id(checkpoint_id)?;
        let label = label.trim();
        if label.is_empty() {
            return Err(EngineError::config(
                "Quest checkpoint label must not be empty",
            ));
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(EngineError::config(
                "Quest checkpoint summary must not be empty",
            ));
        }
        let artifact_path = format!("checkpoints/{checkpoint_id}.md");
        let content = format!(
            "# {label}\n\n## Summary\n\n{summary}\n\n## Workspace\n\n{}\n\n## Project fingerprint\n\n{}\n",
            workspace_id.as_deref().unwrap_or("not recorded"),
            project_fingerprint.as_deref().unwrap_or("not recorded")
        );
        write_text(&self.quest_dir(id).join(&artifact_path), &content)?;

        let mut detail = self.get(id)?;
        let now = unix_time_ms();
        detail
            .record
            .checkpoints
            .retain(|checkpoint| checkpoint.id != checkpoint_id);
        detail.record.checkpoints.push(QuestCheckpoint {
            id: checkpoint_id.to_owned(),
            label: label.to_owned(),
            summary: summary.to_owned(),
            timestamp_ms: now,
            workspace_id: workspace_id.clone(),
            artifact_path: Some(artifact_path.clone()),
            project_fingerprint: project_fingerprint.clone(),
        });
        if !detail
            .record
            .artifact_links
            .iter()
            .any(|artifact| artifact.kind == "checkpoint" && artifact.path == artifact_path)
        {
            detail.record.artifact_links.push(QuestArtifactLink {
                kind: "checkpoint".to_owned(),
                label: label.to_owned(),
                path: artifact_path.clone(),
            });
        }
        detail.record.updated_at_ms = now;
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "checkpoint",
            label,
            serde_json::json!({
                "checkpoint_id": checkpoint_id,
                "path": artifact_path,
                "summary": summary,
                "workspace_id": workspace_id,
                "project_fingerprint": project_fingerprint,
            }),
        )?;
        self.get(id)
    }

    pub fn write_review_finding(
        &self,
        id: &str,
        finding_id: &str,
        title: &str,
        summary: &str,
        severity: &str,
        source: Option<&str>,
    ) -> EngineResult<QuestReviewFinding> {
        validate_id(id)?;
        validate_artifact_id(finding_id)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(EngineError::config(
                "Quest review finding title must not be empty",
            ));
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(EngineError::config(
                "Quest review finding summary must not be empty",
            ));
        }
        let severity = severity.trim();
        let severity = if severity.is_empty() {
            "medium"
        } else {
            severity
        };
        let artifact_path = format!("findings/{finding_id}.md");
        let content = format!(
            "# {title}\n\n## Severity\n\n{severity}\n\n## Finding\n\n{summary}\n\n## Source\n\n{}\n",
            source.unwrap_or("review")
        );
        write_text(&self.quest_dir(id).join(&artifact_path), &content)?;

        let mut detail = self.get(id)?;
        if !detail
            .record
            .artifact_links
            .iter()
            .any(|artifact| artifact.kind == "review_finding" && artifact.path == artifact_path)
        {
            detail.record.artifact_links.push(QuestArtifactLink {
                kind: "review_finding".to_owned(),
                label: title.to_owned(),
                path: artifact_path.clone(),
            });
            detail.record.updated_at_ms = unix_time_ms();
            normalize_record_metadata(&mut detail.record);
            refresh_next_action(&mut detail.record);
            self.save_snapshot(&detail.record)?;
        }
        self.append_event(
            id,
            "review_finding",
            title,
            serde_json::json!({
                "finding_id": finding_id,
                "path": artifact_path,
                "severity": severity,
                "summary": summary,
                "source": source,
            }),
        )?;

        Ok(QuestReviewFinding {
            id: finding_id.to_owned(),
            title: title.to_owned(),
            summary: summary.to_owned(),
            severity: severity.to_owned(),
            artifact_path: Some(artifact_path),
            source: source.map(str::to_owned),
        })
    }

    pub fn write_thinking_trace(
        &self,
        id: &str,
        trace_id: &str,
        label: &str,
        thinking: &str,
    ) -> EngineResult<()> {
        validate_id(id)?;
        validate_artifact_id(trace_id)?;
        let label = label.trim();
        if label.is_empty() {
            return Err(EngineError::config(
                "Quest thinking trace label must not be empty",
            ));
        }
        let thinking = thinking.trim();
        if thinking.is_empty() {
            return Ok(());
        }
        let artifact_path = format!("thinking/{trace_id}.md");
        let content = format!("# {label}\n\n## Provider thinking\n\n{thinking}\n");
        write_text(&self.quest_dir(id).join(&artifact_path), &content)?;

        let mut detail = self.get(id)?;
        if !detail
            .record
            .artifact_links
            .iter()
            .any(|artifact| artifact.kind == "thinking" && artifact.path == artifact_path)
        {
            detail.record.artifact_links.push(QuestArtifactLink {
                kind: "thinking".to_owned(),
                label: label.to_owned(),
                path: artifact_path.clone(),
            });
            detail.record.updated_at_ms = unix_time_ms();
            normalize_record_metadata(&mut detail.record);
            refresh_next_action(&mut detail.record);
            self.save_snapshot(&detail.record)?;
        }
        self.append_event(
            id,
            "thinking",
            label,
            serde_json::json!({
                "trace_id": trace_id,
                "path": artifact_path,
                "bytes": thinking.len(),
                "preview": thinking.chars().take(600).collect::<String>(),
            }),
        )?;
        Ok(())
    }

    pub fn set_review(
        &self,
        id: &str,
        status: QuestStatus,
        review: QuestReview,
    ) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        detail.record.review = Some(review);
        detail.record.status = status;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.get(id)
    }

    pub fn record_decision(
        &self,
        id: &str,
        kind: &str,
        summary: &str,
        files: Vec<String>,
    ) -> EngineResult<QuestDetail> {
        self.record_decision_with_rollback(id, kind, summary, files, None)
    }

    pub fn record_decision_with_rollback(
        &self,
        id: &str,
        kind: &str,
        summary: &str,
        files: Vec<String>,
        rollback_id: Option<String>,
    ) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let decision = QuestDecision {
            kind: kind.to_owned(),
            summary: summary.to_owned(),
            files: files.clone(),
            timestamp_ms: unix_time_ms(),
            rollback_id: rollback_id.clone(),
        };
        detail.record.decisions.push(decision);
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "user_decision",
            summary,
            serde_json::json!({ "kind": kind, "files": files, "rollback_id": rollback_id }),
        )?;
        self.get(id)
    }

    pub fn add_user_note(&self, id: &str, kind: &str, message: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let message = message.trim();
        if message.is_empty() {
            return Err(EngineError::config("Quest note must not be empty"));
        }
        let event_kind = match kind {
            "steer" => "user_steering",
            "clarify" => "clarification_answer",
            "manual_intervention" => "manual_intervention_result",
            "pause" => "manual_intervention_request",
            _ => "user_message",
        };
        let summary = match kind {
            "steer" => "User steered Quest execution",
            "clarify" => "User answered Quest clarification",
            "manual_intervention" => "User recorded manual Editor intervention",
            "pause" => "Quest paused for manual intervention",
            _ => "User added Quest note",
        };
        let next_status = if kind == "pause" && detail.record.status != QuestStatus::WaitingForUser
        {
            Some(QuestStatus::WaitingForUser)
        } else if kind == "clarify"
            && matches!(
                detail.record.status,
                QuestStatus::Clarifying | QuestStatus::WaitingForUser
            )
        {
            Some(QuestStatus::Specified)
        } else {
            None
        };
        if let Some(status) = next_status {
            detail.record.status = status;
            detail.record.updated_at_ms = unix_time_ms();
            normalize_record_metadata(&mut detail.record);
            refresh_next_action(&mut detail.record);
            self.save_snapshot(&detail.record)?;
        }
        self.append_event(
            id,
            event_kind,
            summary,
            serde_json::json!({ "message": message }),
        )?;
        self.get(id)
    }

    pub fn request_quick_fix(&self, id: &str, issue: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        let issue = issue.trim();
        if issue.is_empty() {
            return Err(EngineError::config("Quick-fix issue must not be empty"));
        }
        if !matches!(
            detail.record.status,
            QuestStatus::ReadyForReview | QuestStatus::Blocked | QuestStatus::WaitingForUser
        ) {
            return Err(EngineError::config(
                "Quick fix can only be requested from review, blocked, or waiting states",
            ));
        }
        detail.record.status = QuestStatus::Repairing;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "quick_fix_requested",
            "User requested quick fix for review issue",
            serde_json::json!({ "issue": issue }),
        )?;
        self.get(id)
    }

    pub fn continue_quest(&self, id: &str, reason: &str) -> EngineResult<QuestDetail> {
        let mut detail = self.get(id)?;
        if !matches!(
            detail.record.status,
            QuestStatus::WaitingForUser | QuestStatus::Blocked | QuestStatus::ReadyForReview
        ) {
            return Err(EngineError::config(
                "Quest can only continue from waiting, blocked, or review states",
            ));
        }
        let reason = reason.trim();
        let summary = if reason.is_empty() {
            "Continue Quest from current evidence"
        } else {
            reason
        };
        detail.record.status = QuestStatus::Running;
        detail.record.updated_at_ms = unix_time_ms();
        normalize_record_metadata(&mut detail.record);
        refresh_next_action(&mut detail.record);
        self.save_snapshot(&detail.record)?;
        self.append_event(
            id,
            "continued",
            "Quest continued from current evidence",
            serde_json::json!({ "reason": summary }),
        )?;
        self.append_event(
            id,
            "user_decision",
            summary,
            serde_json::json!({ "kind": "continue", "files": [] }),
        )?;
        self.get(id)
    }

    pub fn cancel(&self, id: &str, reason: &str) -> EngineResult<QuestDetail> {
        let detail = self.get(id)?;
        if matches!(
            detail.record.status,
            QuestStatus::Completed | QuestStatus::Archived | QuestStatus::Canceled
        ) {
            return Err(EngineError::config(
                "Only active, waiting, blocked, or review Quests can be canceled",
            ));
        }
        let summary = if reason.trim().is_empty() {
            "Canceled Quest"
        } else {
            reason.trim()
        };
        let _ = self.record_decision(id, "cancel", summary, Vec::new())?;
        self.transition(id, QuestStatus::Canceled)
    }

    pub fn reopen(&self, id: &str, reason: &str) -> EngineResult<QuestDetail> {
        let detail = self.get(id)?;
        if !matches!(
            detail.record.status,
            QuestStatus::Archived | QuestStatus::Canceled | QuestStatus::Completed
        ) {
            return Err(EngineError::config(
                "Only archived, canceled, or completed Quests can be reopened",
            ));
        }
        let summary = if reason.trim().is_empty() {
            "Reopened Quest"
        } else {
            reason.trim()
        };
        let _ = self.record_decision(id, "reopen", summary, Vec::new())?;
        self.transition(id, QuestStatus::Specified)
    }

    pub fn append_timeline_event(
        &self,
        quest_id: &str,
        kind: &str,
        summary: &str,
        details: Value,
    ) -> EngineResult<()> {
        validate_id(quest_id)?;
        self.append_event(quest_id, kind, summary, details)
    }

    fn quest_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    fn knowledge_path(&self) -> PathBuf {
        self.root.join("knowledge.json")
    }

    fn attached_knowledge_entries(
        &self,
        record: &QuestRecord,
    ) -> EngineResult<Vec<KnowledgeEntry>> {
        let entries = read_knowledge_entries(&self.knowledge_path())?;
        Ok(record
            .attached_knowledge_ids
            .iter()
            .filter_map(|id| {
                entries
                    .iter()
                    .find(|entry| entry.id == *id && entry.status == "approved")
                    .cloned()
            })
            .collect())
    }

    fn update_knowledge_status(&self, id: &str, status: &str) -> EngineResult<Vec<KnowledgeEntry>> {
        let mut entries = read_knowledge_entries(&self.knowledge_path())?;
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| EngineError::config("Knowledge entry does not exist"))?;
        entry.status = status.to_owned();
        entry.updated_at_ms = unix_time_ms();
        write_knowledge_entries(&self.knowledge_path(), &entries)?;
        self.list_knowledge()
    }

    fn save_snapshot(&self, record: &QuestRecord) -> EngineResult<()> {
        let path = self.quest_dir(&record.id).join("quest.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let output = serde_json::to_string_pretty(record)
            .map_err(|error| EngineError::other(error.to_string()))?;
        write_text(&path, &output)
    }

    fn append_event(
        &self,
        quest_id: &str,
        kind: &str,
        summary: &str,
        details: Value,
    ) -> EngineResult<()> {
        let path = self.quest_dir(quest_id).join("events.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let timestamp_ms = unix_time_ms();
        let event = QuestEvent {
            id: format!(
                "event-{timestamp_ms}-{}",
                NEXT_QUEST_ID.fetch_add(1, Ordering::Relaxed)
            ),
            quest_id: quest_id.to_owned(),
            timestamp_ms,
            kind: kind.to_owned(),
            summary: summary.to_owned(),
            details,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
        let line =
            serde_json::to_string(&event).map_err(|error| EngineError::other(error.to_string()))?;
        writeln!(file, "{line}").map_err(|source| EngineError::Filesystem { path, source })
    }
}

fn validate_id(id: &str) -> EngineResult<()> {
    if id.starts_with("quest-")
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        Ok(())
    } else {
        Err(EngineError::config("invalid Quest ID"))
    }
}

fn validate_artifact_id(id: &str) -> EngineResult<()> {
    if !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        Ok(())
    } else {
        Err(EngineError::config("invalid Quest artifact ID"))
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> EngineResult<T> {
    let input = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&input).map_err(|error| EngineError::config(error.to_string()))
}

fn read_events(path: &Path) -> EngineResult<Vec<QuestEvent>> {
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

fn read_knowledge_entries(path: &Path) -> EngineResult<Vec<KnowledgeEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(path)
}

fn write_knowledge_entries(path: &Path, entries: &[KnowledgeEntry]) -> EngineResult<()> {
    let output = serde_json::to_string_pretty(entries)
        .map_err(|error| EngineError::other(error.to_string()))?;
    write_text(path, &output)
}

fn default_knowledge_reference_status() -> String {
    "unchecked".to_owned()
}

fn default_knowledge_reference_summary() -> String {
    "Reference has not been validated.".to_owned()
}

fn normalize_knowledge_reference(entry: &mut KnowledgeEntry, quest_root: &Path) {
    let source = entry.source.trim();
    if source.is_empty() || source == "manual" {
        entry.reference_status = "unverified".to_owned();
        entry.reference_summary = "Manual Knowledge has no machine-checkable source.".to_owned();
        return;
    }
    if source.starts_with("quest-") {
        if validate_id(source).is_ok() && quest_root.join(source).join("quest.json").is_file() {
            entry.reference_status = "valid".to_owned();
            entry.reference_summary =
                format!("Source Quest `{source}` exists in the local Quest store.");
        } else {
            entry.reference_status = "missing".to_owned();
            entry.reference_summary =
                format!("Source Quest `{source}` is missing from the local Quest store.");
        }
        return;
    }

    let relative = Path::new(source);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        entry.reference_status = "invalid".to_owned();
        entry.reference_summary =
            "Source reference must be a Quest ID, manual source, or relative path.".to_owned();
        return;
    }
    let candidate = quest_root.join(relative);
    if candidate.exists() {
        entry.reference_status = "valid".to_owned();
        entry.reference_summary =
            format!("Source path `{source}` exists in the local Quest store.");
    } else {
        entry.reference_status = "missing".to_owned();
        entry.reference_summary =
            format!("Source path `{source}` is missing from the local Quest store.");
    }
}

fn normalize_knowledge_content(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub fn transaction_groups_from_changed_files(
    changed_files: &[ChangedFile],
) -> Vec<QuestTransactionGroup> {
    changed_files
        .iter()
        .map(|file| QuestTransactionGroup {
            id: transaction_group_id_for_path(&file.path),
            label: file.path.clone(),
            summary: format!("{} reviewed change for {}", file.status, file.path),
            files: vec![file.path.clone()],
            risk: "low".to_owned(),
        })
        .collect()
}

fn transaction_group_id_for_path(path: &str) -> String {
    let normalized = path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let trimmed = normalized.trim_matches('-');
    if trimmed.is_empty() {
        "change".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn read_text_with_fallback(path: &Path, fallback: impl FnOnce() -> String) -> EngineResult<String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(fallback()),
        Err(source) => Err(EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_text(path: &Path, content: &str) -> EngineResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, content).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn default_intent_path() -> String {
    "intent.md".to_owned()
}

fn default_spec_path() -> Option<String> {
    Some("spec.md".to_owned())
}

fn default_trace_path() -> String {
    "events.jsonl".to_owned()
}

fn default_quest_thinking_effort() -> String {
    "medium".to_owned()
}

fn default_true() -> bool {
    true
}

fn default_artifact_links() -> Vec<QuestArtifactLink> {
    vec![
        QuestArtifactLink {
            kind: "intent".to_owned(),
            label: "Quest intent".to_owned(),
            path: default_intent_path(),
        },
        QuestArtifactLink {
            kind: "spec".to_owned(),
            label: "Quest spec".to_owned(),
            path: "spec.md".to_owned(),
        },
        QuestArtifactLink {
            kind: "trace".to_owned(),
            label: "Timeline trace".to_owned(),
            path: default_trace_path(),
        },
    ]
}

fn normalize_record_metadata(record: &mut QuestRecord) {
    record.model_config = normalize_model_config(record.model_config.clone());
    record.autonomy.active_project_apply_requires_approval = true;
    if record.intent_path.trim().is_empty() {
        record.intent_path = default_intent_path();
    }
    if record.trace_path.trim().is_empty() {
        record.trace_path = default_trace_path();
    }
    if record.spec_path.as_deref().is_some_and(str::is_empty) {
        record.spec_path = default_spec_path();
    }
    if record.artifact_links.is_empty() {
        record.artifact_links = default_artifact_links();
    }
    record.branch_ids.sort();
    record.branch_ids.dedup();
    record
        .checkpoints
        .sort_by(|left, right| left.timestamp_ms.cmp(&right.timestamp_ms));
    record
        .checkpoints
        .dedup_by(|left, right| left.id == right.id);
    record.tasks = normalize_quest_tasks(std::mem::take(&mut record.tasks));
}

fn normalize_quest_tasks(tasks: Vec<QuestTask>) -> Vec<QuestTask> {
    let mut normalized = Vec::new();
    for (index, task) in tasks.into_iter().enumerate() {
        let title = task.title.trim();
        if title.is_empty() {
            continue;
        }
        let id = task.id.trim();
        let id = if !id.is_empty()
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            id.to_owned()
        } else {
            format!("task-{}", index + 1)
        };
        if normalized
            .iter()
            .any(|existing: &QuestTask| existing.id == id)
        {
            normalized.push(QuestTask {
                id: format!("task-{}", index + 1),
                title: title.chars().take(160).collect(),
                done: task.done,
            });
        } else {
            normalized.push(QuestTask {
                id,
                title: title.chars().take(160).collect(),
                done: task.done,
            });
        }
    }
    normalized
}

fn normalize_model_config(mut config: QuestModelConfig) -> QuestModelConfig {
    config.provider = config.provider.trim().to_owned();
    if config.provider.is_empty() {
        config.provider = "inherit".to_owned();
    }
    config.model = config.model.trim().to_owned();
    if config.max_tokens == 0 {
        config.max_tokens = 4096;
    }
    config.thinking_effort = match config.thinking_effort.trim() {
        "off" | "low" | "medium" | "high" => config.thinking_effort.trim().to_owned(),
        _ => default_quest_thinking_effort(),
    };
    config
}

fn intent_markdown(title: &str, goal: &str) -> String {
    format!("# {title}\n\n## Goal\n\n{goal}\n")
}

fn refresh_next_action(record: &mut QuestRecord) {
    record.next_action = match record.status {
        QuestStatus::Draft => QuestNextAction {
            label: "Capture intent".to_owned(),
            reason: "The Quest has a durable goal and needs a brief or spec before work starts."
                .to_owned(),
        },
        QuestStatus::Clarifying => QuestNextAction {
            label: "Answer clarification".to_owned(),
            reason: "The orchestrator needs sharper constraints before selecting a path."
                .to_owned(),
        },
        QuestStatus::Specified => QuestNextAction {
            label: "Prepare isolated workspace".to_owned(),
            reason: "The spec is sufficient for execution; active project mutation remains gated."
                .to_owned(),
        },
        QuestStatus::Planning => QuestNextAction {
            label: "Review plan".to_owned(),
            reason: "The orchestrator is shaping slices, validation, and risk controls.".to_owned(),
        },
        QuestStatus::Prepared => QuestNextAction {
            label: "Run execution segment".to_owned(),
            reason: "Snapshot, workspace, and policy state are ready for isolated work.".to_owned(),
        },
        QuestStatus::Running => QuestNextAction {
            label: "Wait for workspace events".to_owned(),
            reason: "The orchestrator is reading, editing, or generating artifacts in isolation."
                .to_owned(),
        },
        QuestStatus::WaitingForUser => QuestNextAction {
            label: "User input required".to_owned(),
            reason: "Progress is paused for clarification, approval, or manual intervention."
                .to_owned(),
        },
        QuestStatus::Validating => QuestNextAction {
            label: "Collect validation evidence".to_owned(),
            reason: "Checks are running before the result can be reviewed or repaired.".to_owned(),
        },
        QuestStatus::Repairing => QuestNextAction {
            label: "Repair validation findings".to_owned(),
            reason: "The orchestrator is addressing actionable failures within policy bounds."
                .to_owned(),
        },
        QuestStatus::ReadyForReview => QuestNextAction {
            label: "Review final decision".to_owned(),
            reason: "A result bundle is ready for apply, revision, archive, or rejection."
                .to_owned(),
        },
        QuestStatus::Applying => QuestNextAction {
            label: "Apply reviewed transaction".to_owned(),
            reason: "Approved workspace changes are entering the active project.".to_owned(),
        },
        QuestStatus::Completed => QuestNextAction {
            label: "Inspect completion record".to_owned(),
            reason: "The Quest has a final decision and durable history.".to_owned(),
        },
        QuestStatus::Blocked => QuestNextAction {
            label: "Resolve blocker or revise".to_owned(),
            reason: "The Quest preserved evidence for a blocker that needs a new action."
                .to_owned(),
        },
        QuestStatus::Canceled => QuestNextAction {
            label: "Reopen or archive canceled Quest".to_owned(),
            reason: "The Quest was canceled with a durable decision record.".to_owned(),
        },
        QuestStatus::Archived => QuestNextAction {
            label: "Reopen or keep archived".to_owned(),
            reason: "The Quest history is retained outside active work.".to_owned(),
        },
    };
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &Path) -> QuestProject {
        QuestProject {
            name: "Varg".to_owned(),
            path: root.to_path_buf(),
        }
    }

    #[test]
    fn quest_survives_store_recreation_and_keeps_history() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Build Quest shell".to_owned(),
                "Create a durable shell".to_owned(),
                "# Build Quest shell\n\n## Goal\n\nCreate a durable shell.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        store
            .update_spec(&created.record.id, "# Updated spec")
            .unwrap();

        let reopened = QuestStore::new(temp.path().join("quests"))
            .get(&created.record.id)
            .unwrap();
        assert!(reopened.intent.contains("Create a durable shell"));
        assert_eq!(reopened.spec, "# Updated spec");
        assert_eq!(reopened.events.len(), 2);
        assert_eq!(reopened.record.project.path, temp.path());
        assert_eq!(reopened.record.intent_path, "intent.md");
        assert_eq!(reopened.record.spec_path.as_deref(), Some("spec.md"));
        assert_eq!(reopened.record.trace_path, "events.jsonl");
    }

    #[test]
    fn quest_intent_is_a_separate_editable_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Build Quest shell".to_owned(),
                "Create a durable shell".to_owned(),
                "# Build Quest shell\n\n## Goal\n\nCreate a durable shell.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let updated = store
            .update_intent(
                &created.record.id,
                "# Updated intent\n\nManual constraints stay user-authored.",
            )
            .unwrap();

        assert!(updated.intent.contains("Manual constraints"));
        assert_eq!(updated.spec, created.spec);
        assert_eq!(
            fs::read_to_string(
                store
                    .quest_path(&created.record.id)
                    .unwrap()
                    .join("intent.md")
            )
            .unwrap(),
            "# Updated intent\n\nManual constraints stay user-authored."
        );
        assert!(
            updated
                .events
                .iter()
                .any(|event| event.kind == "intent_updated")
        );
    }

    #[test]
    fn quest_decisions_are_durable_timeline_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Review Quest shell".to_owned(),
                "Apply a reviewed bundle".to_owned(),
                "# Review Quest shell\n\n## Goal\n\nApply a reviewed bundle.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let decided = store
            .record_decision(
                &created.record.id,
                "partial_apply",
                "Partially applied one transaction group",
                vec!["src/main.rs".to_owned()],
            )
            .unwrap();

        assert_eq!(decided.record.decisions.len(), 1);
        assert_eq!(decided.record.decisions[0].kind, "partial_apply");
        assert_eq!(decided.record.decisions[0].files, ["src/main.rs"]);
        assert!(
            decided
                .events
                .iter()
                .any(|event| event.kind == "user_decision")
        );
    }

    #[test]
    fn quest_branch_copies_editable_artifacts_and_links_parent_child_history() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Branch Quest".to_owned(),
                "Try an alternate implementation path.".to_owned(),
                "# Branch Quest\n\n## Goal\n\nTry one path.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        let knowledge = store
            .propose_knowledge("workflow", "Prefer focused validation first.", "manual")
            .unwrap();
        let approved = store.approve_knowledge(&knowledge[0].id).unwrap();
        let source = store
            .update_knowledge_context(&created.record.id, vec![approved[0].id.clone()])
            .unwrap();
        store
            .update_intent(&source.record.id, "# Custom intent\n\nBranch this.")
            .unwrap();
        store
            .update_spec(
                &source.record.id,
                "# Custom spec\n\nKeep this editable artifact.",
            )
            .unwrap();

        let branch = store
            .branch(&source.record.id, Some("Alternate validation path"))
            .unwrap();
        let parent = store.get(&source.record.id).unwrap();
        let reloaded = QuestStore::new(temp.path().join("quests"))
            .get(&branch.record.id)
            .unwrap();

        assert_eq!(branch.record.status, QuestStatus::Draft);
        assert_eq!(
            branch.record.branch_of.as_deref(),
            Some(source.record.id.as_str())
        );
        assert!(parent.record.branch_ids.contains(&branch.record.id));
        assert_eq!(reloaded.intent, "# Custom intent\n\nBranch this.");
        assert_eq!(
            reloaded.spec,
            "# Custom spec\n\nKeep this editable artifact."
        );
        assert_eq!(
            reloaded.record.attached_knowledge_ids,
            [approved[0].id.clone()]
        );
        assert!(
            parent
                .events
                .iter()
                .any(|event| event.kind == "branch_created")
        );
        assert!(reloaded.events.iter().any(|event| event.kind == "branched"));
    }

    #[test]
    fn apply_decisions_can_link_rollback_snapshots() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Rollback Quest".to_owned(),
                "Apply with rollback".to_owned(),
                "# Rollback Quest\n\n## Goal\n\nApply with rollback.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let decided = store
            .record_decision_with_rollback(
                &created.record.id,
                "apply",
                "Applied one file",
                vec!["src/main.rs".to_owned()],
                Some("rollback-1".to_owned()),
            )
            .unwrap();

        assert_eq!(
            decided.record.decisions[0].rollback_id.as_deref(),
            Some("rollback-1")
        );
        assert!(
            decided
                .events
                .iter()
                .any(|event| event.details["rollback_id"] == "rollback-1")
        );
    }

    #[test]
    fn knowledge_entries_are_reviewable_and_deduplicated() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));

        let proposed = store
            .propose_knowledge(
                "architecture",
                "Quest metadata belongs to the editor profile.",
                "quest-1",
            )
            .unwrap();
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].status, "pending");

        let deduped = store
            .propose_knowledge(
                "architecture",
                "Quest metadata belongs to the editor profile.",
                "quest-2",
            )
            .unwrap();
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].source, "quest-2");

        let approved = store.approve_knowledge(&deduped[0].id).unwrap();
        assert_eq!(approved[0].status, "approved");

        let removed = store.remove_knowledge(&approved[0].id).unwrap();
        assert!(removed.is_empty());
    }

    #[test]
    fn knowledge_references_are_validated_when_proposed() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let quest = store
            .create(
                "Reference Source".to_owned(),
                "Create a checkable source".to_owned(),
                "# Reference Source\n\n## Goal\n\nCreate a checkable source.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let quest_sourced = store
            .propose_knowledge(
                "architecture",
                "Quest IDs can back memory proposals.",
                &quest.record.id,
            )
            .unwrap();
        assert_eq!(quest_sourced[0].reference_status, "valid");
        assert!(
            quest_sourced[0]
                .reference_summary
                .contains("exists in the local Quest store")
        );

        let manual = store
            .propose_knowledge("workflow", "User prefers focused tests.", "manual")
            .unwrap();
        let manual_entry = manual
            .iter()
            .find(|entry| entry.source == "manual")
            .unwrap();
        assert_eq!(manual_entry.reference_status, "unverified");
    }

    #[test]
    fn knowledge_revalidation_marks_missing_quest_sources() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let quest = store
            .create(
                "Reference Source".to_owned(),
                "Create a checkable source".to_owned(),
                "# Reference Source\n\n## Goal\n\nCreate a checkable source.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        let source_id = quest.record.id.clone();
        let proposed = store
            .propose_knowledge("architecture", "This depends on a Quest.", &source_id)
            .unwrap();
        assert_eq!(proposed[0].reference_status, "valid");

        store.delete(&source_id).unwrap();
        let revalidated = store.revalidate_knowledge().unwrap();

        assert_eq!(revalidated[0].reference_status, "missing");
        assert!(revalidated[0].reference_summary.contains("missing"));
    }

    #[test]
    fn quest_can_attach_only_approved_knowledge() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Use Knowledge".to_owned(),
                "Attach approved context".to_owned(),
                "# Use Knowledge\n\n## Goal\n\nAttach approved context.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        let pending = store
            .propose_knowledge("workflow", "Run focused tests first.", "manual")
            .unwrap();
        assert!(
            store
                .update_knowledge_context(&created.record.id, vec![pending[0].id.clone()])
                .is_err()
        );

        let approved = store.approve_knowledge(&pending[0].id).unwrap();
        let updated = store
            .update_knowledge_context(&created.record.id, vec![approved[0].id.clone()])
            .unwrap();

        assert_eq!(
            updated.record.attached_knowledge_ids,
            [approved[0].id.clone()]
        );
        assert_eq!(updated.attached_knowledge.len(), 1);
        assert_eq!(
            updated.attached_knowledge[0].content,
            "Run focused tests first."
        );
        assert!(
            updated
                .events
                .iter()
                .any(|event| event.kind == "knowledge_context_updated")
        );
    }

    #[test]
    fn quest_user_notes_are_typed_timeline_events() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Steer Quest".to_owned(),
                "Record steering".to_owned(),
                "# Steer Quest\n\n## Goal\n\nRecord steering.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let steered = store
            .add_user_note(
                &created.record.id,
                "steer",
                "Prefer a smaller validation slice.",
            )
            .unwrap();
        assert!(
            steered
                .events
                .iter()
                .any(|event| event.kind == "user_steering")
        );

        let paused = store
            .add_user_note(
                &created.record.id,
                "pause",
                "I will fix the scene manually.",
            )
            .unwrap();
        assert_eq!(paused.record.status, QuestStatus::WaitingForUser);
        assert!(
            paused
                .events
                .iter()
                .any(|event| event.kind == "manual_intervention_request")
        );
    }

    #[test]
    fn quest_continue_resumes_from_manual_intervention_with_durable_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Continue Quest".to_owned(),
                "Resume after manual work".to_owned(),
                "# Continue Quest\n\n## Goal\n\nResume after manual work.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        let paused = store
            .add_user_note(
                &created.record.id,
                "pause",
                "Adjust the scene transform manually.",
            )
            .unwrap();
        assert_eq!(paused.record.status, QuestStatus::WaitingForUser);

        let continued = store
            .continue_quest(&created.record.id, "Manual scene transform completed.")
            .unwrap();

        assert_eq!(continued.record.status, QuestStatus::Running);
        assert!(
            continued
                .events
                .iter()
                .any(|event| event.kind == "continued"
                    && event.summary == "Quest continued from current evidence")
        );
        assert!(continued.events.iter().any(|event| {
            event.kind == "user_decision"
                && event
                    .details
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "continue")
        }));
    }

    #[test]
    fn quest_continue_rejects_non_resume_states() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Continue Guard".to_owned(),
                "Reject early continue".to_owned(),
                "# Continue Guard\n\n## Goal\n\nReject early continue.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let error = store
            .continue_quest(&created.record.id, "Continue too early.")
            .unwrap_err();

        assert!(error.to_string().contains("waiting, blocked, or review"));
    }

    #[test]
    fn quick_fix_requests_are_durable_repair_events() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Repair Quest".to_owned(),
                "Fix review issue".to_owned(),
                "# Repair Quest\n\n## Goal\n\nFix review issue.".to_owned(),
                project(temp.path()),
            )
            .unwrap();
        let reviewed = store
            .set_review(
                &created.record.id,
                QuestStatus::ReadyForReview,
                QuestReview {
                    summary: "Needs repair".to_owned(),
                    changed_files: Vec::new(),
                    transaction_groups: Vec::new(),
                    exploration_attempts: Vec::new(),
                    findings: Vec::new(),
                    validations: Vec::new(),
                    unresolved_issues: vec!["Validation failed".to_owned()],
                    next_actions: Vec::new(),
                    project_fingerprint: None,
                    metrics: QuestReviewMetrics::default(),
                    risk: "medium".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(reviewed.record.status, QuestStatus::ReadyForReview);

        let repairing = store
            .request_quick_fix(&created.record.id, "Validation failed")
            .unwrap();

        assert_eq!(repairing.record.status, QuestStatus::Repairing);
        assert!(
            repairing
                .events
                .iter()
                .any(|event| event.kind == "quick_fix_requested")
        );
    }

    #[test]
    fn review_findings_are_durable_artifacts_and_timeline_events() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Finding Quest".to_owned(),
                "Record review finding".to_owned(),
                "# Finding Quest\n\n## Goal\n\nRecord review finding.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let finding = store
            .write_review_finding(
                &created.record.id,
                "validation-failed-0",
                "Validation failed: cargo check",
                "cargo check --quiet exited with status 101.",
                "high",
                Some("validation"),
            )
            .unwrap();
        let detail = store.get(&created.record.id).unwrap();

        assert_eq!(
            finding.artifact_path.as_deref(),
            Some("findings/validation-failed-0.md")
        );
        assert!(
            store
                .quest_path(&created.record.id)
                .unwrap()
                .join("findings/validation-failed-0.md")
                .is_file()
        );
        assert!(detail.record.artifact_links.iter().any(|artifact| {
            artifact.kind == "review_finding" && artifact.path == "findings/validation-failed-0.md"
        }));
        assert!(
            detail
                .events
                .iter()
                .any(|event| event.kind == "review_finding"
                    && event.summary == "Validation failed: cargo check")
        );
    }

    #[test]
    fn thinking_traces_are_durable_artifacts_and_timeline_events() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Reasoning Quest".to_owned(),
                "Capture model thinking.".to_owned(),
                "# Reasoning Quest\n\n## Goal\n\nCapture model thinking.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        store
            .write_thinking_trace(
                &created.record.id,
                "initial-plan",
                "Initial planning thinking",
                "Consider the spec, choose a narrow edit, then validate.",
            )
            .unwrap();
        let detail = store.get(&created.record.id).unwrap();

        assert!(
            detail
                .record
                .artifact_links
                .iter()
                .any(|artifact| artifact.kind == "thinking"
                    && artifact.path == "thinking/initial-plan.md")
        );
        assert!(
            detail.events.iter().any(|event| event.kind == "thinking"
                && event.details["path"] == "thinking/initial-plan.md")
        );
        assert!(
            fs::read_to_string(
                store
                    .quest_path(&created.record.id)
                    .unwrap()
                    .join("thinking/initial-plan.md")
            )
            .unwrap()
            .contains("Consider the spec")
        );
    }

    #[test]
    fn cancel_and_reopen_are_durable_lifecycle_decisions() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Cancel Quest".to_owned(),
                "Cancel and reopen".to_owned(),
                "# Cancel Quest\n\n## Goal\n\nCancel and reopen.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let canceled = store
            .cancel(&created.record.id, "User stopped this Quest")
            .unwrap();
        assert_eq!(canceled.record.status, QuestStatus::Canceled);
        assert!(
            canceled
                .record
                .decisions
                .iter()
                .any(|decision| decision.kind == "cancel")
        );

        let reopened = store
            .reopen(&created.record.id, "Try again with a smaller scope")
            .unwrap();
        assert_eq!(reopened.record.status, QuestStatus::Specified);
        assert!(
            reopened
                .record
                .decisions
                .iter()
                .any(|decision| decision.kind == "reopen")
        );
    }

    #[test]
    fn invalid_transition_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Build Quest shell".to_owned(),
                "Create a durable shell".to_owned(),
                "# Build Quest shell\n\n## Goal\n\nCreate a durable shell.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        let error = store
            .transition(&created.record.id, QuestStatus::Completed)
            .unwrap_err();
        assert!(error.to_string().contains("invalid Quest transition"));
    }

    #[test]
    fn mock_execution_produces_review_without_project_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let store = QuestStore::new(temp.path().join("global-quests"));
        let created = store
            .create(
                "Build Quest shell".to_owned(),
                "Create a durable shell".to_owned(),
                "# Build Quest shell\n\n## Goal\n\nCreate a durable shell.".to_owned(),
                project(&project_root),
            )
            .unwrap();

        let reviewed = store.mock_execute(&created.record.id).unwrap();
        assert_eq!(reviewed.record.status, QuestStatus::ReadyForReview);
        assert!(reviewed.record.review.is_some());
        let review = reviewed.record.review.as_ref().unwrap();
        assert!(
            review
                .changed_files
                .iter()
                .all(|file| !file.diff.is_empty())
        );
        assert_eq!(review.metrics.isolated_attempt_count, 2);
        assert_eq!(review.metrics.validation_count, 2);
        assert_eq!(review.metrics.validation_failure_count, 0);
        assert!(
            review
                .metrics
                .review_evidence_quality_score
                .unwrap_or_default()
                > 0.0
        );
        assert_eq!(reviewed.record.checkpoints.len(), 1);
        assert_eq!(reviewed.record.checkpoints[0].id, "mock-workspace");
        assert_eq!(
            reviewed.record.checkpoints[0].workspace_id.as_deref(),
            reviewed.record.workspace_id.as_deref()
        );
        assert!(
            reviewed
                .record
                .artifact_links
                .iter()
                .any(|artifact| artifact.kind == "checkpoint"
                    && artifact.path == "checkpoints/mock-workspace.md")
        );
        assert!(reviewed.events.iter().any(
            |event| event.kind == "checkpoint" && event.summary == "Mock workspace checkpoint"
        ));
        assert_eq!(fs::read_dir(project_root).unwrap().count(), 0);
    }

    #[test]
    fn deleting_quest_removes_snapshot_and_history() {
        let temp = tempfile::tempdir().unwrap();
        let store = QuestStore::new(temp.path().join("quests"));
        let created = store
            .create(
                "Delete Quest shell".to_owned(),
                "Remove local task records".to_owned(),
                "# Delete Quest shell\n\n## Goal\n\nRemove local task records.".to_owned(),
                project(temp.path()),
            )
            .unwrap();

        store.delete(&created.record.id).unwrap();

        assert!(store.list().unwrap().is_empty());
        assert!(store.get(&created.record.id).is_err());
    }
}
