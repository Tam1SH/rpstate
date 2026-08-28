//! Trying to break the write path rather than exercise it.
//!
//! Killing the process is not among the attempts, and deliberately: the whole
//! document goes to a temporary file and only then replaces the target, so a
//! dead process cannot tear the file. What that arrangement does not cover is
//! power loss - the replacement can reach the disk while the contents are still
//! in the write-back cache - and no test reproduces that. It is a reading of
//! the write path, not something asserted here.
//!
//! Every claim is about a store rather than one engine, so each test runs
//! against whatever is compiled in. A test that fails is the finding and says
//! so in its `#[ignore]`, the way the `tamper_*` suite does.

use amethystate::amethystate;
use amethystate::store::builder::StoreBuilder;
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
use amethystate::store::config::{FileWritePolicy, WriteAttempts};
use amethystate_core::test_utils::TempPath;
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
use std::fs::OpenOptions;
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
use std::time::Instant;

#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
mod common;

#[amethystate(prefix = "atomic")]
pub struct Held {
    #[amestate(default = 1)]
    pub a: u32,

    #[amestate(default = 1)]
    pub b: u32,
}

/// A file the store cannot write must not take the process down, and must not
/// leave the caller thinking the value landed.
#[test]
fn a_path_that_cannot_be_written_is_reported() {
    let path = TempPath::new("atomic_unwritable");

    // The store's own path, occupied by a directory: every write to it fails at
    // the filesystem, which is the cheapest stand-in for a full disk or a
    // permission error.
    std::fs::create_dir_all(path.path()).unwrap();

    match StoreBuilder::new(path.path()).build() {
        Err(report) => {
            let rendered = format!("{report:?}");
            assert!(
                rendered.contains(&path.path().display().to_string()),
                "the report must name the file it could not use: {rendered}"
            );
        }
        Ok(store) => {
            let held = Held::new_with(&store).unwrap();
            held.a().set(7).unwrap();
            assert!(
                store.save_now().is_err(),
                "a flush that cannot write must say so rather than report success"
            );
        }
    }
}

/// A temporary file left behind by a process that died mid-write is litter in
/// the store's own directory. Opening again must not read it, trip over it, or
/// refuse to start.
#[test]
fn a_leftover_temp_file_does_not_disturb_the_next_open() {
    let path = TempPath::new("atomic_leftover");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let held = Held::new_with(&store).unwrap();
        held.a().set(42).unwrap();
        store.save_now().unwrap();
    }

    let dir = path.path().parent().unwrap();
    let stem = path
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    for litter in [
        format!("{stem}.tmp"),
        format!(".tmp{stem}"),
        format!("{stem}~"),
    ] {
        std::fs::write(dir.join(litter), b"this is not a store").unwrap();
    }

    let store = StoreBuilder::new(path.path())
        .build()
        .expect("litter beside the store must not stop it opening");
    let held = Held::new_with(&store).unwrap();
    assert_eq!(held.a().get(), 42, "the store's own file is still the store");
}

/// The text engines copy the data file to `.bak` on open, so a failed migration
/// can put it back. A process that dies *during* a migration therefore leaves a
/// good backup beside a half-migrated file - and the next open must not treat
/// that file as the thing worth backing up.
#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
#[test]
#[ignore = "known: `create_backups` on open copies the data file over the backup \
            before anything reads either, so a process killed mid-migration loses \
            the only good copy on the next start"]
fn a_backup_is_not_overwritten_by_the_file_it_exists_to_replace() {
    let path = TempPath::new("atomic_backup");

    {
        let store = StoreBuilder::new(path.path())
            .backend(common::text_backend())
            .build()
            .unwrap();
        let held = Held::new_with(&store).unwrap();
        held.a().set(99).unwrap();
        store.save_now().unwrap();
    }

    let good = std::fs::read_to_string(path.path()).unwrap();

    // What a process killed mid-migration leaves behind: the backup it took on
    // open, and a data file that never finished being rewritten.
    // `backup_of` appends `.bak` to the whole file name.
    let mut backup_name = path.path().file_name().unwrap().to_os_string();
    backup_name.push(".bak");
    let backup = path.path().with_file_name(backup_name);
    std::fs::write(&backup, &good).unwrap();
    std::fs::write(path.path(), "{ this never finished").unwrap();

    let _ = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .build();

    let backup_now = std::fs::read_to_string(&backup).unwrap_or_default();
    assert_eq!(
        backup_now, good,
        "the only good copy was overwritten by the broken file it was there to replace"
    );
}

/// An antivirus, an indexer or a cloud client holding the target file open is
/// the ordinary Windows failure, and it is a different class from a disk error:
/// the same call succeeds a moment later. Opening the target with no sharing at
/// all is exactly what those look like from here.
///
/// Only the text engines replace a file to write it; redb and sqlite hold their
/// own handle and write through it, so there is no replacement to block.
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
#[test]
fn a_file_held_by_someone_else_does_not_cost_the_old_contents() {
    use std::os::windows::fs::OpenOptionsExt;

    let path = TempPath::new("atomic_held");

    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .build()
        .unwrap();
    let held = Held::new_with(&store).unwrap();
    held.a().set(5).unwrap();
    store.save_now().unwrap();

    let before = std::fs::read(path.path()).unwrap();

    // FILE_SHARE_READ and nothing else: replacing the file needs the existing
    // handle to permit deletion, so this blocks the replacement while still
    // letting the test read what survived.
    const FILE_SHARE_READ: u32 = 1;
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path.path())
        .expect("the store's own file must be openable");

    held.a().set(6).unwrap();
    let flushed = store.save_now();

    assert!(
        flushed.is_err(),
        "a write that could not replace the file reported success"
    );
    assert_eq!(
        std::fs::read(path.path()).unwrap_or_default(),
        before,
        "a failed replacement must leave the previous contents where they were"
    );

    drop(blocker);

    store
        .save_now()
        .expect("once the other holder lets go, the same write must land");
    assert_ne!(std::fs::read(path.path()).unwrap(), before);
}

/// The reason the replacement is retried at all: the holder is transient, and
/// letting go a moment later is the ordinary case. So a single `save_now` has to
/// span the holder, not fail and leave the caller to try again - which is what
/// makes this different from the test above, where the file is free by the time
/// the second save starts.
///
/// The holder lets go half a budget in: late enough that the first attempt has
/// already failed, early enough that attempts are left. A budget of one passes
/// every other test in this file and fails this one, which is the whole point
/// of it.
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
#[test]
fn a_holder_that_lets_go_mid_write_does_not_cost_the_write() {
    use std::os::windows::fs::OpenOptionsExt;

    let path = TempPath::new("atomic_midwrite");
    let policy = FileWritePolicy::default();
    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .file_write(|_| policy)
        .build()
        .unwrap();
    let held = Held::new_with(&store).unwrap();
    held.a().set(5).unwrap();
    store.save_now().unwrap();

    const FILE_SHARE_READ: u32 = 1;
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path.path())
        .expect("the store's own file must be openable");

    let letting_go = std::thread::spawn(move || {
        std::thread::sleep(policy.replace.budget() / 2);
        drop(blocker);
    });

    held.a().set(6).unwrap();
    let flushed = store.save_now();
    letting_go.join().unwrap();

    flushed.expect("a holder that let go inside the retry budget still cost the write");

    let reopened = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .build()
        .unwrap();
    assert_eq!(
        Held::new_with(&reopened).unwrap().a().get(),
        6,
        "the write reported success without the value reaching the file"
    );
}

/// The other end of the same budget. A holder that never lets go must not hang
/// the caller, and the failure must name the file and carry what the OS said -
/// otherwise `is_err()` above would accept an error from anywhere.
///
/// The elapsed time is asserted because it is the only evidence the attempts
/// happened at all: a path that gives up immediately reports the same error.
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
#[test]
fn a_holder_that_never_lets_go_is_given_up_on() {
    use std::os::windows::fs::OpenOptionsExt;

    let path = TempPath::new("atomic_forever");
    let policy = FileWritePolicy::default();
    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .file_write(|_| policy)
        .build()
        .unwrap();
    let held = Held::new_with(&store).unwrap();
    held.a().set(5).unwrap();
    store.save_now().unwrap();

    const FILE_SHARE_READ: u32 = 1;
    let _blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path.path())
        .expect("the store's own file must be openable");

    held.a().set(6).unwrap();
    let started = Instant::now();
    let report = store.save_now().unwrap_err();
    let elapsed = started.elapsed();

    let budget = policy.replace.budget();
    assert!(
        elapsed >= budget,
        "the replacement was given up on after {elapsed:?}, which is less than the \
         configured {budget:?} - a holder letting go a moment later would have cost \
         the write"
    );
    assert!(
        elapsed < budget * 8,
        "a write nobody can complete held the caller for {elapsed:?}"
    );

    let rendered = format!("{report:?}");
    assert!(
        rendered.contains(&path.path().display().to_string()),
        "the failure must name the file it could not replace: {rendered}"
    );
    assert!(
        rendered.contains("os error 5"),
        "the failure must carry what the OS said rather than a summary: {rendered}"
    );
}

/// The two tests above read the default policy, so a store that ignored the
/// configuration entirely would still pass them. This one asks for a budget
/// nobody would arrive at by accident: no retry at all, which turns the same
/// blocked write into an immediate failure.
#[cfg(all(windows, any(feature = "json", feature = "toml", feature = "ron")))]
#[test]
fn a_policy_that_says_not_to_retry_is_obeyed() {
    use std::os::windows::fs::OpenOptionsExt;

    let path = TempPath::new("atomic_norety");
    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .file_write(|w| w.replacing(WriteAttempts::once()))
        .build()
        .unwrap();
    let held = Held::new_with(&store).unwrap();
    held.a().set(5).unwrap();
    store.save_now().unwrap();

    const FILE_SHARE_READ: u32 = 1;
    let _blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path.path())
        .expect("the store's own file must be openable");

    held.a().set(6).unwrap();
    let started = Instant::now();
    assert!(store.save_now().is_err());
    let elapsed = started.elapsed();

    let default_budget = FileWritePolicy::default().replace.budget();
    assert!(
        elapsed < default_budget / 2,
        "asking for no retry still took {elapsed:?}, near the default {default_budget:?} - \
         the configured policy never reached the write path"
    );
}

/// Content that parses as far as it goes and then turns to rubbish. A store
/// that reads the good prefix and stops has accepted a file nobody wrote.
#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
#[test]
fn valid_content_followed_by_rubbish_is_refused() {
    let path = TempPath::new("atomic_rubbish");

    {
        let store = StoreBuilder::new(path.path())
            .backend(common::text_backend())
            .build()
            .unwrap();
        let held = Held::new_with(&store).unwrap();
        held.a().set(5).unwrap();
        store.save_now().unwrap();
    }

    let good = std::fs::read_to_string(path.path()).unwrap();
    std::fs::write(
        path.path(),
        format!("{good}\n\u{0}\u{0}garbage not in any grammar"),
    )
    .unwrap();

    assert!(
        StoreBuilder::new(path.path())
            .backend(common::text_backend())
            .build()
            .is_err(),
        "a file whose tail is rubbish parsed as if the rubbish were not there"
    );
}
