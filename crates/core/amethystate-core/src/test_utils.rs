use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unique_path(suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("amethystate-{suffix}-{pid}-{nanos}-{seq}.db"))
}

/// A [`unique_path`] that deletes itself, and anything a backend wrote beside
/// it, when it goes out of scope.
///
/// Declare it before the store so the store drops first: a backend holding the
/// file open would otherwise keep the removal from landing on Windows.
pub struct TempPath(PathBuf);

impl TempPath {
    pub fn new(suffix: &str) -> Self {
        Self(unique_path(suffix))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempPath {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let (Some(dir), Some(stem)) = (self.0.parent(), self.0.file_name()) else {
            return;
        };
        let stem = stem.to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&stem) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
