#![deny(missing_docs)]

//! Optional agent tooling contracts for sandbox, worktree, transaction, and trace smoke paths.

use std::path::{Component, Path, PathBuf};

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

    /// Creates a transactional editor write policy.
    pub const fn transactional_write() -> Self {
        Self {
            write_mode: AgentWriteMode::Transactional,
            filesystem_write: true,
            process_execution: false,
            network: false,
            direct_write: false,
        }
    }

    /// Creates a direct active-project write policy with process and network access.
    pub const fn full_access() -> Self {
        Self {
            write_mode: AgentWriteMode::Direct,
            filesystem_write: true,
            process_execution: true,
            network: true,
            direct_write: true,
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

/// Structured external command requested by an agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentCommand {
    /// Command argv. The first item is the executable name.
    pub argv: Vec<String>,
    /// Working directory for the command.
    pub cwd: PathBuf,
    /// Paths the command is expected to read or write.
    pub touched_paths: Vec<PathBuf>,
    /// Whether the command needs outbound network access.
    pub network_required: bool,
}

impl AgentCommand {
    /// Creates a structured command request.
    pub fn new(argv: impl IntoIterator<Item = impl Into<String>>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
            touched_paths: Vec::new(),
            network_required: false,
        }
    }

    /// Adds expected touched paths.
    pub fn with_touched_paths(
        mut self,
        paths: impl IntoIterator<Item = impl Into<PathBuf>>,
    ) -> Self {
        self.touched_paths = paths.into_iter().map(Into::into).collect();
        self
    }

    /// Marks whether the command needs outbound network access.
    pub const fn with_network_required(mut self, required: bool) -> Self {
        self.network_required = required;
        self
    }
}

/// Deterministic risk classification for an agent command request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentCommandRisk {
    /// Routine command inside the sandbox, such as validation or formatting.
    Low,
    /// Command is not allowlisted but has no obvious sandbox escape.
    Medium,
    /// Command is destructive or writes outside the sandbox.
    High,
    /// Command requests network, shell/interpreter execution, or invalid argv.
    Critical,
}

/// Authorization decision for a structured agent command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentCommandAuthorization {
    /// The command may run under the current sandbox policy.
    AllowSandboxed,
    /// The command requires an explicit user or organization approval.
    RequiresApproval {
        /// Deterministic risk classification.
        risk: AgentCommandRisk,
        /// Why approval is required.
        reason: String,
        /// Codex-style argv prefix that could be approved for future runs.
        suggested_prefix_rule: Option<Vec<String>>,
    },
    /// The command is malformed or cannot be represented safely.
    Deny {
        /// Deterministic risk classification.
        risk: AgentCommandRisk,
        /// Why the command is denied.
        reason: String,
    },
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
        let path = normalize_path(path);
        self.allowed_roots
            .iter()
            .map(|root| normalize_path(root))
            .any(|root| path.starts_with(root))
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

    /// Authorizes a structured command request.
    ///
    /// This mirrors the Codex-style command gate used by Varg's AI
    /// specification: commands are argv arrays, allow rules are argv prefixes,
    /// and sandbox escapes become explicit approval requests.
    pub fn authorize_command(&self, command: &AgentCommand) -> AgentCommandAuthorization {
        if command.argv.is_empty() {
            return AgentCommandAuthorization::Deny {
                risk: AgentCommandRisk::Critical,
                reason: "agent command argv must not be empty".into(),
            };
        }

        if !self.allows_path(&command.cwd) {
            return AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::Critical,
                reason: format!(
                    "agent command cwd '{}' is outside the sandbox",
                    command.cwd.display()
                ),
                suggested_prefix_rule: None,
            };
        }

        if let Some(path) = command
            .touched_paths
            .iter()
            .map(|path| command_path(&command.cwd, path))
            .find(|path| !self.allows_path(path))
        {
            return AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::High,
                reason: format!(
                    "agent command touches '{}' outside the sandbox",
                    path.display()
                ),
                suggested_prefix_rule: None,
            };
        }

        if command.network_required && !self.network_enabled {
            return AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::Critical,
                reason: "agent command requires network but sandbox network is disabled".into(),
                suggested_prefix_rule: suggested_prefix_rule(&command.argv),
            };
        }

        if self.allows_command(&command.argv) {
            return AgentCommandAuthorization::AllowSandboxed;
        }

        let risk = classify_command(&command.argv);
        AgentCommandAuthorization::RequiresApproval {
            risk,
            reason: match risk {
                AgentCommandRisk::Critical => {
                    "agent command uses a shell or interpreter and needs explicit approval".into()
                }
                AgentCommandRisk::High => {
                    "agent command appears destructive and needs explicit approval".into()
                }
                AgentCommandRisk::Medium | AgentCommandRisk::Low => {
                    "agent command is not allowlisted by the sandbox policy".into()
                }
            },
            suggested_prefix_rule: if matches!(
                risk,
                AgentCommandRisk::Critical | AgentCommandRisk::High
            ) {
                None
            } else {
                suggested_prefix_rule(&command.argv)
            },
        }
    }
}

fn classify_command(argv: &[String]) -> AgentCommandRisk {
    let executable = argv[0].as_str();
    if matches!(
        executable,
        "bash" | "sh" | "zsh" | "fish" | "python" | "python3" | "node" | "bun" | "deno"
    ) {
        return AgentCommandRisk::Critical;
    }

    if matches!(
        executable,
        "rm" | "rmdir" | "git" | "cargo" | "npm" | "pnpm" | "yarn"
    ) && argv.iter().skip(1).any(|arg| {
        matches!(
            arg.as_str(),
            "-rf"
                | "-fr"
                | "--force"
                | "reset"
                | "clean"
                | "publish"
                | "login"
                | "owner"
                | "unpublish"
        )
    }) {
        return AgentCommandRisk::High;
    }

    AgentCommandRisk::Medium
}

fn suggested_prefix_rule(argv: &[String]) -> Option<Vec<String>> {
    if argv.is_empty() {
        return None;
    }

    let len = argv.len().min(2);
    Some(argv[..len].to_vec())
}

fn command_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&cwd.join(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
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
        assert!(!sandbox.allows_path(Path::new("/project/../etc/passwd")));
        assert!(sandbox.allows_command(&["cargo".into(), "test".into(), "--workspace".into()]));
        assert!(!sandbox.allows_command(&["cargo".into(), "publish".into()]));
        assert!(!sandbox.allows_network());
    }

    #[test]
    fn sandbox_authorizes_allowlisted_structured_command() {
        let root = PathBuf::from("/project");
        let mut sandbox = SandboxPolicy::new([root.clone()]);
        sandbox.allow_command(["cargo", "test"]);

        let command = AgentCommand::new(["cargo", "test", "--workspace"], root.clone())
            .with_touched_paths([root.join("target")]);

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::AllowSandboxed
        );
    }

    #[test]
    fn sandbox_resolves_relative_touched_paths_from_command_cwd() {
        let root = PathBuf::from("/project");
        let mut sandbox = SandboxPolicy::new([root.clone()]);
        sandbox.allow_command(["cargo", "test"]);

        let command = AgentCommand::new(["cargo", "test"], root.join("crates/engine-editor"))
            .with_touched_paths([PathBuf::from("../../target")]);

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::AllowSandboxed
        );
    }

    #[test]
    fn sandbox_requires_approval_for_network_command_when_network_disabled() {
        let root = PathBuf::from("/project");
        let sandbox = SandboxPolicy::new([root.clone()]);
        let command = AgentCommand::new(["git", "fetch"], root).with_network_required(true);

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::Critical,
                reason: "agent command requires network but sandbox network is disabled".into(),
                suggested_prefix_rule: Some(vec!["git".into(), "fetch".into()]),
            }
        );
    }

    #[test]
    fn sandbox_requires_approval_for_paths_outside_allowed_roots() {
        let sandbox = SandboxPolicy::new([PathBuf::from("/project")]);
        let command = AgentCommand::new(["cargo", "test"], "/project")
            .with_touched_paths([PathBuf::from("/tmp/outside")]);

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::High,
                reason: "agent command touches '/tmp/outside' outside the sandbox".into(),
                suggested_prefix_rule: None,
            }
        );
    }

    #[test]
    fn sandbox_requires_explicit_approval_for_destructive_commands() {
        let sandbox = SandboxPolicy::new([PathBuf::from("/project")]);
        let command = AgentCommand::new(["rm", "-rf", "target"], "/project");

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::RequiresApproval {
                risk: AgentCommandRisk::High,
                reason: "agent command appears destructive and needs explicit approval".into(),
                suggested_prefix_rule: None,
            }
        );
    }

    #[test]
    fn sandbox_denies_empty_command() {
        let sandbox = SandboxPolicy::new([PathBuf::from("/project")]);
        let command = AgentCommand::new(Vec::<String>::new(), "/project");

        assert_eq!(
            sandbox.authorize_command(&command),
            AgentCommandAuthorization::Deny {
                risk: AgentCommandRisk::Critical,
                reason: "agent command argv must not be empty".into(),
            }
        );
    }

    #[test]
    fn worktree_manager_refuses_active_project_path() {
        let mut manager = WorktreeManager::default();
        assert!(
            manager
                .create("/project", "/project", "HEAD", "agent-tools", "agent-a")
                .is_err()
        );

        let worktree = manager
            .create(
                "/project",
                "/tmp/varg-agent/project",
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
