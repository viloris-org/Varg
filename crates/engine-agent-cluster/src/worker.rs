//! Worker trait and concrete Worker type definitions.
//!
//! Workers are specialized AI agents that execute bounded tasks inside
//! an isolated git-backed task workspace. Each Worker receives a fresh
//! session with only its role-specific context packet — never the
//! Manager's full conversation, sibling Worker chat, or raw user prompt.
//!
//! ## Worker Lifecycle
//!
//! ```text
//! 1. Manager builds TaskAssignment + ContextPacket
//! 2. Session orchestrator spawns fresh session with ContextPacket
//! 3. Worker receives ONLY: task brief, allowed scope, evidence references
//! 4. Worker executes tool calls through the tool layer (grant enforcement)
//! 5. Worker produces structured WorkerOutput with objective artifacts
//! 6. Worker output passes through local review before integration
//! ```
//!
//! ## Worker Isolation
//!
//! - Workers cannot read from mutable live project state after snapshot.
//! - Workers write only to the assigned git-backed task workspace.
//! - Workers cannot broaden their own permissions — only the Capability
//!   Issuer can issue or expand grants.
//! - If a Worker needs access outside its grant, it must stop and emit
//!   a structured capability escalation request.

use engine_core::EngineResult;

use crate::protocol::{TaskAssignment, WorkerOutput};

/// A specialized AI Worker that executes a bounded task.
///
/// Implementations use an AI model to perform work but are constrained
/// by the task assignment, capability grant, and sandbox. The Worker
/// trait enforces structured output — free-form prose is captured only
/// as an untrusted self-report alongside objective artifacts.
pub trait Worker {
    /// Returns the Worker kind this implementation handles.
    fn kind(&self) -> crate::protocol::WorkerKind;

    /// Executes the assigned task and returns structured output.
    ///
    /// The Worker receives:
    /// - `assignment`: the task brief, grant hash, step limit, deadline.
    /// - `context_packet_json`: the role-specific context packet as JSON.
    ///
    /// The Worker must:
    /// - Stay within the allowed scope defined in the task brief.
    /// - Carry the grant hash on every tool call.
    /// - Stop and report if it hits the step limit, deadline, or an
    ///   unrecoverable error.
    /// - Produce objective artifacts (diffs, scene previews, etc.) rather
    ///   than relying on prose claims.
    fn execute(
        &self,
        assignment: &TaskAssignment,
        context_packet_json: &serde_json::Value,
    ) -> EngineResult<WorkerOutput>;

    /// Requests capability escalation from the Manager.
    ///
    /// Called when the Worker needs access outside its current grant.
    /// The Worker must justify why the additional access is needed and
    /// what alternatives were considered.
    fn request_escalation(
        &self,
        task_id: engine_policy::ids::TaskId,
        needed_capability: &str,
        justification: &str,
        alternatives_considered: &[String],
    ) -> EngineResult<escalation::CapabilityEscalationRequest>;
}

/// Capability escalation request from a Worker to the Manager.
pub mod escalation {
    use engine_policy::ids::TaskId;
    use serde::{Deserialize, Serialize};

    /// A structured request for broader access than the current grant.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CapabilityEscalationRequest {
        /// The task requesting escalation.
        pub task_id: TaskId,

        /// The Worker requesting escalation.
        pub worker_id: String,

        /// The capability being requested.
        pub requested_capability: String,

        /// Why this capability is necessary for task completion.
        pub justification: String,

        /// The minimal viable scope (narrower than "everything").
        pub minimal_scope: String,

        /// Expected artifact if the capability is granted.
        pub expected_artifact: String,

        /// Risk self-assessment by the Worker.
        pub risk_tags: Vec<String>,

        /// Alternative approaches that were considered and rejected.
        pub alternatives_considered: Vec<String>,

        /// Impact on rollback if the capability is granted.
        pub rollback_impact: String,

        /// Impact on the evidence contract if the capability is granted.
        pub evidence_impact: String,

        /// Impact on the original task objective.
        pub task_impact: String,
    }
}

/// Concrete Worker implementation that delegates to an AI model.
///
/// This is the standard Worker used in Auto mode. It:
/// 1. Receives the context packet as a system prompt
/// 2. Processes the task brief
/// 3. Calls tools through the tool layer (with grant enforcement)
/// 4. Produces structured WorkerOutput
///
/// In a test environment, this can be replaced with a deterministic stub.
pub struct ModelWorker<M: crate::ModelProvider> {
    kind: crate::protocol::WorkerKind,
    model: M,
    max_tool_calls: u32,
}

/// Abstraction over the AI model provider, used by Workers and the Manager.
///
/// This is a simplified trait used within the cluster crate. The full
/// `AiModel` trait in `engine-ai` can implement this via an adapter.
pub trait ModelProvider {
    /// Sends a prompt and receives a response.
    fn generate(&self, system: &str, user: &str) -> EngineResult<String>;
}

/// Adapts an `engine_ai::AiModel` (full request/response) to the simplified
/// `ModelProvider` trait (string prompt → string response).
///
/// Any type implementing `engine_ai::AiModel` can be wrapped and used
/// wherever `ModelProvider` is expected — Workers, the Deep Reviewer, etc.
pub struct AiModelAdapter<M> {
    inner: M,
}

impl<M> AiModelAdapter<M>
where
    M: engine_ai::AiModel,
{
    /// Wraps an AiModel for use as a ModelProvider.
    pub fn new(model: M) -> Self {
        Self { inner: model }
    }
}

impl<M> ModelProvider for AiModelAdapter<M>
where
    M: engine_ai::AiModel,
{
    fn generate(&self, system: &str, user: &str) -> EngineResult<String> {
        let request = engine_ai::AiRequest::single_turn(
            system.to_string(),
            serde_json::Value::Null,
            user.to_string(),
        );
        let response = self.inner.chat(request)?;
        Ok(response.content)
    }
}
