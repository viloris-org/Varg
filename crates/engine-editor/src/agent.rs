#![deny(missing_docs)]

//! Optional agent tooling contracts for sandbox, worktree, transaction, and trace smoke paths.

use std::path::{Path, PathBuf};

use engine_core::{EngineError, EngineResult};

/// Write isolation mode selected for an agent operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentWriteMode {
    /// Observe state only.
    #[default]
    ReadOnly,
    /// Write through rollback-capable editor/runtime services.
    Transactional,
    /// Write in an isolated worktree.
    Worktree,
    /// Write directly to the active project when explicitly permitted.
    Direct,
}

/// Permission policy for an agent invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionPolicy {
    /// Selected write mode.
    pub write_mode: AgentWriteMode,
    /// Whether filesystem writes are permitted by this policy.
    pub filesystem_write: bool,
    /// Whether external process execution is permitted by this policy.
    pub process_execution: bool,
    /// Whether outbound network access is permitted by this policy.
    pub network: bool,
    /// Whether direct active-project writes are permitted.
    pub direct_write: bool,
}

impl PermissionPolicy {
    /// Creates the default read-only policy.
    pub const fn read_only() -> Self {
        Self {
            write_mode: AgentWriteMode::ReadOnly,
            filesystem_write: false,
            process_execution: false,
            network: false,
            direct_write: false,
        }
    }

    /// Creates an isolated worktree write policy.
    pub const fn worktree_write() -> Self {
        Self {
            write_mode: AgentWriteMode::Worktree,
            filesystem_write: true,
            process_execution: false,
            network: false,
            direct_write: false,
        }
    }

    /// Validates whether the policy allows the requested write mode.
    pub fn require_write_mode(&self, requested: AgentWriteMode) -> EngineResult<()> {
        if self.write_mode == requested {
            return Ok(());
        }

        Err(EngineError::config(format!(
            "agent write mode {:?} denied by {:?} policy",
            requested, self.write_mode
        )))
    }
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::read_only()
    }
}

/// Sandbox boundary for file, network, process, and environment access.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SandboxPolicy {
    allowed_roots: Vec<PathBuf>,
    command_allowlist: Vec<Vec<String>>,
    network_enabled: bool,
}

impl SandboxPolicy {
    /// Creates a sandbox with filesystem roots.
    pub fn new(allowed_roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            allowed_roots: allowed_roots.into_iter().collect(),
            command_allowlist: Vec::new(),
            network_enabled: false,
        }
    }

    /// Adds an allowed command prefix.
    pub fn allow_command(&mut self, command: impl IntoIterator<Item = impl Into<String>>) {
        self.command_allowlist
            .push(command.into_iter().map(Into::into).collect());
    }

    /// Enables or disables network access.
    pub fn set_network_enabled(&mut self, enabled: bool) {
        self.network_enabled = enabled;
    }

    /// Returns true when a path is inside an allowed root.
    pub fn allows_path(&self, path: &Path) -> bool {
        self.allowed_roots.iter().any(|root| path.starts_with(root))
    }

    /// Returns true when the command starts with an allowlisted prefix.
    pub fn allows_command(&self, command: &[String]) -> bool {
        self.command_allowlist.iter().any(|prefix| {
            command.len() >= prefix.len()
                && command
                    .iter()
                    .zip(prefix.iter())
                    .all(|(actual, expected)| actual == expected)
        })
    }

    /// Returns true when outbound network access is enabled.
    pub const fn allows_network(&self) -> bool {
        self.network_enabled
    }
}

/// Metadata for an isolated agent worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentWorktree {
    /// Worktree identifier.
    pub id: String,
    /// Source project root.
    pub parent_project: PathBuf,
    /// Isolated worktree path.
    pub path: PathBuf,
    /// Base revision or snapshot identifier.
    pub base_revision: String,
    /// Active build profile.
    pub profile: String,
    /// Creating agent or session identifier.
    pub created_by: String,
}

/// In-memory worktree manager used by smoke tests and first service wiring.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorktreeManager {
    worktrees: Vec<AgentWorktree>,
}

impl WorktreeManager {
    /// Creates a tracked worktree record.
    pub fn create(
        &mut self,
        parent_project: impl Into<PathBuf>,
        path: impl Into<PathBuf>,
        base_revision: impl Into<String>,
        profile: impl Into<String>,
        created_by: impl Into<String>,
    ) -> EngineResult<&AgentWorktree> {
        let worktree = AgentWorktree {
            id: format!("agent-worktree-{}", self.worktrees.len() + 1),
            parent_project: parent_project.into(),
            path: path.into(),
            base_revision: base_revision.into(),
            profile: profile.into(),
            created_by: created_by.into(),
        };

        if worktree.parent_project == worktree.path {
            return Err(EngineError::config(
                "agent worktree must not point at the active project root",
            ));
        }

        self.worktrees.push(worktree);
        Ok(self.worktrees.last().expect("worktree was just pushed"))
    }

    /// Returns all tracked worktrees.
    pub fn worktrees(&self) -> &[AgentWorktree] {
        &self.worktrees
    }
}

/// Transaction lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    /// Transaction is open.
    Open,
    /// Transaction was committed.
    Committed,
    /// Transaction was rolled back.
    RolledBack,
}

/// Transaction record for rollback-capable operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTransaction {
    /// Transaction identifier.
    pub id: String,
    /// Human-readable operation summary.
    pub summary: String,
    /// Current state.
    pub state: TransactionState,
}

/// Trace entry for an agent operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceEntry {
    /// Tool or operation name.
    pub tool: String,
    /// Result summary.
    pub result: String,
    /// Recovery hint for failures or non-rollback operations.
    pub recovery_hint: String,
}

/// In-memory trace recorder.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceRecorder {
    entries: Vec<TraceEntry>,
}

impl TraceRecorder {
    /// Records an operation trace entry.
    pub fn record(
        &mut self,
        tool: impl Into<String>,
        result: impl Into<String>,
        recovery_hint: impl Into<String>,
    ) {
        self.entries.push(TraceEntry {
            tool: tool.into(),
            result: result.into(),
            recovery_hint: recovery_hint.into(),
        });
    }

    /// Returns recorded trace entries.
    pub fn entries(&self) -> &[TraceEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_policy_rejects_write_modes() {
        let policy = PermissionPolicy::read_only();
        assert!(policy.require_write_mode(AgentWriteMode::ReadOnly).is_ok());
        assert!(policy.require_write_mode(AgentWriteMode::Worktree).is_err());
        assert!(!policy.filesystem_write);
        assert!(!policy.direct_write);
    }

    #[test]
    fn sandbox_limits_paths_commands_and_network() {
        let root = PathBuf::from("/project");
        let mut sandbox = SandboxPolicy::new([root.clone()]);
        sandbox.allow_command(["cargo", "test"]);

        assert!(sandbox.allows_path(&root.join("src/lib.rs")));
        assert!(!sandbox.allows_path(Path::new("/etc/passwd")));
        assert!(sandbox.allows_command(&["cargo".into(), "test".into(), "--workspace".into()]));
        assert!(!sandbox.allows_command(&["cargo".into(), "publish".into()]));
        assert!(!sandbox.allows_network());
    }

    #[test]
    fn worktree_manager_refuses_active_project_path() {
        let mut manager = WorktreeManager::default();
        assert!(manager
            .create("/project", "/project", "HEAD", "agent-tools", "agent-a")
            .is_err());

        let worktree = manager
            .create(
                "/project",
                "/tmp/aster-agent/project",
                "HEAD",
                "agent-tools",
                "agent-a",
            )
            .unwrap();

        assert_eq!(worktree.id, "agent-worktree-1");
        assert_eq!(manager.worktrees().len(), 1);
    }

    #[test]
    fn trace_records_recovery_hints() {
        let mut trace = TraceRecorder::default();
        trace.record("worktree.create", "created", "discard worktree");

        assert_eq!(trace.entries()[0].tool, "worktree.create");
        assert_eq!(trace.entries()[0].recovery_hint, "discard worktree");
    }
}
