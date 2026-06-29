//! Engine task runtime for CPU work that should not block frame-owned threads.

use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

/// Priority bucket used by the engine task runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TaskPriority {
    /// Latency-sensitive work that should be picked before normal tasks.
    High,
    /// Default CPU work.
    Normal,
    /// Background work that should not compete with frame-critical tasks.
    Background,
}

impl TaskPriority {
    const COUNT: usize = 3;

    const fn queue_index(self) -> usize {
        match self {
            Self::High => 0,
            Self::Normal => 1,
            Self::Background => 2,
        }
    }
}

/// Snapshot of task runtime queue and execution counters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskRuntimeStats {
    /// Number of worker threads owned by the runtime.
    pub worker_count: usize,
    /// Number of high-priority tasks waiting in the queue.
    pub queued_high: usize,
    /// Number of normal-priority tasks waiting in the queue.
    pub queued_normal: usize,
    /// Number of background-priority tasks waiting in the queue.
    pub queued_background: usize,
    /// Number of tasks currently running on worker threads.
    pub active: usize,
    /// Number of tasks submitted since runtime creation.
    pub submitted: u64,
    /// Number of tasks that returned a value successfully.
    pub completed: u64,
    /// Number of tasks whose body panicked.
    pub panicked: u64,
}

impl TaskRuntimeStats {
    /// Returns the total number of queued tasks.
    pub const fn queued_total(&self) -> usize {
        self.queued_high + self.queued_normal + self.queued_background
    }

    /// Returns the total number of finished tasks, including panics.
    pub const fn finished(&self) -> u64 {
        self.completed + self.panicked
    }
}

/// Configuration for an [`EngineTaskRuntime`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRuntimeConfig {
    /// Human-readable worker thread name prefix.
    pub thread_name: String,
    /// Number of worker threads to create.
    pub worker_count: usize,
}

impl TaskRuntimeConfig {
    /// Creates a task runtime configuration with the requested worker count.
    pub fn new(worker_count: usize) -> Self {
        Self {
            thread_name: "varg-task".to_owned(),
            worker_count: worker_count.max(1),
        }
    }
}

impl Default for TaskRuntimeConfig {
    fn default() -> Self {
        let worker_count = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .saturating_sub(1)
            .max(1);
        Self::new(worker_count)
    }
}

/// Error returned when waiting for a task result.
#[derive(Debug, thiserror::Error)]
pub enum TaskJoinError {
    /// The task runtime stopped before returning a value.
    #[error("task `{debug_name}` was canceled before completion")]
    Canceled {
        /// Debug name supplied when the task was spawned.
        debug_name: String,
    },

    /// The task body panicked.
    #[error("task `{debug_name}` panicked")]
    Panicked {
        /// Debug name supplied when the task was spawned.
        debug_name: String,
    },
}

enum TaskOutcome<T> {
    Completed(T),
    Panicked,
}

/// Handle to a spawned engine task.
pub struct TaskHandle<T> {
    debug_name: String,
    completed: Arc<AtomicBool>,
    result_rx: mpsc::Receiver<TaskOutcome<T>>,
}

impl<T> TaskHandle<T> {
    /// Returns the task debug name.
    pub fn debug_name(&self) -> &str {
        &self.debug_name
    }

    /// Returns true once the task has finished running.
    pub fn is_finished(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    /// Blocks until the task finishes and returns its result.
    pub fn wait(self) -> Result<T, TaskJoinError> {
        match self.result_rx.recv() {
            Ok(TaskOutcome::Completed(value)) => Ok(value),
            Ok(TaskOutcome::Panicked) => Err(TaskJoinError::Panicked {
                debug_name: self.debug_name,
            }),
            Err(_) => Err(TaskJoinError::Canceled {
                debug_name: self.debug_name,
            }),
        }
    }
}

impl<T> fmt::Debug for TaskHandle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskHandle")
            .field("debug_name", &self.debug_name)
            .field("is_finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}

struct TaskJob {
    priority: TaskPriority,
    run: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl TaskJob {
    fn run(mut self) {
        if let Some(run) = self.run.take() {
            run();
        }
    }
}

#[derive(Default)]
struct TaskQueues {
    queues: [VecDeque<TaskJob>; TaskPriority::COUNT],
    shutdown: bool,
}

impl TaskQueues {
    fn push(&mut self, job: TaskJob) {
        self.queues[job.priority.queue_index()].push_back(job);
    }

    fn pop(&mut self) -> Option<TaskJob> {
        self.queues.iter_mut().find_map(VecDeque::pop_front)
    }

    fn is_empty(&self) -> bool {
        self.queues.iter().all(VecDeque::is_empty)
    }

    fn len(&self, priority: TaskPriority) -> usize {
        self.queues[priority.queue_index()].len()
    }
}

#[derive(Default)]
struct TaskCounters {
    active: AtomicUsize,
    submitted: AtomicU64,
    completed: AtomicU64,
    panicked: AtomicU64,
}

struct SharedTaskState {
    queues: Mutex<TaskQueues>,
    wake_workers: Condvar,
    counters: Arc<TaskCounters>,
}

/// Small engine-owned CPU task runtime.
///
/// This runtime is intentionally separate from render, window, and editor main
/// threads. It is meant for short to medium CPU work such as preparation,
/// import, compilation, and background analysis jobs.
pub struct EngineTaskRuntime {
    state: Arc<SharedTaskState>,
    workers: Vec<JoinHandle<()>>,
}

impl EngineTaskRuntime {
    /// Creates a runtime with the default worker count.
    pub fn new() -> Self {
        Self::with_config(TaskRuntimeConfig::default())
    }

    /// Creates a runtime using the supplied configuration.
    pub fn with_config(config: TaskRuntimeConfig) -> Self {
        let state = Arc::new(SharedTaskState {
            queues: Mutex::new(TaskQueues::default()),
            wake_workers: Condvar::new(),
            counters: Arc::new(TaskCounters::default()),
        });
        let mut workers = Vec::with_capacity(config.worker_count);

        for index in 0..config.worker_count {
            let worker_state = Arc::clone(&state);
            let thread_name = format!("{}-{index}", config.thread_name);
            let worker = thread::Builder::new()
                .name(thread_name)
                .spawn(move || worker_loop(worker_state))
                .expect("spawn engine task worker");
            workers.push(worker);
        }

        Self { state, workers }
    }

    /// Returns the number of worker threads in this runtime.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Returns a point-in-time snapshot of queue and execution counters.
    pub fn stats(&self) -> TaskRuntimeStats {
        let queues = self
            .state
            .queues
            .lock()
            .expect("engine task queue mutex poisoned");
        TaskRuntimeStats {
            worker_count: self.worker_count(),
            queued_high: queues.len(TaskPriority::High),
            queued_normal: queues.len(TaskPriority::Normal),
            queued_background: queues.len(TaskPriority::Background),
            active: self.state.counters.active.load(Ordering::Acquire),
            submitted: self.state.counters.submitted.load(Ordering::Acquire),
            completed: self.state.counters.completed.load(Ordering::Acquire),
            panicked: self.state.counters.panicked.load(Ordering::Acquire),
        }
    }

    /// Spawns a task and returns a typed handle for waiting on the result.
    pub fn spawn<T, F>(
        &self,
        debug_name: impl Into<String>,
        priority: TaskPriority,
        task: F,
    ) -> TaskHandle<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let debug_name = debug_name.into();
        let handle_debug_name = debug_name.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let completed_for_task = Arc::clone(&completed);
        let counters = Arc::clone(&self.state.counters);
        counters.submitted.fetch_add(1, Ordering::AcqRel);

        let run = Box::new(move || {
            counters.active.fetch_add(1, Ordering::AcqRel);
            let outcome = match catch_unwind(AssertUnwindSafe(task)) {
                Ok(value) => {
                    counters.completed.fetch_add(1, Ordering::AcqRel);
                    TaskOutcome::Completed(value)
                }
                Err(_) => {
                    counters.panicked.fetch_add(1, Ordering::AcqRel);
                    TaskOutcome::Panicked
                }
            };
            completed_for_task.store(true, Ordering::Release);
            let _ = result_tx.send(outcome);
            counters.active.fetch_sub(1, Ordering::AcqRel);
        });

        let mut queues = self
            .state
            .queues
            .lock()
            .expect("engine task queue mutex poisoned");
        queues.push(TaskJob {
            priority,
            run: Some(run),
        });
        drop(queues);
        self.state.wake_workers.notify_one();

        TaskHandle {
            debug_name: handle_debug_name,
            completed,
            result_rx,
        }
    }
}

impl Default for EngineTaskRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EngineTaskRuntime {
    fn drop(&mut self) {
        let mut queues = self
            .state
            .queues
            .lock()
            .expect("engine task queue mutex poisoned");
        queues.shutdown = true;
        drop(queues);
        self.state.wake_workers.notify_all();

        while let Some(worker) = self.workers.pop() {
            let _ = worker.join();
        }
    }
}

/// Returns the process-wide shared engine task runtime.
///
/// This is suitable for subsystems that need a common background CPU pool and
/// do not own a runtime lifecycle themselves.
pub fn shared_task_runtime() -> &'static EngineTaskRuntime {
    static RUNTIME: OnceLock<EngineTaskRuntime> = OnceLock::new();
    RUNTIME.get_or_init(EngineTaskRuntime::new)
}

fn worker_loop(state: Arc<SharedTaskState>) {
    loop {
        let job = {
            let mut queues = state
                .queues
                .lock()
                .expect("engine task queue mutex poisoned");
            loop {
                if let Some(job) = queues.pop() {
                    break job;
                }
                if queues.shutdown && queues.is_empty() {
                    return;
                }
                queues = state
                    .wake_workers
                    .wait(queues)
                    .expect("engine task queue mutex poisoned");
            }
        };
        job.run();
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineTaskRuntime, TaskPriority, TaskRuntimeConfig};

    #[test]
    fn task_runtime_returns_spawned_result() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let handle = runtime.spawn("answer", TaskPriority::Normal, || 42);

        assert_eq!(handle.wait().unwrap(), 42);
    }

    #[test]
    fn task_runtime_runs_high_priority_before_background() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let (order_tx, order_rx) = std::sync::mpsc::channel();
        let first = runtime.spawn("gate", TaskPriority::Normal, move || {
            resume_rx.recv().unwrap();
        });

        let low_order_tx = order_tx.clone();
        let low = runtime.spawn("background", TaskPriority::Background, move || {
            low_order_tx.send("background").unwrap();
        });
        let high = runtime.spawn("high", TaskPriority::High, move || {
            order_tx.send("high").unwrap();
        });

        resume_tx.send(()).unwrap();
        first.wait().unwrap();
        low.wait().unwrap();
        high.wait().unwrap();

        assert_eq!(order_rx.recv().unwrap(), "high");
        assert_eq!(order_rx.recv().unwrap(), "background");
    }

    #[test]
    fn task_handle_reports_finished_state() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let handle = runtime.spawn("finished", TaskPriority::Normal, move || {
            resume_rx.recv().unwrap();
            "done"
        });

        assert!(!handle.is_finished());
        resume_tx.send(()).unwrap();
        assert_eq!(handle.wait().unwrap(), "done");
    }

    #[test]
    fn task_runtime_reports_panics() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let handle = runtime.spawn("panic", TaskPriority::Normal, || -> usize {
            panic!("intentional task panic");
        });

        assert!(handle.wait().is_err());
    }

    #[test]
    fn task_runtime_stats_track_queued_active_and_finished_tasks() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let gate = runtime.spawn("gate", TaskPriority::Normal, move || {
            started_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
        });
        started_rx.recv().unwrap();

        let background = runtime.spawn("background", TaskPriority::Background, || "background");
        let high = runtime.spawn("high", TaskPriority::High, || "high");

        let stats = runtime.stats();
        assert_eq!(stats.worker_count, 1);
        assert_eq!(stats.submitted, 3);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.queued_high, 1);
        assert_eq!(stats.queued_background, 1);
        assert_eq!(stats.queued_total(), 2);
        assert_eq!(stats.finished(), 0);

        resume_tx.send(()).unwrap();
        gate.wait().unwrap();
        assert_eq!(high.wait().unwrap(), "high");
        assert_eq!(background.wait().unwrap(), "background");

        let stats = runtime.stats();
        assert_eq!(stats.active, 0);
        assert_eq!(stats.queued_total(), 0);
        assert_eq!(stats.completed, 3);
        assert_eq!(stats.panicked, 0);
        assert_eq!(stats.finished(), 3);
    }

    #[test]
    fn task_runtime_stats_track_panicked_tasks() {
        let runtime = EngineTaskRuntime::with_config(TaskRuntimeConfig::new(1));
        let handle = runtime.spawn("panic", TaskPriority::Normal, || {
            panic!("intentional task panic");
        });

        assert!(handle.wait().is_err());

        let stats = runtime.stats();
        assert_eq!(stats.submitted, 1);
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.panicked, 1);
        assert_eq!(stats.finished(), 1);
    }
}
