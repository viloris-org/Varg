//! Transaction bundle definition, verification, and application.
//!
//! A `TransactionBundle` is an immutable, hash-verified package of operations,
//! artifacts, rollback journal, and approval metadata. It is the **only**
//! mechanism by which AI-generated changes may enter the active project.
//!
//! ## Bundle Lifecycle
//!
//! ```text
//! 1. Manager merges approved Worker outputs → IntegrationCandidate
//! 2. Deterministic validators run on the candidate
//! 3. Deep Reviewer inspects the candidate
//! 4. Manager packages reviewed work into a TransactionBundle
//! 5. User reviews the bundle (diffs, previews, validation, risks)
//! 6. User approves, partially accepts, rejects, or requests revision
//! 7. Approved bundle is atomically applied to the active project
//!    through the editor transaction engine
//! 8. Apply verification (reload scenes, rescan assets, validate hashes)
//! 9. On failure → full rollback to pre-bundle trusted state
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use engine_policy::ids::{BundleHash, SnapshotId, TaskId};

use crate::protocol::{ProblemReport, QuickFixAction, ReviewFinding, ReviewRisk};

// ── TransactionBundle ─────────────────────────────────────────────────────────

/// An immutable transaction bundle ready for user review and application.
///
/// The bundle packages all reviewed, validated changes into a single
/// atomic unit. The editor transaction engine applies the bundle to the
/// active project — never directly, never incrementally.
///
/// The bundle is content-addressed: `bundle_hash` is SHA-256 over the
/// canonical JSON of the operation list, touched artifacts, rollback
/// journal, validation report, audit report, review report, and approval
/// metadata. Any tampering is detected by hash mismatch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionBundle {
    /// Stable bundle identifier (e.g., `bundle-task-0001`).
    pub bundle_id: String,

    /// The task this bundle fulfills.
    pub task_id: TaskId,

    /// The snapshot this bundle was built from.
    pub snapshot_id: SnapshotId,

    /// Base git revision at snapshot time.
    pub base_revision: String,

    /// Scope hash — identifies the task scope this bundle is bound to.
    pub scope_hash: String,

    /// Content hash — SHA-256 over canonical JSON of the bundle contents.
    /// Verified before apply. Mismatch → bundle rejected.
    pub bundle_hash: BundleHash,

    /// Canonical operation list (the actual changes to apply).
    pub operations: Vec<BundleOperation>,

    /// Logical change groups for user-facing presentation.
    pub change_groups: Vec<BundleChangeGroup>,

    /// All files touched by this bundle.
    pub touched_files: Vec<String>,

    /// All entities touched by this bundle.
    pub touched_entities: Vec<String>,

    /// All scenes touched by this bundle.
    pub touched_scenes: Vec<String>,

    /// All assets touched by this bundle.
    pub touched_assets: Vec<String>,

    /// All editor commands executed.
    pub executed_commands: Vec<String>,

    /// All editor settings modified.
    pub modified_settings: Vec<String>,

    /// Before hashes for all modified trusted artifacts.
    pub before_hashes: BTreeMap<String, String>,

    /// After hashes for all modified trusted artifacts.
    pub after_hashes: BTreeMap<String, String>,

    /// Rollback journal for atomic undo.
    pub rollback_journal: RollbackJournal,

    /// Deterministic validation report.
    pub validation_report: serde_json::Value,

    /// Deterministic audit report (for scripts/commands).
    pub audit_report: Option<serde_json::Value>,

    /// Optional Risk Auditor advisory report.
    pub risk_auditor_report: Option<serde_json::Value>,

    /// Deep Reviewer report.
    pub review_report: ReviewReport,

    /// Unresolved problems that remain after all repair cycles.
    pub unresolved_problems: Vec<ProblemReport>,

    /// Available quick-fix actions for unresolved problems.
    pub quick_fix_actions: Vec<QuickFixAction>,

    /// Risk assessment summary.
    pub risk_assessment: BundleRiskAssessment,

    /// User approval record (populated after user approves).
    pub user_approval: Option<UserApprovalRecord>,

    /// Application and verification results (populated after apply).
    pub apply_result: Option<ApplyResult>,

    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// A single operation in the bundle's canonical operation list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleOperation {
    /// Operation identifier within the bundle.
    pub operation_id: String,

    /// Operation type (maps to `AgentOperation` variant names).
    pub operation_type: String,

    /// The Worker that produced this operation.
    pub source_worker: String,

    /// The task this operation belongs to.
    pub source_task_id: TaskId,

    /// The tool call that generated this operation.
    pub tool_call_id: String,

    /// Operation parameters as structured JSON.
    pub params: serde_json::Value,

    /// The logical change group this operation belongs to.
    pub change_group_id: String,
}

/// A logical group of related changes for user-facing presentation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleChangeGroup {
    /// Group identifier.
    pub group_id: String,

    /// Human-readable description (e.g., "Player controller script").
    pub description: String,

    /// Files modified in this group.
    pub files: Vec<String>,

    /// Entities modified in this group.
    pub entities: Vec<String>,

    /// Whether this group can be accepted independently (partial accept).
    pub independently_acceptible: bool,
}

// ── Rollback Journal ─────────────────────────────────────────────────────────

/// Rollback journal enabling atomic undo of the entire bundle.
///
/// Every file create, update, delete, asset database change, scene change,
/// settings change, and editor dirty-state change is recorded. Rollback
/// replays the journal in reverse order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollbackJournal {
    /// File operations in application order.
    pub file_operations: Vec<FileRollbackEntry>,

    /// Asset database operations.
    pub asset_operations: Vec<AssetRollbackEntry>,

    /// Scene operations.
    pub scene_operations: Vec<SceneRollbackEntry>,

    /// Editor settings operations.
    pub settings_operations: Vec<SettingsRollbackEntry>,

    /// Whether the rollback journal covers all bundle operations.
    pub is_complete: bool,

    /// Journal hash for integrity verification.
    pub journal_hash: String,
}

/// A file-level rollback entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRollbackEntry {
    /// File path relative to project root.
    pub path: String,

    /// Operation type.
    pub operation: FileOperation,

    /// Content before the operation (for rollback).
    /// None if the file did not exist before (create operation).
    pub before_content: Option<String>,

    /// Content after the operation (for redo).
    pub after_content: Option<String>,

    /// Whether a `.bak` backup was created.
    pub backup_created: bool,

    /// Path to the backup file, if any.
    pub backup_path: Option<String>,
}

/// File operation type for rollback.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    /// File was created.
    Create,
    /// File was updated.
    Update,
    /// File was deleted.
    Delete,
}

/// An asset database rollback entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetRollbackEntry {
    /// Asset GUID.
    pub guid: String,

    /// Asset path.
    pub path: String,

    /// Operation type.
    pub operation: AssetOperation,

    /// Serialized asset state before the operation.
    pub before_state: Option<String>,

    /// Serialized asset state after the operation.
    pub after_state: Option<String>,
}

/// Asset operation type for rollback.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetOperation {
    /// Asset was imported.
    Import,
    /// Asset was updated/reimported.
    Update,
    /// Asset was deleted.
    Delete,
    /// Asset metadata was changed.
    MetadataChange,
}

/// A scene-level rollback entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneRollbackEntry {
    /// Scene file path.
    pub scene_path: String,

    /// Entity affected (or None for scene-level changes).
    pub entity_id: Option<String>,

    /// Operation type.
    pub operation: SceneOperation,

    /// Serialized scene state before the operation.
    pub before_snapshot: Option<String>,

    /// Serialized scene state after the operation.
    pub after_snapshot: Option<String>,
}

/// Scene operation type for rollback.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneOperation {
    /// Entity was created.
    EntityCreated,
    /// Entity was destroyed.
    EntityDestroyed,
    /// Component was added.
    ComponentAdded,
    /// Component was removed.
    ComponentRemoved,
    /// Component field was modified.
    ComponentModified,
    /// Transform was changed.
    TransformChanged,
    /// Scene-level change (e.g., settings, hierarchy).
    SceneLevelChange,
}

/// An editor settings rollback entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsRollbackEntry {
    /// Settings key path (e.g., "editor.layout").
    pub key: String,

    /// Value before the change.
    pub before_value: Option<serde_json::Value>,

    /// Value after the change.
    pub after_value: Option<serde_json::Value>,
}

// ── Review Report ─────────────────────────────────────────────────────────────

/// The Deep Reviewer's report embedded in the transaction bundle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewReport {
    /// Review verdict at time of bundling.
    pub verdict: crate::protocol::ReviewVerdict,

    /// All review findings.
    pub findings: Vec<ReviewFinding>,

    /// Residual risk after review.
    pub residual_risk: ReviewRisk,

    /// Whether all blocking issues were resolved.
    pub blocking_issues_resolved: bool,

    /// Reviewer session identifier (for traceability).
    pub reviewer_session_id: String,

    /// ISO-8601 review timestamp.
    pub reviewed_at: String,
}

// ── Risk Assessment ───────────────────────────────────────────────────────────

/// Risk assessment embedded in the transaction bundle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleRiskAssessment {
    /// Deterministic risk level from the RiskClassifier.
    pub deterministic_risk: engine_policy::risk::RiskClass,

    /// Residual risk from the Deep Reviewer.
    pub reviewer_residual_risk: ReviewRisk,

    /// Whether any high-risk operations are included.
    pub has_high_risk_operations: bool,

    /// Whether step-up confirmation is required.
    pub requires_step_up_confirmation: bool,

    /// Human-readable risk summary.
    pub summary: String,
}

// ── User Approval ─────────────────────────────────────────────────────────────

/// Record of user approval for a transaction bundle.
///
/// User approval authorizes the bundle for application, but it does NOT
/// bypass policy, validation, sandbox, audit, review, rollback, or
/// stale-context checks. Those are enforced deterministically regardless
/// of approval state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserApprovalRecord {
    /// Whether the user approved this bundle.
    pub approved: bool,

    /// Whether this was a partial acceptance (subset of changes).
    pub partial_acceptance: bool,

    /// If partial, which change groups were accepted.
    pub accepted_groups: Vec<String>,

    /// If partial, which change groups were rejected.
    pub rejected_groups: Vec<String>,

    /// Whether step-up confirmation was performed.
    pub step_up_confirmed: bool,

    /// Step-up confirmation method (e.g., "project_name_reentry").
    pub step_up_method: Option<String>,

    /// ISO-8601 approval timestamp.
    pub approved_at: String,

    /// User identifier (from editor account, if available).
    pub user_id: Option<String>,
}

// ── Apply Result ──────────────────────────────────────────────────────────────

/// Result of applying the transaction bundle to the active project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyResult {
    /// Whether the apply succeeded.
    pub success: bool,

    /// Whether post-apply verification passed.
    pub verification_passed: bool,

    /// Number of file operations applied.
    pub file_ops_applied: u32,

    /// Number of asset operations applied.
    pub asset_ops_applied: u32,

    /// Number of scene operations applied.
    pub scene_ops_applied: u32,

    /// Post-apply scene state hash.
    pub post_apply_scene_hash: Option<String>,

    /// Post-apply asset index hash.
    pub post_apply_asset_hash: Option<String>,

    /// Whether rollback was triggered (on failure).
    pub rollback_triggered: bool,

    /// Rollback result (if rollback was triggered).
    pub rollback_result: Option<RollbackResult>,

    /// ISO-8601 apply timestamp.
    pub applied_at: String,
}

/// Result of a rollback operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollbackResult {
    /// Whether rollback succeeded.
    pub success: bool,

    /// Number of operations rolled back.
    pub ops_rolled_back: u32,

    /// Whether the project state was restored to the pre-bundle state.
    pub state_restored: bool,

    /// Post-rollback state hash (should match pre-bundle hash).
    pub post_rollback_hash: Option<String>,

    /// Any errors encountered during rollback.
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_bundle_has_required_fields() {
        let bundle = TransactionBundle {
            bundle_id: "bundle-001".into(),
            task_id: TaskId::from_u128(1),
            snapshot_id: SnapshotId::from_u128(100),
            base_revision: "HEAD".into(),
            scope_hash: "scope-abc".into(),
            bundle_hash: BundleHash::new("hash-xyz"),
            operations: vec![],
            change_groups: vec![],
            touched_files: vec!["scenes/main.json".into()],
            touched_entities: vec!["1:1".into()],
            touched_scenes: vec!["scenes/main.json".into()],
            touched_assets: vec![],
            executed_commands: vec![],
            modified_settings: vec![],
            before_hashes: BTreeMap::new(),
            after_hashes: BTreeMap::new(),
            rollback_journal: RollbackJournal {
                file_operations: vec![],
                asset_operations: vec![],
                scene_operations: vec![],
                settings_operations: vec![],
                is_complete: true,
                journal_hash: "journal-hash".into(),
            },
            validation_report: serde_json::json!({"passed": true}),
            audit_report: None,
            risk_auditor_report: None,
            review_report: ReviewReport {
                verdict: crate::protocol::ReviewVerdict::Approved,
                findings: vec![],
                residual_risk: ReviewRisk::Low,
                blocking_issues_resolved: true,
                reviewer_session_id: "review-session-1".into(),
                reviewed_at: "2025-01-01T00:00:00Z".into(),
            },
            unresolved_problems: vec![],
            quick_fix_actions: vec![],
            risk_assessment: BundleRiskAssessment {
                deterministic_risk: engine_policy::risk::RiskClass::Low,
                reviewer_residual_risk: ReviewRisk::Low,
                has_high_risk_operations: false,
                requires_step_up_confirmation: false,
                summary: "Low risk; routine scene edit".into(),
            },
            user_approval: None,
            apply_result: None,
            created_at: "2025-01-01T00:00:00Z".into(),
        };

        assert_eq!(bundle.bundle_id, "bundle-001");
        assert!(bundle.rollback_journal.is_complete);
        assert!(!bundle.risk_assessment.requires_step_up_confirmation);
    }

    #[test]
    fn rollback_journal_covers_all_operation_types() {
        let journal = RollbackJournal {
            file_operations: vec![
                FileRollbackEntry {
                    path: "scripts/player.aster".into(),
                    operation: FileOperation::Create,
                    before_content: None,
                    after_content: Some("fn on_start() {}".into()),
                    backup_created: false,
                    backup_path: None,
                },
                FileRollbackEntry {
                    path: "scenes/main.json".into(),
                    operation: FileOperation::Update,
                    before_content: Some("old".into()),
                    after_content: Some("new".into()),
                    backup_created: true,
                    backup_path: Some("scenes/main.json.bak".into()),
                },
            ],
            asset_operations: vec![],
            scene_operations: vec![],
            settings_operations: vec![],
            is_complete: true,
            journal_hash: "test".into(),
        };

        assert_eq!(journal.file_operations.len(), 2);
        assert_eq!(journal.file_operations[0].operation, FileOperation::Create);
        assert_eq!(journal.file_operations[1].operation, FileOperation::Update);
    }

    #[test]
    fn apply_result_records_rollback_on_failure() {
        let result = ApplyResult {
            success: false,
            verification_passed: false,
            file_ops_applied: 3,
            asset_ops_applied: 0,
            scene_ops_applied: 1,
            post_apply_scene_hash: None,
            post_apply_asset_hash: None,
            rollback_triggered: true,
            rollback_result: Some(RollbackResult {
                success: true,
                ops_rolled_back: 4,
                state_restored: true,
                post_rollback_hash: Some("pre-bundle-hash".into()),
                errors: vec![],
            }),
            applied_at: "2025-01-01T00:00:00Z".into(),
        };

        assert!(result.rollback_triggered);
        assert!(result.rollback_result.unwrap().state_restored);
    }

    #[test]
    fn partial_acceptance_tracks_accepted_and_rejected_groups() {
        let approval = UserApprovalRecord {
            approved: true,
            partial_acceptance: true,
            accepted_groups: vec!["group-scene".into()],
            rejected_groups: vec!["group-script".into()],
            step_up_confirmed: false,
            step_up_method: None,
            approved_at: "2025-01-01T00:00:00Z".into(),
            user_id: None,
        };

        assert!(approval.partial_acceptance);
        assert_eq!(approval.accepted_groups.len(), 1);
        assert_eq!(approval.rejected_groups.len(), 1);
    }
}
