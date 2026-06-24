use std::{cell::UnsafeCell, sync::Mutex, thread::ThreadId};

use crate::EditorHost;

/// Thread-safe wrapper for `EditorHost`.
///
/// `EditorHost` is accessed from synchronous Tauri commands and may hold
/// platform-bound runtime state. This wrapper uses
/// `UnsafeCell` + `Mutex<()>` to provide exclusive access while
/// recording the creating thread ID at construction.
///
/// # Safety
///
/// Tauri synchronous `#[tauri::command]` functions always execute on
/// the main thread, ensuring thread affinity. `with_host()` verifies at
/// runtime that the caller is the creating thread. An `unsafe impl Send`
/// + `Sync` is required because `State<'_, T>` needs `T: Send + Sync`,
/// but access is checked on every invocation.
pub struct EditorHostState {
    host: UnsafeCell<EditorHost>,
    lock: Mutex<()>,
    thread_id: ThreadId,
}

// SAFETY: `with_host()` asserts the calling thread matches `thread_id`
// at runtime. Mutex provides exclusive access. Tauri sync commands run
// on the main thread, upholding the thread-affinity invariant.
unsafe impl Send for EditorHostState {}
unsafe impl Sync for EditorHostState {}

impl EditorHostState {
    pub fn new(host: EditorHost) -> Self {
        Self {
            host: UnsafeCell::new(host),
            lock: Mutex::new(()),
            thread_id: std::thread::current().id(),
        }
    }

    /// Access the inner `EditorHost` under lock.
    ///
    /// # Panics
    ///
    /// Panics if called from a thread other than the one that created
    /// this instance (catches cross-thread `!Send` access in debug
    /// builds - release builds still check).
    pub fn with_host<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EditorHost) -> R,
    {
        let current_id = std::thread::current().id();
        assert_eq!(
            current_id, self.thread_id,
            "EditorHostState accessed from thread {:?} but was created on {:?}",
            current_id, self.thread_id
        );
        let _guard = self.lock.lock().expect("poisoned lock");
        // SAFETY: Thread-affinity assertion + mutex guarantee exclusive
        // mutable access from the correct thread.
        f(unsafe { &mut *self.host.get() })
    }
}
