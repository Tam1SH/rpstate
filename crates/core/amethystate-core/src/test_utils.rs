use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unique_path(suffix: &str) -> PathBuf {
    // The clock alone is not enough: Windows resolves `SystemTime::now` far
    // more coarsely than tests start, so parallel tests collided on one path
    // and the second one met `DatabaseAlreadyOpen`.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("amethystate-{suffix}-{pid}-{nanos}-{seq}.db"))
}
