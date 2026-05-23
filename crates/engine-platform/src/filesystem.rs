//! Filesystem abstraction.

use std::path::{Path, PathBuf};

use engine_core::{EngineError, EngineResult};

/// Filesystem operations required by the core runtime.
pub trait FileSystem {
    /// Reads a file into memory.
    fn read(&self, path: &Path) -> EngineResult<Vec<u8>>;

    /// Returns whether a path exists.
    fn exists(&self, path: &Path) -> bool;
}

/// Host filesystem implementation using `std`.
#[derive(Clone, Debug, Default)]
pub struct HostFileSystem;

impl FileSystem for HostFileSystem {
    fn read(&self, path: &Path) -> EngineResult<Vec<u8>> {
        std::fs::read(path).map_err(|source| EngineError::Filesystem {
            path: PathBuf::from(path),
            source,
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// A sandboxed filesystem that restricts all access to a configured root directory.
///
/// Paths are canonicalized and verified to stay within `root`. Symlinks are
/// resolved and checked against the same constraint.
#[derive(Clone, Debug)]
pub struct SandboxedFileSystem {
    root: PathBuf,
}

impl SandboxedFileSystem {
    /// Creates a new sandboxed filesystem rooted at `root`.
    ///
    /// Returns an error if `root` does not exist or cannot be canonicalized.
    pub fn new(root: impl Into<PathBuf>) -> EngineResult<Self> {
        let root_path: PathBuf = root.into();
        let root = root_path.canonicalize().map_err(|source| {
            EngineError::Filesystem {
                path: root_path.clone(),
                source,
            }
        })?;
        Ok(Self { root })
    }

    /// Validates that `path` is within the sandbox root.
    fn validate_path(&self, path: &Path) -> EngineResult<PathBuf> {
        let canonical = path.canonicalize().map_err(|source| EngineError::Filesystem {
            path: PathBuf::from(path),
            source,
        })?;
        if !canonical.starts_with(&self.root) {
            return Err(EngineError::Filesystem {
                path: PathBuf::from(path),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "path is outside the sandbox root",
                ),
            });
        }
        Ok(canonical)
    }
}

impl FileSystem for SandboxedFileSystem {
    fn read(&self, path: &Path) -> EngineResult<Vec<u8>> {
        let safe_path = self.validate_path(path)?;
        std::fs::read(&safe_path).map_err(|source| EngineError::Filesystem {
            path: safe_path,
            source,
        })
    }

    fn exists(&self, path: &Path) -> bool {
        self.validate_path(path)
            .map(|p| p.exists())
            .unwrap_or(false)
    }
}
