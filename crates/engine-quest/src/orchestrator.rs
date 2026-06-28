//! Durable Quest orchestration interfaces.
//!
//! The orchestrator owns persistent run state. Hosts provide adapters for
//! model execution, workspace creation, validation, and UI/event publication.

use std::path::PathBuf;

use engine_core::EngineResult;

use crate::{
    ValidationResult,
    runtime::{QuestRun, QuestRunStatus, QuestRuntime, QuestRuntimeEvent},
};

/// Isolated workspace prepared for a Quest run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuestWorkspace {
    /// Stable workspace id stored on Quest run state.
    pub id: String,
    /// Filesystem root for this isolated workspace.
    pub root: PathBuf,
}

/// Host-provided adapters needed by the Quest orchestrator.
pub trait QuestHost {
    /// Prepares or resumes an isolated workspace for a Quest run.
    fn prepare_workspace(&self, quest_id: &str, run_id: &str) -> EngineResult<QuestWorkspace>;

    /// Emits an already-recorded Quest runtime event to the host UI.
    fn emit_event(&self, event: &QuestRuntimeEvent) -> EngineResult<()>;

    /// Validates an isolated workspace and returns review evidence.
    fn validate_workspace(&self, workspace: &QuestWorkspace)
    -> EngineResult<Vec<ValidationResult>>;
}

/// Engine-level durable Quest orchestrator.
#[derive(Clone, Debug)]
pub struct QuestOrchestrator {
    runtime: QuestRuntime,
}

impl QuestOrchestrator {
    /// Creates a Quest orchestrator backed by the provided runtime.
    pub fn new(runtime: QuestRuntime) -> Self {
        Self { runtime }
    }

    /// Starts durable execution state for a Quest objective.
    pub fn start_run(
        &self,
        host: &dyn QuestHost,
        quest_id: &str,
        objective: &str,
    ) -> EngineResult<QuestRun> {
        let mut run = self.runtime.start_run(quest_id, objective)?;
        let workspace = host.prepare_workspace(quest_id, &run.id)?;
        run.workspace_id = Some(workspace.id);
        run.active_step = Some("prepare_workspace".to_owned());
        self.runtime.update_run(&mut run)?;
        if let Some(event) = self.runtime.events(quest_id)?.last() {
            host.emit_event(event)?;
        }
        Ok(run)
    }

    /// Records validation evidence against a durable Quest run.
    pub fn validate_run(
        &self,
        host: &dyn QuestHost,
        run: &mut QuestRun,
        workspace: &QuestWorkspace,
    ) -> EngineResult<Vec<ValidationResult>> {
        run.active_step = Some("validate_workspace".to_owned());
        self.runtime.update_run(run)?;
        let validations = host.validate_workspace(workspace)?;
        run.status = if validations
            .iter()
            .any(|validation| validation.status == "failed")
        {
            QuestRunStatus::Blocked
        } else {
            QuestRunStatus::ReadyForReview
        };
        self.runtime.update_run(run)?;
        Ok(validations)
    }

    /// Returns the underlying runtime.
    pub fn runtime(&self) -> &QuestRuntime {
        &self.runtime
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, fs};

    use super::*;

    #[derive(Default)]
    struct TestHost {
        emitted: RefCell<Vec<String>>,
    }

    impl QuestHost for TestHost {
        fn prepare_workspace(&self, quest_id: &str, run_id: &str) -> EngineResult<QuestWorkspace> {
            Ok(QuestWorkspace {
                id: format!("{quest_id}-{run_id}"),
                root: std::env::temp_dir(),
            })
        }

        fn emit_event(&self, event: &QuestRuntimeEvent) -> EngineResult<()> {
            self.emitted.borrow_mut().push(event.kind.clone());
            Ok(())
        }

        fn validate_workspace(
            &self,
            _workspace: &QuestWorkspace,
        ) -> EngineResult<Vec<ValidationResult>> {
            Ok(vec![ValidationResult::new("smoke", "passed", "ok")])
        }
    }

    #[test]
    fn orchestrator_starts_run_with_workspace() {
        let root =
            std::env::temp_dir().join(format!("varg-quest-orchestrator-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let orchestrator = QuestOrchestrator::new(QuestRuntime::new(&root));
        let host = TestHost::default();

        let run = orchestrator
            .start_run(&host, "quest-orchestrator-test", "Create a level")
            .unwrap();

        assert_eq!(run.active_step.as_deref(), Some("prepare_workspace"));
        assert!(run.workspace_id.is_some());
        assert_eq!(host.emitted.borrow().as_slice(), ["run_updated"]);

        let _ = fs::remove_dir_all(root);
    }
}
