//! Structured message protocol for Agent Cluster orchestration.
//!
//! All communication between Manager, Workers, Reviewers, and the session
//! orchestrator passes through typed messages. Free-form prose is never
//! treated as a structured protocol element.
//!
//! ## Message Flow (Auto Mode)
//!
//! ```text
//! User Request
//!   ↓
//! Manager.decompose() → TaskDecomposition
//!   ↓
//! Manager → session orchestrator → Worker (fresh session)
//!   carries: TaskAssignment + ContextPacket
//!   ↓
//! Worker → Manager
//!   carries: WorkerOutput (approved/needs_revision/blocked)
//!   ↓
//! Manager.merge() → IntegrationCandidate
//!   ↓
//! Deterministic Validators run
//!   ↓
//! Manager → session orchestrator → Deep Reviewer (fresh session)
//!   carries: ReviewRequest + ContextPacket
//!   ↓
//! Deep Reviewer → Manager
//!   carries: ReviewDecision (approved/needs_revision/blocked)
//!   ↓
//! [If needs_revision] Manager → Repair Worker → patched IntegrationCandidate
//!   ↓
//! Manager → User: FinalReport
//!   ↓
//! User approves → TransactionBundle → Editor Apply
//! ```

use serde::{Deserialize, Serialize};

use engine_policy::context::{AgentRole, ExpectedArtifact, TaskBrief};
use engine_policy::ids::{GrantHash, SnapshotId, TaskId, WorkspaceId};

// ── Task Decomposition ────────────────────────────────────────────────────────

/// Output of the Manager's `decompose()` step.
///
/// Carries the immutable snapshot ID, git workspace reference, and the
/// list of bounded tasks the Manager intends to assign to Workers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDecomposition {
    /// Stable decomposition identifier.
    pub decomposition_id: String,

    /// The parent user request this decomposition serves.
    pub request_id: String,

    /// Immutable snapshot created before any work begins.
    pub snapshot_id: SnapshotId,

    /// Git-backed isolated workspace for all Worker writes.
    pub workspace_id: WorkspaceId,

    /// Base git revision (e.g., "HEAD") at snapshot time.
    pub base_revision: String,

    /// Tasks to execute. Order may imply dependencies; Workers with
    /// no inter-task dependencies may execute in parallel.
    pub tasks: Vec<TaskTicket>,

    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// A bounded, independently reviewable task ticket.
///
/// Created by the Manager during decomposition. Before execution,
/// the Manager requests a capability grant from the Capability Issuer
/// and builds a context packet for the assigned Worker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskTicket {
    /// Stable task identifier.
    pub task_id: TaskId,

    /// The Worker kind best suited for this task.
    pub worker_kind: WorkerKind,

    /// The policy-checked task brief (normalized from Manager prose).
    pub brief: TaskBrief,

    /// Whether this task depends on the output of another task.
    /// If set, the Worker is not spawned until the dependency completes.
    pub depends_on: Vec<TaskId>,

    /// Priority hint for scheduling (0 = lowest, 10 = highest).
    pub priority: u8,
}

/// Worker specialization kind (mirrors `engine_policy::context::WorkerKind`
/// for protocol use without pulling in the full context module).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    /// Scene manipulation.
    Scene,
    /// Script creation and modification.
    Script,
    /// Asset import, reference validation.
    Asset,
    /// Diagnostics analysis.
    Diagnostics,
    /// Read-only explanation.
    Explain,
    /// Scoped repair.
    Repair,
    /// High-risk script/command audit.
    Audit,
}

impl From<WorkerKind> for engine_policy::context::WorkerKind {
    fn from(kind: WorkerKind) -> Self {
        match kind {
            WorkerKind::Scene => Self::Scene,
            WorkerKind::Script => Self::Script,
            WorkerKind::Asset => Self::Asset,
            WorkerKind::Diagnostics => Self::Diagnostics,
            WorkerKind::Explain => Self::Explain,
            WorkerKind::Repair => Self::Repair,
            WorkerKind::Audit => Self::Audit,
        }
    }
}

impl From<engine_policy::context::WorkerKind> for WorkerKind {
    fn from(kind: engine_policy::context::WorkerKind) -> Self {
        match kind {
            engine_policy::context::WorkerKind::Scene => Self::Scene,
            engine_policy::context::WorkerKind::Script => Self::Script,
            engine_policy::context::WorkerKind::Asset => Self::Asset,
            engine_policy::context::WorkerKind::Diagnostics => Self::Diagnostics,
            engine_policy::context::WorkerKind::Explain => Self::Explain,
            engine_policy::context::WorkerKind::Repair => Self::Repair,
            engine_policy::context::WorkerKind::Audit => Self::Audit,
        }
    }
}

// ── Task Assignment (Manager → Worker) ───────────────────────────────────────

/// The structured assignment handed to a Worker in a fresh session.
///
/// This is the ONLY message a Worker receives at session start.
/// It does not see the Manager's full conversation, sibling Worker
/// context, or the user's raw prompt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskAssignment {
    /// The task the Worker must execute.
    pub task_id: TaskId,

    /// The Worker's role for this assignment.
    pub role: AgentRole,

    /// The capability grant hash authorizing this Worker's tool calls.
    pub grant_hash: GrantHash,

    /// Context packet ID the Worker receives (for traceability).
    pub context_packet_id: String,

    /// The task brief (policy-checked scope).
    pub brief: TaskBrief,

    /// Maximum tool calls allowed before the Worker must stop and report.
    pub step_limit: u32,

    /// ISO-8601 deadline for this task (or None for no deadline).
    pub deadline: Option<String>,

    /// Immutable project snapshot this task was decomposed from.
    pub snapshot_id: SnapshotId,
}

// ── Worker Output (Worker → Manager) ─────────────────────────────────────────

/// Structured output from a Worker after completing (or failing) its task.
///
/// Worker self-reports (rationale, claims, summaries) are `UNTRUSTED_WORKER_REPORT`.
/// The Manager and Reviewer evaluate ONLY the objective artifacts (diffs, validator
/// output, actual file changes), not the Worker's prose.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerOutput {
    /// The task this output corresponds to.
    pub task_id: TaskId,

    /// The Worker's role.
    pub worker_kind: WorkerKind,

    /// Final state of this Worker's task.
    pub state: WorkerState,

    /// Objective artifacts produced (diffs, scene previews, validator logs).
    /// These are what Reviewers evaluate, not the Worker's self-report.
    pub artifacts: Vec<Artifact>,

    /// Untrusted Worker self-report — rationale, approach, claimed tests.
    /// Labeled as untrusted; Reviewers treat this as navigation hints only.
    pub self_report: Option<String>,

    /// Problems encountered (unrecoverable failures, out-of-scope needs,
    /// ambiguous conditions). Must not be empty for non-success states.
    pub problems: Vec<ProblemReport>,

    /// ISO-8601 completion timestamp.
    pub completed_at: String,
}

/// Worker task completion state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Task completed successfully with all expected artifacts.
    Completed,
    /// Task completed but some acceptance criteria were not met.
    /// Artifacts may still be usable after local review.
    PartiallyCompleted,
    /// Task encountered an unrecoverable failure.
    /// The Manager must decide: reassign, repair, escalate, or block.
    Failed,
    /// Task was blocked by a policy or capability constraint.
    Blocked,
}

/// An objective artifact produced by a Worker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    /// Artifact type: "diff", "scene_preview", "validator_log", "script_file", etc.
    pub artifact_type: String,

    /// Target reference (file path, entity ID, asset GUID).
    pub target: String,

    /// The artifact content as structured JSON (not free-form prose).
    pub content: serde_json::Value,

    /// Trust label for this artifact.
    pub label: ArtifactTrust,
}

/// Trust classification for Worker-produced artifacts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactTrust {
    /// A deterministic diff or structured change record (trusted shape, untrusted intent).
    DeterministicShape,
    /// Output of a deterministic validator (trusted).
    ValidatorOutput,
    /// Output of a deterministic auditor (trusted).
    AuditorOutput,
    /// Worker-generated content marked as untrusted (scripts, scene changes, prose).
    UntrustedWorkerContent,
}

// ── Local Review (per-Worker) ─────────────────────────────────────────────────

/// Decision from local review of a single Worker's output.
///
/// Local review happens BEFORE integration. Blocked outputs cannot
/// proceed to the integration candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalReviewDecision {
    /// The Worker output being reviewed.
    pub task_id: TaskId,

    /// Review outcome.
    pub decision: ReviewVerdict,

    /// Specific findings (what needs fixing, if `needs_revision`).
    pub findings: Vec<ReviewFinding>,

    /// Risk tags associated with this output.
    pub risk_tags: Vec<String>,

    /// ISO-8601 review timestamp.
    pub reviewed_at: String,
}

// ── Deep Review (integrated candidate) ───────────────────────────────────────

/// Request sent to the Deep Reviewer in a fresh session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewRequest {
    /// The integration candidate being reviewed.
    pub candidate_id: String,

    /// The original task brief and acceptance criteria.
    pub task_brief: TaskBrief,

    /// All accepted Worker artifacts (after local review).
    pub accepted_artifacts: Vec<Artifact>,

    /// Deterministic validator output (build, schema, asset refs, etc.).
    pub validator_output: serde_json::Value,

    /// Audit report from the Deterministic Static Auditor (if applicable).
    pub audit_report: Option<serde_json::Value>,

    /// Context packet ID for traceability.
    pub context_packet_id: String,
}

/// Decision from the Deep Reviewer.
///
/// The Deep Reviewer evaluates the INTEGRATED candidate (not individual
/// Worker outputs). Its decision is advisory and monotonic — it may raise
/// risk, recommend blocking, or send back for repair, but cannot grant
/// permissions or override deterministic validators.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewDecision {
    /// The integration candidate being reviewed.
    pub candidate_id: String,

    /// Review verdict.
    pub verdict: ReviewVerdict,

    /// Specific findings.
    pub findings: Vec<ReviewFinding>,

    /// Whether any findings are blocking.
    pub has_blocking_issues: bool,

    /// Residual risk level after review (monotonic: cannot be lower than
    /// the deterministic risk classification).
    pub residual_risk: ReviewRisk,

    /// ISO-8601 review timestamp.
    pub reviewed_at: String,
}

/// Review verdict for both local and deep review.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    /// All criteria met; output is ready for the next stage.
    Approved,
    /// Specific issues found; Worker must revise.
    NeedsRevision,
    /// Output is fundamentally blocked (out of scope, unsafe, irrecoverable).
    Blocked,
}

/// A specific finding from a review pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// Finding identifier within this review.
    pub finding_id: String,

    /// Severity: "info", "warning", "error", "blocking".
    pub severity: String,

    /// Human-readable description.
    pub description: String,

    /// Affected files, entities, assets, or commands.
    pub affected: Vec<String>,

    /// Suggested remediation (for `needs_revision` verdicts).
    pub suggested_fix: Option<String>,
}

/// Residual risk level reported by the Deep Reviewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRisk {
    /// No significant residual risk.
    Low,
    /// Some uncertainty; user should review but may proceed.
    Medium,
    /// Significant uncertainty or potential regression; user should
    /// review carefully before approving.
    High,
}

// ── Repair Ticket ─────────────────────────────────────────────────────────────

/// A scoped repair ticket created when validation or review fails.
///
/// Repair Workers receive this ticket in a fresh session with only the
/// ticket, failing evidence, and the current integration candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairTicket {
    /// Stable ticket identifier.
    pub ticket_id: String,

    /// The integration candidate that needs repair.
    pub candidate_id: String,

    /// Severity of the issue requiring repair.
    pub severity: RepairSeverity,

    /// Affected files, entities, assets, or commands.
    pub affected: Vec<String>,

    /// How to reproduce the failure.
    pub reproduction: String,

    /// Expected outcome after repair.
    pub expected_outcome: String,

    /// Allowed repair scope (files/entities/assets that may be modified).
    pub allowed_scope: Vec<String>,

    /// Current retry count (starts at 0; cap at repair_policy.max_retries).
    pub retry_count: u32,

    /// Maximum retries before escalation to blocked.
    pub max_retries: u32,

    /// Failing evidence: validator output, review findings, etc.
    pub failing_evidence: serde_json::Value,

    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// Repair ticket severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairSeverity {
    /// Non-blocking improvement (e.g., naming convention).
    Advisory,
    /// Should be fixed but the candidate could be applied without it.
    NonBlocking,
    /// Must be fixed before the candidate can be applied.
    Blocking,
}

// ── Problem Report ────────────────────────────────────────────────────────────

/// A structured problem report from a Worker or Reviewer.
///
/// Problems must NOT be silently skipped. Every unrecoverable failure,
/// out-of-scope need, ambiguous condition, or exhausted retry appears
/// in the Manager's final report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProblemReport {
    /// Stable problem identifier within the task.
    pub problem_id: String,

    /// The task or review that encountered this problem.
    pub source_task_id: TaskId,

    /// Concise description of what failed and why.
    pub description: String,

    /// Affected files, entities, assets, or commands.
    pub affected: Vec<String>,

    /// Problem severity.
    pub severity: ProblemSeverity,

    /// Whether this problem blocks the overall request.
    pub is_blocking: bool,

    /// Whether the user must clarify product intent to resolve this.
    pub requires_user_clarification: bool,

    /// Suggested quick-fix action the user can trigger from the report.
    pub quick_fix: Option<QuickFixAction>,

    /// ISO-8601 timestamp.
    pub reported_at: String,
}

/// Problem severity classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemSeverity {
    /// Informational only; no action needed.
    Advisory,
    /// Non-blocking issue; task can proceed but may have reduced quality.
    NonBlocking,
    /// Blocks this specific task but not the overall request.
    TaskBlocking,
    /// Blocks the entire request from proceeding.
    RequestBlocking,
}

/// A scoped quick-fix action the user can trigger from the final report.
///
/// Quick-fix launches a new scoped Auto mode task limited to this issue.
/// It reuses the original snapshot and workspace where possible.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickFixAction {
    /// Stable action identifier.
    pub action_id: String,

    /// Human-readable label shown on the quick-fix button.
    pub label: String,

    /// What the quick-fix will do.
    pub description: String,

    /// The task ID of the original task that produced the problem.
    pub original_task_id: TaskId,

    /// Whether the quick-fix requires user input (product intent clarification).
    pub requires_user_input: bool,

    /// The scoped task brief for the quick-fix Worker.
    pub fix_task_brief: TaskBrief,

    /// Expected artifacts from the quick-fix.
    pub expected_artifacts: Vec<ExpectedArtifact>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_policy::context::{RepairPolicy, ReviewRubric};

    #[test]
    fn worker_kind_converts_to_context_worker_kind() {
        let kind: engine_policy::context::WorkerKind = WorkerKind::Scene.into();
        assert_eq!(kind, engine_policy::context::WorkerKind::Scene);
    }

    #[test]
    fn context_worker_kind_converts_to_worker_kind() {
        let kind: WorkerKind = engine_policy::context::WorkerKind::Audit.into();
        assert_eq!(kind, WorkerKind::Audit);
    }

    #[test]
    fn task_ticket_serialization_roundtrip() {
        let ticket = TaskTicket {
            task_id: TaskId::from_u128(1),
            worker_kind: WorkerKind::Scene,
            brief: TaskBrief {
                task_id: TaskId::from_u128(1),
                objective: "Create a player object".into(),
                non_goals: vec!["Don't modify existing objects".into()],
                allowed_files: vec![],
                allowed_entities: vec![],
                allowed_scenes: vec![],
                allowed_assets: vec![],
                allowed_operations: vec!["create_object".into()],
                forbidden_operations: vec![],
                acceptance_criteria: vec!["Scene contains entity named 'Player'".into()],
                expected_artifacts: vec![],
                review_rubric: ReviewRubric {
                    correctness: vec![],
                    scope: vec![],
                    safety: vec![],
                    rollback: vec![],
                    user_impact: vec![],
                },
                required_evidence: vec![],
                repair_policy: RepairPolicy::default(),
            },
            depends_on: vec![],
            priority: 5,
        };

        let json = serde_json::to_string(&ticket).unwrap();
        let decoded: TaskTicket = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.task_id, TaskId::from_u128(1));
        assert_eq!(decoded.worker_kind, WorkerKind::Scene);
        assert_eq!(decoded.brief.objective, "Create a player object");
    }

    #[test]
    fn problem_report_includes_quick_fix() {
        let report = ProblemReport {
            problem_id: "p-001".into(),
            source_task_id: TaskId::from_u128(2),
            description: "Script compilation failed: undefined variable 'playerSpeed'".into(),
            affected: vec!["scripts/player.rhai".into()],
            severity: ProblemSeverity::TaskBlocking,
            is_blocking: true,
            requires_user_clarification: false,
            quick_fix: Some(QuickFixAction {
                action_id: "qf-001".into(),
                label: "Fix player script compilation error".into(),
                description: "Correct the undefined variable reference in player.rhai".into(),
                original_task_id: TaskId::from_u128(2),
                requires_user_input: false,
                fix_task_brief: TaskBrief {
                    task_id: TaskId::from_u128(99),
                    objective: "Fix undefined variable in player.rhai".into(),
                    non_goals: vec![],
                    allowed_files: vec!["scripts/player.rhai".into()],
                    allowed_entities: vec![],
                    allowed_scenes: vec![],
                    allowed_assets: vec![],
                    allowed_operations: vec!["write_script".into()],
                    forbidden_operations: vec![],
                    acceptance_criteria: vec!["player.rhai compiles without errors".into()],
                    expected_artifacts: vec![],
                    review_rubric: ReviewRubric {
                        correctness: vec![],
                        scope: vec![],
                        safety: vec![],
                        rollback: vec![],
                        user_impact: vec![],
                    },
                    required_evidence: vec![],
                    repair_policy: RepairPolicy::default(),
                },
                expected_artifacts: vec![],
            }),
            reported_at: "2025-01-01T00:00:00Z".into(),
        };

        assert!(report.quick_fix.is_some());
        let qf = report.quick_fix.unwrap();
        assert_eq!(qf.label, "Fix player script compilation error");
    }
}
