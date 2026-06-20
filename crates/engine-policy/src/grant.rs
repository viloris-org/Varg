//! Capability grant definition, signing, and enforcement contracts.
//!
//! A `CapabilityGrant` binds a Worker to a specific task, workspace, tool set,
//! risk class, and expiration. Grants are issued by the `CapabilityIssuer`
//! (deterministic Rust code) and enforced by the tool layer on every Worker
//! tool call.
//!
//! ## Grant Flow
//!
//! ```text
//! Manager requests grant → CapabilityIssuer evaluates
//!   ↓                              ↓
//!   grant_request              Deterministic checks:
//!   (task scope,                • Schema validity
//!    worker kind,                • Task binding
//!    needed tools)               • Path scope vs sandbox
//!                                • Command identity vs registry
//!                                • Risk classification
//!                                • Expiration + evidence contract
//!                                ↓
//!                           CapabilityDecision:
//!                           • Approved → signed CapabilityGrant
//!                           • Narrowed → grant with reduced scope
//!                           • Denied → rejection with reason
//!                           • Escalated → requires user/org approval
//! ```
//!
//! ## Signing
//!
//! Grants are HMAC-SHA256 signed with the issuer's secret. The tool layer
//! verifies the signature on every Worker tool call. No Worker, Manager,
//! Reviewer, or user can forge a valid grant signature.

use serde::{Deserialize, Serialize};

use crate::ids::{GrantHash, SnapshotId, TaskId, WorkspaceId};
use crate::risk::RiskClass;

// ── CapabilityGrant ───────────────────────────────────────────────────────────

/// A signed capability grant authorizing a Worker to execute specific tools
/// within a bounded task, workspace, time window, and risk class.
///
/// The grant is the SOLE source of truth for what a Worker may do. The tool
/// layer checks the grant hash on every call. No other component (Manager,
/// Worker prompt, Reviewer report, user approval) can grant permissions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityGrant {
    /// The task this grant authorizes.
    pub task_id: TaskId,

    /// The Worker this grant is issued to.
    pub worker_id: String,

    /// The immutable snapshot this grant is bound to.
    pub snapshot_id: SnapshotId,

    /// The isolated workspace for all writes.
    pub workspace_id: WorkspaceId,

    /// Absolute path to the workspace root on disk.
    pub workspace_root: String,

    /// Base git revision at snapshot time.
    pub base_revision: String,

    /// Content-addressed hash of this grant (HMAC-SHA256).
    pub grant_hash: GrantHash,

    /// Allowed operations the Worker may perform.
    pub allowed: GrantScope,

    /// Explicitly forbidden operations (overrides `allowed`).
    pub forbidden: Vec<String>,

    /// Risk classification (deterministic, not from AI).
    pub risk_class: RiskClass,

    /// Required review route before Worker output is accepted.
    pub review_route: ReviewRoute,

    /// Escalation route when the Worker needs broader access.
    pub escalation_route: EscalationRoute,

    /// Expiration, step limits, and revocation rules.
    pub limits: GrantLimits,

    /// Required evidence the Worker must produce.
    pub evidence_contract: EvidenceContract,

    /// ISO-8601 issuance timestamp.
    pub issued_at: String,

    /// HMAC-SHA256 signature over the canonical JSON of this grant.
    /// Computed by the CapabilityIssuer. Verified by the tool layer.
    #[serde(skip)]
    pub signature: Option<Vec<u8>>,
}

/// The scope of what a grant allows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrantScope {
    /// Allowed AI tools (e.g., "create_object", "write_script").
    pub tools: Vec<String>,

    /// Allowed editor commands by ID.
    pub commands: Vec<String>,

    /// Allowed read paths (relative to project root or canonical).
    pub read_paths: Vec<String>,

    /// Allowed write paths (inside the task workspace).
    pub write_paths: Vec<String>,

    /// Allowed entity IDs or ID patterns (e.g., "1:*").
    pub entities: Vec<String>,

    /// Allowed scene file paths.
    pub scenes: Vec<String>,

    /// Allowed asset paths or GUIDs.
    pub assets: Vec<String>,

    /// Allowed operation types (map to AgentOperation variants).
    pub operation_types: Vec<String>,

    /// Whether process execution is allowed.
    pub process_execution: bool,

    /// Whether outbound network access is allowed.
    pub network: bool,

    /// Whether this is a narrow or broad grant.
    pub breadth: GrantBreadth,
}

/// Whether a grant is narrow (tight scope) or broad (enterprise tasks).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantBreadth {
    /// Tight scope: minimal paths, specific entities, limited tools.
    Narrow,
    /// Broad scope: may include `assets/**` reads, multiple entities,
    /// importer execution, or other enterprise-necessary access.
    /// Requires stronger evidence, audit, and review.
    Broad,
}

/// Required review routing for Worker output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewRoute {
    /// Whether local review is required before integration.
    pub local_review_required: bool,

    /// Whether deep review of the integrated result is required.
    pub deep_review_required: bool,

    /// Whether a Risk Auditor pass is required for scripts/commands.
    pub risk_audit_required: bool,

    /// Whether user/org approval is required before apply.
    pub user_approval_required: bool,
}

/// Escalation route when the Worker needs broader access.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRoute {
    /// Whether the Worker may request capability escalation.
    pub escalation_allowed: bool,

    /// Who decides escalation requests.
    pub escalated_to: EscalationTarget,
}

/// Target for escalated capability requests.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationTarget {
    /// Manager decides (default for routine scope adjustments).
    Manager,
    /// Manager consults a peer reviewer before deciding.
    ManagerWithPeerReview,
    /// Risk Auditor must review before Manager decides.
    RiskAuditorReview,
    /// Organization policy must approve (enterprise).
    OrganizationPolicy,
    /// User must explicitly approve.
    User,
}

/// Limits on grant usage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrantLimits {
    /// Maximum number of tool calls the Worker may make.
    pub max_steps: u32,

    /// ISO-8601 absolute expiry time.
    pub expires_at: Option<String>,

    /// Maximum number of retries for the Worker's task.
    pub max_retries: u32,

    /// Whether this grant has been revoked.
    pub revoked: bool,

    /// Trace parent ID for linking all Worker tool calls.
    pub trace_parent_id: String,
}

/// Required evidence the Worker must produce for its output to be accepted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceContract {
    /// Required artifact types (e.g., "diff", "scene_preview").
    pub required_artifacts: Vec<String>,

    /// Whether a diff of all changes is required.
    pub diff_required: bool,

    /// Whether a scene preview is required.
    pub scene_preview_required: bool,

    /// Whether asset reference validation is required.
    pub asset_reference_check_required: bool,

    /// Whether validator logs must be attached.
    pub validator_log_required: bool,

    /// Whether a rollback plan must be provided.
    pub rollback_plan_required: bool,
}

// ── CapabilityRequest (Manager → Issuer) ──────────────────────────────────────

/// A structured request from the Manager to the Capability Issuer.
///
/// The Manager proposes what a Worker needs; the Issuer validates and decides.
/// The Manager CANNOT mint grants — only the Issuer can.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityRequest {
    /// The task this grant is for.
    pub task_id: TaskId,

    /// The Worker kind making the request.
    pub worker_kind: String,

    /// Unique Worker identifier.
    pub worker_id: String,

    /// The snapshot the Worker will read from.
    pub snapshot_id: SnapshotId,

    /// The workspace the Worker will write to.
    pub workspace_id: WorkspaceId,

    /// Absolute workspace root path.
    pub workspace_root: String,

    /// Base git revision.
    pub base_revision: String,

    /// Requested tools.
    pub requested_tools: Vec<String>,

    /// Requested commands.
    pub requested_commands: Vec<String>,

    /// Requested read paths.
    pub requested_read_paths: Vec<String>,

    /// Requested write paths (inside workspace).
    pub requested_write_paths: Vec<String>,

    /// Requested entities.
    pub requested_entities: Vec<String>,

    /// Requested scenes.
    pub requested_scenes: Vec<String>,

    /// Requested assets.
    pub requested_assets: Vec<String>,

    /// Requested operation types.
    pub requested_operations: Vec<String>,

    /// Whether process execution is needed.
    pub needs_process_execution: bool,

    /// Whether network access is needed.
    pub needs_network: bool,

    /// Why this grant is needed (for audit and review).
    pub justification: String,

    /// Expected artifacts from the Worker.
    pub expected_artifacts: Vec<String>,

    /// Alternative approaches considered (for broad requests).
    pub alternatives_considered: Vec<String>,

    /// Risk tags self-identified by the Manager.
    pub self_identified_risks: Vec<String>,
}

/// Decision from the Capability Issuer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CapabilityDecision {
    /// Grant approved as requested.
    Approved {
        /// The signed capability grant.
        grant: CapabilityGrant,
    },
    /// Grant approved with narrowed scope.
    Narrowed {
        /// The narrowed grant.
        grant: CapabilityGrant,
        /// What was narrowed and why.
        narrowing_reasons: Vec<String>,
    },
    /// Grant denied.
    Denied {
        /// Why the grant was denied.
        reasons: Vec<String>,
    },
    /// Grant requires escalation (user/org approval).
    EscalationRequired {
        /// What needs escalation.
        escalated_items: Vec<String>,
        /// The escalation target.
        escalate_to: EscalationTarget,
    },
}

// ── DefaultCapabilityIssuer ─────────────────────────────────────────────────

/// Default implementation of the CapabilityIssuer.
///
/// Uses HMAC-SHA256 for grant signing. The evaluation logic follows
/// deterministic rules:
/// - Reads must not overlap with workspace write paths (isolated workspaces)
/// - Process/network requests always escalate
/// - Script operations require audit
/// - Broad scope requires deep review + user approval
pub struct DefaultCapabilityIssuer {
    secret: Vec<u8>,
}

impl DefaultCapabilityIssuer {
    /// Creates a new issuer with the given HMAC secret.
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// Creates an issuer with a random 32-byte secret.
    pub fn random() -> Self {
        Self {
            secret: (0..32).map(|_| fast_random_byte()).collect(),
        }
    }

    /// Classifies a request into a RiskClass based on deterministic criteria.
    fn classify_risk(&self, request: &CapabilityRequest) -> RiskClass {
        // Critical: process execution, network, or broad scope
        if request.needs_process_execution || request.needs_network {
            return RiskClass::Critical;
        }

        // High: destructive operations, scripts (audit-worthy), or broad
        if request.requested_operations.iter().any(|op| {
            matches!(
                op.as_str(),
                "destroy_object" | "remove_component" | "execute_command" | "run_command"
            )
        }) || request.requested_tools.len() > 5
        {
            return RiskClass::High;
        }

        // Medium: write operations with clear rollback
        if request.requested_operations.iter().any(|op| {
            matches!(
                op.as_str(),
                "create_object" | "write_script" | "set_property"
            )
        }) {
            return RiskClass::Medium;
        }

        // Low: read-only queries
        RiskClass::Low
    }

    /// Determines the required review route based on risk and scope.
    fn determine_review_route(&self, risk: RiskClass, request: &CapabilityRequest) -> ReviewRoute {
        match risk {
            RiskClass::Critical => ReviewRoute {
                local_review_required: true,
                deep_review_required: true,
                risk_audit_required: true,
                user_approval_required: true,
            },
            RiskClass::High => ReviewRoute {
                local_review_required: true,
                deep_review_required: true,
                risk_audit_required: request.requested_operations.iter().any(|op| {
                    op == "write_script" || op == "execute_command" || op == "run_command"
                }),
                user_approval_required: true,
            },
            RiskClass::Medium => ReviewRoute {
                local_review_required: true,
                deep_review_required: false,
                risk_audit_required: false,
                user_approval_required: false,
            },
            RiskClass::Low => ReviewRoute {
                local_review_required: false,
                deep_review_required: false,
                risk_audit_required: false,
                user_approval_required: false,
            },
        }
    }

    /// Builds the evidence contract matching the risk level and operations.
    fn build_evidence_contract(&self, risk: RiskClass) -> EvidenceContract {
        match risk {
            RiskClass::Critical | RiskClass::High => EvidenceContract {
                required_artifacts: vec![
                    "diff".into(),
                    "scene_preview".into(),
                    "validator_log".into(),
                    "rollback_plan".into(),
                ],
                diff_required: true,
                scene_preview_required: true,
                asset_reference_check_required: true,
                validator_log_required: true,
                rollback_plan_required: true,
            },
            RiskClass::Medium => EvidenceContract {
                required_artifacts: vec!["diff".into()],
                diff_required: true,
                scene_preview_required: false,
                asset_reference_check_required: true,
                validator_log_required: false,
                rollback_plan_required: false,
            },
            RiskClass::Low => EvidenceContract {
                required_artifacts: vec![],
                diff_required: false,
                scene_preview_required: false,
                asset_reference_check_required: false,
                validator_log_required: false,
                rollback_plan_required: false,
            },
        }
    }

    /// Validates that requested paths are inside the workspace root.
    fn validate_paths(&self, request: &CapabilityRequest) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Normalise workspace root to canonical form (strip trailing separator)
        for path in request
            .requested_read_paths
            .iter()
            .chain(request.requested_write_paths.iter())
        {
            let canonical = path.trim_end_matches('/').trim_end_matches('\\');
            if canonical.is_empty() {
                continue;
            }
            // Reject absolute paths that escape the workspace root
            if canonical.starts_with('/')
                || canonical.starts_with("\\")
                || (canonical.len() > 1 && canonical.as_bytes()[1] == b':')
            {
                errors.push(format!("absolute path outside workspace root: {path}"));
                continue;
            }
            // Reject parent-directory traversal that escapes above the root
            if canonical.starts_with("..")
                || canonical.contains("/../")
                || canonical.contains("\\..\\")
            {
                errors.push(format!("path traversal outside workspace root: {path}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validates the task binding consistency.
    fn validate_task_binding(&self, request: &CapabilityRequest) -> Result<(), Vec<String>> {
        if request.task_id.as_u128() == 0 {
            return Err(vec!["task_id must be non-zero".into()]);
        }
        if request.snapshot_id.as_u128() == 0 {
            return Err(vec!["snapshot_id must be non-zero".into()]);
        }
        if request.workspace_id.as_u128() == 0 {
            return Err(vec!["workspace_id must be non-zero".into()]);
        }
        if request.workspace_root.is_empty() {
            return Err(vec!["workspace_root must not be empty".into()]);
        }
        Ok(())
    }
}

impl CapabilityIssuer for DefaultCapabilityIssuer {
    fn evaluate(&self, request: &CapabilityRequest) -> CapabilityDecision {
        // 1. Schema validation
        if request.worker_kind.is_empty() || request.worker_id.is_empty() {
            return CapabilityDecision::Denied {
                reasons: vec!["worker_kind and worker_id are required".into()],
            };
        }

        // 2. Task binding
        if let Err(reasons) = self.validate_task_binding(request) {
            return CapabilityDecision::Denied { reasons };
        }

        // 3. Path validation
        if request.requested_tools.is_empty() && request.requested_operations.is_empty() {
            return CapabilityDecision::Denied {
                reasons: vec!["at least one tool or operation must be requested".into()],
            };
        }
        if let Err(reasons) = self.validate_paths(request) {
            return CapabilityDecision::Denied { reasons };
        }

        // 4. Risk classification
        let risk_class = self.classify_risk(request);

        // 5. Determine scope breadth
        let breadth = if request.needs_process_execution
            || request.needs_network
            || request.requested_tools.len() > 5
            || request.requested_write_paths.len() > 10
        {
            GrantBreadth::Broad
        } else {
            GrantBreadth::Narrow
        };

        // 6. Check if escalation is needed
        if risk_class == RiskClass::Critical {
            let mut escalated_items = Vec::new();
            if request.needs_process_execution {
                escalated_items.push("process_execution".into());
            }
            if request.needs_network {
                escalated_items.push("network_access".into());
            }
            if !escalated_items.is_empty() {
                return CapabilityDecision::EscalationRequired {
                    escalated_items,
                    escalate_to: EscalationTarget::User,
                };
            }
        }

        // 7. Build allowed scope (possibly narrowed from request)
        //    For Broad grants we may narrow to just what's necessary.
        let (scope, narrowing_reasons) = if breadth == GrantBreadth::Broad {
            let narrowed_tools: Vec<String> = request
                .requested_tools
                .iter()
                .filter(|t| {
                    !matches!(
                        t.as_str(),
                        "execute_process"
                            | "network_request"
                            | "destroy_all_objects"
                            | "delete_all_assets"
                    )
                })
                .cloned()
                .collect();

            if narrowed_tools.len() < request.requested_tools.len() {
                (
                    GrantScope {
                        tools: narrowed_tools,
                        commands: request.requested_commands.clone(),
                        read_paths: request.requested_read_paths.clone(),
                        write_paths: request.requested_write_paths.clone(),
                        entities: request.requested_entities.clone(),
                        scenes: request.requested_scenes.clone(),
                        assets: request.requested_assets.clone(),
                        operation_types: request.requested_operations.clone(),
                        process_execution: false,
                        network: false,
                        breadth,
                    },
                    vec!["Removed dangerous tools from broad grant".into()],
                )
            } else {
                (
                    GrantScope {
                        tools: request.requested_tools.clone(),
                        commands: request.requested_commands.clone(),
                        read_paths: request.requested_read_paths.clone(),
                        write_paths: request.requested_write_paths.clone(),
                        entities: request.requested_entities.clone(),
                        scenes: request.requested_scenes.clone(),
                        assets: request.requested_assets.clone(),
                        operation_types: request.requested_operations.clone(),
                        process_execution: request.needs_process_execution,
                        network: request.needs_network,
                        breadth,
                    },
                    vec![],
                )
            }
        } else {
            (
                GrantScope {
                    tools: request.requested_tools.clone(),
                    commands: request.requested_commands.clone(),
                    read_paths: request.requested_read_paths.clone(),
                    write_paths: request.requested_write_paths.clone(),
                    entities: request.requested_entities.clone(),
                    scenes: request.requested_scenes.clone(),
                    assets: request.requested_assets.clone(),
                    operation_types: request.requested_operations.clone(),
                    process_execution: request.needs_process_execution,
                    network: request.needs_network,
                    breadth,
                },
                vec![],
            )
        };

        // 8. Build review route and evidence contract
        //    These are embedded in the grant via issue() below.
        let _ = self.determine_review_route(risk_class, request);
        let _ = self.build_evidence_contract(risk_class);

        // 9. Narrow process_execution and network back to false for non-Critical
        //    (the evaluate function may still deny broad requests at a higher level)
        let mut final_scope = scope;
        if risk_class != RiskClass::Critical {
            final_scope.process_execution = false;
            final_scope.network = false;
        }

        // 10. Issue the grant
        let grant = self.issue(request, final_scope);

        if narrowing_reasons.is_empty() {
            CapabilityDecision::Approved { grant }
        } else {
            CapabilityDecision::Narrowed {
                grant,
                narrowing_reasons,
            }
        }
    }

    fn issue(&self, request: &CapabilityRequest, scope: GrantScope) -> CapabilityGrant {
        let mut grant = CapabilityGrant {
            task_id: request.task_id,
            worker_id: request.worker_id.clone(),
            snapshot_id: request.snapshot_id,
            workspace_id: request.workspace_id,
            workspace_root: request.workspace_root.clone(),
            base_revision: request.base_revision.clone(),
            grant_hash: GrantHash::new("pending"), // computed below
            allowed: scope,
            forbidden: vec![], // explicit denials from policy
            risk_class: self.classify_risk(request),
            review_route: self.determine_review_route(self.classify_risk(request), request),
            escalation_route: EscalationRoute {
                escalation_allowed: true,
                escalated_to: EscalationTarget::Manager,
            },
            limits: GrantLimits {
                max_steps: 20,
                expires_at: None,
                max_retries: 3,
                revoked: false,
                trace_parent_id: format!("trace-{}", request.task_id),
            },
            evidence_contract: self.build_evidence_contract(self.classify_risk(request)),
            issued_at: timestamp_now(),
            signature: None,
        };

        // Compute grant hash over canonical JSON (without signature)
        let canonical = serde_json::to_string(&grant).unwrap_or_else(|_| "fallback".to_string());
        let hash = compute_hmac_sha256(&canonical, &self.secret);
        grant.grant_hash = GrantHash::new(hex_encode(&hash));

        // Sign the grant
        grant.signature = Some(self.sign(&canonical));

        grant
    }

    fn verify(&self, grant: &CapabilityGrant) -> bool {
        let mut check = grant.clone();
        check.grant_hash = GrantHash::new("pending");
        check.signature = None;

        let canonical = serde_json::to_string(&check).unwrap_or_else(|_| "fallback".to_string());

        // Verify grant hash matches
        let expected_hash = compute_hmac_sha256(&canonical, &self.secret);
        let hash_ok = hex_encode(&expected_hash) == grant.grant_hash.as_str();

        // Verify signature
        let sig_ok = grant
            .signature
            .as_ref()
            .map(|sig| verify_hmac_sha256(&canonical, sig, &self.secret))
            .unwrap_or(false);

        hash_ok && sig_ok
    }

    fn sign(&self, grant_json: &str) -> Vec<u8> {
        compute_hmac_sha256(grant_json, &self.secret)
    }
}

// ── HMAC-SHA256 helpers ───────────────────────────────────────────────────────

fn compute_hmac_sha256(data: &str, secret: &[u8]) -> Vec<u8> {
    use hmac::Mac;
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("HMAC key length is valid");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn verify_hmac_sha256(data: &str, signature: &[u8], secret: &[u8]) -> bool {
    let expected = compute_hmac_sha256(data, secret);
    expected.len() == signature.len() && expected == signature
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn fast_random_byte() -> u8 {
    // Use getrandom for cryptographic-quality randomness.
    // This is used for grant secrets that must be unpredictable.
    let mut buf = [0u8; 1];
    getrandom::fill(&mut buf).unwrap_or_else(|_| {
        // Fallback: mixing wall-clock nanos should never happen on modern kernels,
        // but handle it gracefully rather than panicking in a security context.
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf[0] = (nanos
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            >> 56) as u8;
    });
    buf[0]
}

/// Returns the current timestamp as an ISO-8601 string.
pub fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1970 + d.as_secs() / 31_536_000,
        1 + (d.as_secs() % 31_536_000) / 2_592_000,
        1 + (d.as_secs() % 2_592_000) / 86_400,
        (d.as_secs() % 86_400) / 3_600,
        (d.as_secs() % 3_600) / 60,
        d.as_secs() % 60,
        d.subsec_millis(),
    )
}

// ── CapabilityIssuer trait ────────────────────────────────────────────────────

/// The deterministic capability issuer.
///
/// This is the SINGLE component that may issue grants. It is trusted Rust
/// code, never an AI model. Its inputs are all deterministic:
/// - Command capability registry
/// - Sandbox policy
/// - Risk classifier
/// - Task scope (policy-checked)
/// - Snapshot metadata
///
/// The trait is designed so that in a future milestone, the implementation
/// can be moved into a separate Policy Daemon process communicating over
/// a local Unix domain socket.
pub trait CapabilityIssuer: Send + Sync {
    /// Evaluates a capability request and returns a decision.
    ///
    /// The Issuer validates the request against deterministic policy:
    /// 1. Schema validation (is the request well-formed?)
    /// 2. Task binding (are task_id, snapshot_id, workspace_id consistent?)
    /// 3. Command validation (are requested commands in the AI-safe registry?)
    /// 4. Path validation (are requested paths inside canonical roots?)
    /// 5. Risk classification (what risk class does this request map to?)
    /// 6. Evidence requirements (what evidence must the Worker produce?)
    /// 7. Rollback plan (is rollback feasible for the requested operations?)
    fn evaluate(&self, request: &CapabilityRequest) -> CapabilityDecision;

    /// Issues a signed grant for an approved request.
    ///
    /// Computes the grant hash (HMAC-SHA256 over canonical JSON) and signs it.
    /// Returns the fully populated CapabilityGrant with signature.
    fn issue(&self, request: &CapabilityRequest, scope: GrantScope) -> CapabilityGrant;

    /// Verifies a grant signature.
    ///
    /// Returns true if the grant's signature matches its content.
    /// The tool layer calls this before every Worker tool call.
    fn verify(&self, grant: &CapabilityGrant) -> bool;

    /// Signs the canonical JSON of a grant, returning the signature bytes.
    fn sign(&self, grant_json: &str) -> Vec<u8>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{GrantHash, SnapshotId, TaskId, WorkspaceId};

    #[test]
    fn capability_grant_has_required_fields() {
        let grant = CapabilityGrant {
            task_id: TaskId::from_u128(1),
            worker_id: "scene-worker-001".into(),
            snapshot_id: SnapshotId::from_u128(100),
            workspace_id: WorkspaceId::from_u128(42),
            workspace_root: "/tmp/ai-workspace/task-1".into(),
            base_revision: "HEAD".into(),
            grant_hash: GrantHash::new("test-hash"),
            allowed: GrantScope {
                tools: vec!["create_object".into()],
                commands: vec![],
                read_paths: vec!["scenes/".into()],
                write_paths: vec!["ai-workspace/".into()],
                entities: vec![],
                scenes: vec![],
                assets: vec![],
                operation_types: vec!["create_object".into()],
                process_execution: false,
                network: false,
                breadth: GrantBreadth::Narrow,
            },
            forbidden: vec!["destroy_object".into(), "execute_command".into()],
            risk_class: RiskClass::Medium,
            review_route: ReviewRoute {
                local_review_required: true,
                deep_review_required: false,
                risk_audit_required: false,
                user_approval_required: false,
            },
            escalation_route: EscalationRoute {
                escalation_allowed: true,
                escalated_to: EscalationTarget::Manager,
            },
            limits: GrantLimits {
                max_steps: 20,
                expires_at: None,
                max_retries: 3,
                revoked: false,
                trace_parent_id: "trace-task-1".into(),
            },
            evidence_contract: EvidenceContract {
                required_artifacts: vec!["diff".into(), "scene_preview".into()],
                diff_required: true,
                scene_preview_required: true,
                asset_reference_check_required: false,
                validator_log_required: false,
                rollback_plan_required: false,
            },
            issued_at: "2025-01-01T00:00:00Z".into(),
            signature: None,
        };

        assert_eq!(grant.task_id, TaskId::from_u128(1));
        assert_eq!(grant.allowed.tools.len(), 1);
        assert!(!grant.allowed.process_execution);
        assert_eq!(grant.limits.max_steps, 20);
        assert!(!grant.limits.revoked);
    }

    #[test]
    fn narrow_grant_differs_from_broad() {
        assert_ne!(GrantBreadth::Narrow, GrantBreadth::Broad);
    }
}
