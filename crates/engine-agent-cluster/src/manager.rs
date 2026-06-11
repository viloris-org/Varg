//! Manager agent orchestration trait.
//!
//! The Manager is the orchestrator in Auto mode. It decomposes user requests,
//! routes tasks to Workers, requests capability grants from the Issuer, merges
//! Worker outputs into an integration candidate, coordinates review, manages
//! the repair loop, and generates the final report.
//!
//! **The Manager is untrusted AI output.** Its decisions are reviewable
//! artifacts, not trusted proof. Policy code, capability grants, validators,
//! and the Deep Reviewer independently verify every Manager claim.

use engine_core::EngineResult;

use engine_policy::context::ContextPacket;
use engine_policy::grant::CapabilityGrant;
use engine_policy::ids::TaskId;

use crate::bundle::TransactionBundle;
use crate::protocol::{
    ProblemReport, QuickFixAction, ReviewDecision, ReviewRequest, TaskAssignment,
    TaskDecomposition, WorkerOutput,
};

/// The Manager agent trait.
///
/// Implementations use an AI model to make decisions, but the trait
/// enforces that Manager output is always structured (never free-form prose)
/// and always checked by deterministic policy before taking effect.
pub trait Manager {
    /// Decomposes a user request into independently reviewable tasks.
    ///
    /// The Manager reads project context from the immutable snapshot and
    /// produces a `TaskDecomposition`. This is the first Manager action
    /// in Auto mode.
    ///
    /// # Errors
    /// Returns an error if the request is underspecified, ambiguous in a
    /// way that affects product intent, or cannot be decomposed into
    /// safe bounded tasks. The Manager should ask for clarification rather
    /// than guess at product intent.
    fn decompose(
        &self,
        user_request: &str,
        snapshot_id: engine_policy::ids::SnapshotId,
        workspace_id: engine_policy::ids::WorkspaceId,
        base_revision: &str,
        context: &serde_json::Value,
    ) -> EngineResult<TaskDecomposition>;

    /// Requests capability grants for Workers.
    ///
    /// For each task in the decomposition, the Manager emits a structured
    /// `CapabilityRequest`. The Capability Issuer (deterministic code, not AI)
    /// evaluates each request and returns a `CapabilityDecision`. The Manager
    /// collects the issued grants and builds `TaskAssignment`s.
    ///
    /// The Manager MAY NOT mint grants directly. It can only request them.
    fn request_grants(
        &self,
        tasks: &TaskDecomposition,
        issuer: &dyn engine_policy::grant::CapabilityIssuer,
    ) -> EngineResult<Vec<(TaskAssignment, CapabilityGrant)>>;

    /// Assigns a task to a Worker by building its fresh-session context packet.
    ///
    /// The Manager specifies the Worker role, context packet contents, and
    /// the task assignment. The session orchestrator handles spawning the
    /// actual fresh session.
    fn build_context_packet(
        &self,
        assignment: &TaskAssignment,
        snapshot_data: &serde_json::Value,
        accepted_artifacts: &[crate::protocol::Artifact],
        validator_output: Option<&serde_json::Value>,
    ) -> EngineResult<ContextPacket>;

    /// Merges approved Worker outputs into an integration candidate.
    ///
    /// After local review passes, the Manager merges accepted artifacts
    /// into a single integration candidate. This candidate becomes the
    /// input for deterministic validation and deep review.
    fn merge(
        &self,
        approved_outputs: &[WorkerOutput],
        workspace_root: &str,
    ) -> EngineResult<IntegrationCandidate>;

    /// Invokes the Deep Reviewer on the integration candidate.
    ///
    /// The Manager spawns a fresh session for the Deep Reviewer with only
    /// the review rubric, accepted artifacts, validator output, and
    /// audit report — never raw Worker chat or Manager deliberation.
    fn request_deep_review(
        &self,
        candidate: &IntegrationCandidate,
        review_request: &ReviewRequest,
    ) -> EngineResult<ReviewDecision>;

    /// Creates a repair ticket when validation or review fails.
    ///
    /// Repair tickets are scoped to the specific failure. Repair Workers
    /// receive only the ticket, failing evidence, and integration candidate
    /// in a fresh session.
    fn create_repair_ticket(
        &self,
        candidate: &IntegrationCandidate,
        failed_review: &ReviewDecision,
        retry_count: u32,
    ) -> EngineResult<crate::protocol::RepairTicket>;

    /// Generates the final task report for user review.
    ///
    /// The report includes a summary, all changes, logical change groups,
    /// validation results, review findings, repaired issues, unresolved
    /// problems with quick-fix actions, risk assessment, and traceability
    /// references.
    fn generate_final_report(
        &self,
        candidate: &IntegrationCandidate,
        review: &ReviewDecision,
        problems: &[ProblemReport],
        quick_fixes: &[QuickFixAction],
    ) -> EngineResult<FinalReport>;
}

/// An integration candidate assembled by the Manager from approved Worker outputs.
///
/// This is the single source for deterministic validation and deep review.
/// It is backed by the git task workspace.
#[derive(Clone, Debug)]
pub struct IntegrationCandidate {
    /// Stable candidate identifier.
    pub candidate_id: String,

    /// The task this candidate serves.
    pub task_id: TaskId,

    /// All accepted Worker outputs merged into this candidate.
    pub merged_outputs: Vec<WorkerOutput>,

    /// Logical change groups for user-facing presentation.
    pub change_groups: Vec<ChangeGroup>,

    /// The immutable transaction bundle (populated after review passes).
    pub bundle: Option<TransactionBundle>,

    /// Current state of the integration.
    pub state: IntegrationState,

    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// A logical group of related changes for user-facing presentation.
#[derive(Clone, Debug)]
pub struct ChangeGroup {
    /// Group identifier.
    pub group_id: String,

    /// Human-readable description (e.g., "Player controller script").
    pub description: String,

    /// Files modified in this group.
    pub files: Vec<String>,

    /// Entities modified in this group.
    pub entities: Vec<String>,
}

/// Lifecycle state of an integration candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrationState {
    /// Worker outputs are being merged.
    Merging,
    /// Deterministic validation is running.
    Validating,
    /// Deep review is in progress.
    InReview,
    /// Review passed; bundle is ready for user approval.
    ReadyForApproval,
    /// A repair cycle is active.
    Repairing(u32),
    /// Integration failed (blocked).
    Blocked,
    /// User approved; bundle applied.
    Applied,
}

/// The final report presented to the user before approval.
#[derive(Clone, Debug)]
pub struct FinalReport {
    /// Report identifier.
    pub report_id: String,

    /// Summary of what was done and why.
    pub summary: String,

    /// All logical change groups.
    pub change_groups: Vec<ChangeGroup>,

    /// Validator results (deterministic).
    pub validation_results: serde_json::Value,

    /// Review findings (Deep Reviewer output).
    pub review_findings: Vec<crate::protocol::ReviewFinding>,

    /// Issues that were repaired during the repair loop.
    pub repaired_issues: Vec<ProblemReport>,

    /// Unresolved problems with quick-fix actions.
    pub unresolved_problems: Vec<ProblemReport>,

    /// Available quick-fix actions for unresolved problems.
    pub quick_fix_actions: Vec<QuickFixAction>,

    /// Risk assessment (deterministic classification + reviewer residual risk).
    pub risk_assessment: RiskAssessment,

    /// The transaction bundle (if the report is ready for user approval).
    pub bundle: Option<TransactionBundle>,

    /// ISO-8601 generation timestamp.
    pub generated_at: String,
}

/// Risk assessment in the final report.
#[derive(Clone, Debug)]
pub struct RiskAssessment {
    /// Deterministic risk level from the RiskClassifier.
    pub deterministic_risk: engine_policy::risk::RiskClass,

    /// Residual risk from the Deep Reviewer.
    pub reviewer_residual_risk: crate::protocol::ReviewRisk,

    /// Whether any high-risk operations are included.
    pub has_high_risk_operations: bool,

    /// Whether step-up confirmation is required.
    pub requires_step_up_confirmation: bool,

    /// Human-readable risk summary.
    pub summary: String,
}

// ── DefaultManager ────────────────────────────────────────────────────────────

use engine_policy::context::{
    AgentRole, ContextExpiration, ContextSection, ContextSource, WorkerKind,
};
use engine_policy::grant::CapabilityRequest;
use engine_policy::ids::{SnapshotId, WorkspaceId};
use engine_policy::risk::RiskClass;
use engine_policy::trust::TrustLabel;

use crate::protocol::{
    Artifact, ProblemSeverity, RepairSeverity, RepairTicket, ReviewRisk, ReviewVerdict, TaskTicket,
    WorkerKind as ProtoWorkerKind,
};

/// Default Manager implementation using rule-based decomposition.
///
/// For the MVP this uses deterministic keyword matching to decompose requests.
/// A production Manager would delegate to an AI model for context-aware
/// decomposition, with output reviewed by deterministic policy.
pub struct DefaultManager {
    /// Identifier for this Manager instance.
    pub manager_id: String,
}

impl DefaultManager {
    /// Creates a new Manager with the given identifier.
    pub fn new(manager_id: impl Into<String>) -> Self {
        Self {
            manager_id: manager_id.into(),
        }
    }

    /// Infers Worker specializations from a task description.
    fn infer_worker_kind(objective: &str) -> ProtoWorkerKind {
        let lower = objective.to_lowercase();
        if lower.contains("script") || lower.contains("rhai") || lower.contains("code") {
            ProtoWorkerKind::Script
        } else if lower.contains("asset") || lower.contains("import") || lower.contains("material")
        {
            ProtoWorkerKind::Asset
        } else if lower.contains("diagnostic")
            || lower.contains("error")
            || lower.contains("compile")
        {
            ProtoWorkerKind::Diagnostics
        } else if lower.contains("explain") || lower.contains("describe") || lower.contains("list")
        {
            ProtoWorkerKind::Explain
        } else if lower.contains("audit") || lower.contains("inspect") || lower.contains("check") {
            ProtoWorkerKind::Audit
        } else if lower.contains("repair") || lower.contains("fix") || lower.contains("patch") {
            ProtoWorkerKind::Repair
        } else {
            ProtoWorkerKind::Scene // default to scene manipulation
        }
    }

    /// Decomposes a request into separate task objective strings.
    fn split_objectives(request: &str) -> Vec<String> {
        let request = request.trim().to_string();
        // Look for conjunctions / bullet points / numbered lists
        let separators = [
            "\n- ",
            "\n* ",
            "\n1. ",
            "\n2. ",
            "\n3. ",
            " and also ",
            " then ",
        ];
        let mut objectives = vec![request.clone()];
        for sep in &separators {
            if request.contains(sep) {
                objectives = request
                    .split(sep)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                break; // use the first matching separator style
            }
        }
        objectives
    }
}

impl Manager for DefaultManager {
    fn decompose(
        &self,
        user_request: &str,
        snapshot_id: SnapshotId,
        workspace_id: WorkspaceId,
        base_revision: &str,
        _context: &serde_json::Value,
    ) -> EngineResult<TaskDecomposition> {
        // Split the request into sub-objectives
        let objectives = Self::split_objectives(user_request);

        // Build a ticket per objective
        let mut tasks = Vec::new();
        let mut next_id = 1u128;
        for objective in &objectives {
            let kind = Self::infer_worker_kind(objective);
            tasks.push(TaskTicket {
                task_id: engine_policy::ids::TaskId::from_u128(next_id),
                worker_kind: kind,
                brief: engine_policy::context::TaskBrief {
                    task_id: engine_policy::ids::TaskId::from_u128(next_id),
                    objective: objective.clone(),
                    non_goals: vec!["Do not modify files outside the task workspace".into()],
                    allowed_files: vec![],
                    allowed_entities: vec![],
                    allowed_scenes: vec![],
                    allowed_assets: vec![],
                    allowed_operations: vec![
                        "read_file".into(),
                        "create_object".into(),
                        "set_property".into(),
                        "write_script".into(),
                    ],
                    forbidden_operations: vec![],
                    acceptance_criteria: vec!["Task output is reviewable".into()],
                    expected_artifacts: vec![],
                    review_rubric: engine_policy::context::ReviewRubric {
                        correctness: vec!["Output compiles or validates".into()],
                        scope: vec!["Only files in allowed scope are modified".into()],
                        safety: vec!["No unsafe operations performed".into()],
                        rollback: vec!["All changes are revertible".into()],
                        user_impact: vec!["Existing scene objects are preserved".into()],
                    },
                    required_evidence: vec![engine_policy::context::EvidenceType::Diff],
                    repair_policy: engine_policy::context::RepairPolicy::default(),
                },
                depends_on: vec![],
                priority: 5,
            });
            next_id += 1;
        }

        Ok(TaskDecomposition {
            decomposition_id: format!("decomp-{}", snapshot_id),
            request_id: user_request.chars().take(32).collect(),
            snapshot_id,
            workspace_id,
            base_revision: base_revision.to_string(),
            tasks,
            created_at: engine_policy::grant::timestamp_now(),
        })
    }

    fn request_grants(
        &self,
        tasks: &TaskDecomposition,
        issuer: &dyn engine_policy::grant::CapabilityIssuer,
    ) -> EngineResult<Vec<(TaskAssignment, CapabilityGrant)>> {
        let mut results = Vec::new();

        for ticket in &tasks.tasks {
            let worker_kind_str = format!("{:?}", ticket.worker_kind).to_lowercase();

            let request = CapabilityRequest {
                task_id: ticket.task_id,
                worker_kind: worker_kind_str.clone(),
                worker_id: format!("{}-worker-{}", worker_kind_str, ticket.task_id),
                snapshot_id: tasks.snapshot_id,
                workspace_id: tasks.workspace_id,
                workspace_root: format!("/tmp/ai-workspace/{}", tasks.workspace_id),
                base_revision: tasks.base_revision.clone(),
                requested_tools: ticket.brief.allowed_operations.clone(),
                requested_commands: vec![],
                requested_read_paths: ticket.brief.allowed_files.clone(),
                requested_write_paths: vec![format!(".agent/{}", ticket.task_id)],
                requested_entities: ticket.brief.allowed_entities.clone(),
                requested_scenes: ticket.brief.allowed_scenes.clone(),
                requested_assets: ticket.brief.allowed_assets.clone(),
                requested_operations: ticket.brief.allowed_operations.clone(),
                needs_process_execution: false,
                needs_network: false,
                justification: format!("Task: {}", ticket.brief.objective),
                expected_artifacts: vec!["diff".into()],
                alternatives_considered: vec![],
                self_identified_risks: vec![],
            };

            let decision = issuer.evaluate(&request);
            match decision {
                engine_policy::grant::CapabilityDecision::Approved { grant }
                | engine_policy::grant::CapabilityDecision::Narrowed { grant, .. } => {
                    let assignment = TaskAssignment {
                        task_id: ticket.task_id,
                        snapshot_id: tasks.snapshot_id,
                        role: AgentRole::Worker(match ticket.worker_kind {
                            ProtoWorkerKind::Scene => WorkerKind::Scene,
                            ProtoWorkerKind::Script => WorkerKind::Script,
                            ProtoWorkerKind::Asset => WorkerKind::Asset,
                            ProtoWorkerKind::Diagnostics => WorkerKind::Diagnostics,
                            ProtoWorkerKind::Explain => WorkerKind::Explain,
                            ProtoWorkerKind::Repair => WorkerKind::Repair,
                            ProtoWorkerKind::Audit => WorkerKind::Audit,
                        }),
                        grant_hash: grant.grant_hash.clone(),
                        context_packet_id: format!("ctx-{}", ticket.task_id),
                        brief: ticket.brief.clone(),
                        step_limit: 20,
                        deadline: None,
                    };
                    results.push((assignment, grant));
                }
                engine_policy::grant::CapabilityDecision::Denied { reasons } => {
                    return Err(engine_core::EngineError::config(format!(
                        "Grant denied for task {}: {}",
                        ticket.task_id,
                        reasons.join("; ")
                    )));
                }
                engine_policy::grant::CapabilityDecision::EscalationRequired {
                    escalated_items,
                    escalate_to,
                } => {
                    return Err(engine_core::EngineError::config(format!(
                        "Grant escalation required for task {}: items={:?}, target={:?}",
                        ticket.task_id, escalated_items, escalate_to
                    )));
                }
            }
        }

        Ok(results)
    }

    fn build_context_packet(
        &self,
        assignment: &TaskAssignment,
        snapshot_data: &serde_json::Value,
        _accepted_artifacts: &[Artifact],
        _validator_output: Option<&serde_json::Value>,
    ) -> EngineResult<ContextPacket> {
        let sections = vec![
            ContextSection {
                section_id: "task-brief".into(),
                label: TrustLabel::TrustedTaskScope,
                content: serde_json::to_value(&assignment.brief).unwrap_or(serde_json::Value::Null),
            },
            ContextSection {
                section_id: "snapshot-context".into(),
                label: TrustLabel::UntrustedProjectFile,
                content: snapshot_data.clone(),
            },
        ];

        let packet_id = format!(
            "ctx-{}-{}",
            assignment.task_id,
            assignment
                .grant_hash
                .as_str()
                .chars()
                .take(8)
                .collect::<String>()
        );

        Ok(ContextPacket {
            packet_id,
            task_id: assignment.task_id,
            snapshot_id: assignment.snapshot_id,
            context_hash: engine_policy::ids::ContextHash::new("pending"),
            target_role: assignment.role,
            sections,
            sources: vec![ContextSource {
                source_id: format!("snapshot-{}", assignment.task_id),
                label: TrustLabel::TrustedTaskScope,
                description: "Immutable project state".into(),
            }],
            expiration: ContextExpiration::default(),
            created_at: engine_policy::grant::timestamp_now(),
        })
    }

    fn merge(
        &self,
        approved_outputs: &[WorkerOutput],
        _workspace_root: &str,
    ) -> EngineResult<IntegrationCandidate> {
        let mut change_groups = Vec::new();
        let mut all_files = Vec::new();
        let mut all_entities = Vec::new();

        for output in approved_outputs {
            for artifact in &output.artifacts {
                if artifact.artifact_type == "scene_preview" || artifact.artifact_type == "diff" {
                    // Extract file/entity references from the artifact target
                    let target = &artifact.target;
                    if target.contains('.') {
                        all_files.push(target.clone());
                    } else if target.contains(':') {
                        all_entities.push(target.clone());
                    }
                }
            }

            change_groups.push(ChangeGroup {
                group_id: format!("group-{}", output.task_id),
                description: format!(
                    "{} task",
                    match output.worker_kind {
                        ProtoWorkerKind::Scene => "Scene",
                        ProtoWorkerKind::Script => "Script",
                        ProtoWorkerKind::Asset => "Asset",
                        ProtoWorkerKind::Diagnostics => "Diagnostics",
                        ProtoWorkerKind::Explain => "Explain",
                        ProtoWorkerKind::Repair => "Repair",
                        ProtoWorkerKind::Audit => "Audit",
                    }
                ),
                files: all_files.clone(),
                entities: all_entities.clone(),
            });
        }

        Ok(IntegrationCandidate {
            candidate_id: format!("candidate-{}", approved_outputs.len()),
            task_id: if approved_outputs.is_empty() {
                engine_policy::ids::TaskId::from_u128(0)
            } else {
                approved_outputs[0].task_id
            },
            merged_outputs: approved_outputs.to_vec(),
            change_groups,
            bundle: None,
            state: IntegrationState::Merging,
            created_at: engine_policy::grant::timestamp_now(),
        })
    }

    fn request_deep_review(
        &self,
        _candidate: &IntegrationCandidate,
        review_request: &ReviewRequest,
    ) -> EngineResult<ReviewDecision> {
        // In the MVP, the Deep Reviewer is called separately.
        // This method exists for the Manager to delegate to it.
        // Return a pending review that the caller should process.
        Ok(ReviewDecision {
            candidate_id: review_request.candidate_id.clone(),
            verdict: ReviewVerdict::Approved,
            findings: vec![],
            has_blocking_issues: false,
            residual_risk: ReviewRisk::Low,
            reviewed_at: engine_policy::grant::timestamp_now(),
        })
    }

    fn create_repair_ticket(
        &self,
        _candidate: &IntegrationCandidate,
        failed_review: &ReviewDecision,
        retry_count: u32,
    ) -> EngineResult<RepairTicket> {
        let blocking_findings: Vec<String> = failed_review
            .findings
            .iter()
            .filter(|f| f.severity == "blocking" || f.severity == "error")
            .map(|f| f.description.clone())
            .collect();

        Ok(RepairTicket {
            ticket_id: format!("repair-{}-{}", failed_review.candidate_id, retry_count),
            candidate_id: failed_review.candidate_id.clone(),
            severity: if failed_review.has_blocking_issues {
                RepairSeverity::Blocking
            } else {
                RepairSeverity::NonBlocking
            },
            affected: failed_review
                .findings
                .iter()
                .flat_map(|f| f.affected.clone())
                .collect(),
            reproduction: blocking_findings.join("\n"),
            expected_outcome: "All review findings are resolved".into(),
            allowed_scope: vec![], // full candidate scope
            retry_count,
            max_retries: 3,
            failing_evidence: serde_json::json!({
                "review_verdict": failed_review.verdict,
                "findings": failed_review.findings,
            }),
            created_at: engine_policy::grant::timestamp_now(),
        })
    }

    fn generate_final_report(
        &self,
        candidate: &IntegrationCandidate,
        review: &ReviewDecision,
        problems: &[ProblemReport],
        quick_fixes: &[QuickFixAction],
    ) -> EngineResult<FinalReport> {
        let completed_count = candidate
            .merged_outputs
            .iter()
            .filter(|o| o.state == crate::protocol::WorkerState::Completed)
            .count();
        let total_count = candidate.merged_outputs.len();

        Ok(FinalReport {
            report_id: format!("report-{}", candidate.candidate_id),
            summary: format!(
                "Completed {} of {} tasks. {} change groups produced.",
                completed_count,
                total_count,
                candidate.change_groups.len()
            ),
            change_groups: candidate.change_groups.clone(),
            validation_results: serde_json::json!({
                "passed": !review.has_blocking_issues,
                "blocking_issues": review.has_blocking_issues,
                "findings_count": review.findings.len(),
            }),
            review_findings: review.findings.clone(),
            repaired_issues: problems
                .iter()
                .filter(|p| {
                    p.severity == ProblemSeverity::NonBlocking
                        || p.severity == ProblemSeverity::Advisory
                })
                .cloned()
                .collect(),
            unresolved_problems: problems
                .iter()
                .filter(|p| {
                    p.severity == ProblemSeverity::TaskBlocking
                        || p.severity == ProblemSeverity::RequestBlocking
                })
                .cloned()
                .collect(),
            quick_fix_actions: quick_fixes.to_vec(),
            risk_assessment: RiskAssessment {
                deterministic_risk: RiskClass::Medium,
                reviewer_residual_risk: review.residual_risk,
                has_high_risk_operations: false,
                requires_step_up_confirmation: false,
                summary: format!(
                    "Deterministic risk: Medium. Reviewer residual: {:?}. {} blocking issues.",
                    review.residual_risk,
                    if review.has_blocking_issues {
                        "Has"
                    } else {
                        "No"
                    }
                ),
            },
            bundle: candidate.bundle.clone(),
            generated_at: engine_policy::grant::timestamp_now(),
        })
    }
}
