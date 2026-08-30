//! Atomicity under a load that is trying to catch it out.
//!
//! [`atomic_write`] asks whether the write path does the right thing when one
//! named condition holds. This asks the same question with the conditions
//! arriving in an order nobody chose: a reader looking at the file while it is
//! being replaced, several writers going at once, a holder appearing and
//! letting go at moments picked by a generator.
//!
//! Every run is reproducible. The seed is fixed unless `AME_STRESS_SEED` says
//! otherwise, and every failure prints the one it ran with, so a schedule that
//! breaks something can be gone back to.
//!
//! [`atomic_write`]: ../atomic_write/index.html

#![cfg(any(feature = "json", feature = "toml", feature = "ron"))]

use amethystate::store::builder::StoreBuilder;
use amethystate::store::field_with_path;
#[cfg(feature = "json")]
use amethystate::store::{StoreBackend, StoreLayout};
use amethystate::uuid::Uuid;
use amethystate_core::test_utils::TempPath;
#[cfg(feature = "json")]
use std::sync::Arc;
#[cfg(feature = "json")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
#[cfg(feature = "json")]
use std::time::Instant;

mod common;

/// A schedule, rather than a source of randomness: the point is that the same
/// seed replays the same interleaving, not that the numbers are good ones.
struct Schedule(u64);

impl Schedule {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }

    /// A pause short enough that the loop around it still runs many times.
    fn brief_pause(&mut self, up_to_micros: u64) {
        let micros = self.below(up_to_micros);
        if micros > 0 {
            std::thread::sleep(Duration::from_micros(micros));
        }
    }
}

fn seed() -> u64 {
    std::env::var("AME_STRESS_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_1234)
}

/// How long a stress test is allowed to run before it stops looking.
#[cfg(feature = "json")]
const DEADLINE: Duration = Duration::from_secs(5);

/// A write in flight is a narrow window, so a run that never caught one has
/// not shown anything and must not report success.
#[cfg(feature = "json")]
fn assert_saw_enough(observed: usize, floor: usize, what: &str, seed: u64) {
    assert!(
        observed >= floor,
        "only {observed} {what} in {DEADLINE:?}, which is too few to have exercised \
         anything - the test would pass whether the write path were atomic or not \
         (seed {seed:#x})"
    );
}

/// A reader that is not the store, looking at the store's file at moments the
/// store did not choose. Every look must find a whole document.
///
/// The parse is what makes this a test rather than an exercise: a replacement
/// that let a reader see part of a write leaves a file that no parser accepts.
/// Only `json` is checked this way, because the test can parse that itself
/// without asking the crate under test whether its own file is well formed.
#[cfg(feature = "json")]
#[test]
fn a_reader_never_meets_a_half_written_file() {
    let seed = seed();
    let path = TempPath::new("stress_torn");

    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .debounce(Duration::from_millis(1))
        .build()
        .unwrap();
    let n = field_with_path::<u64>(&store, ["stress", "n"], 0, Uuid::new_v4()).unwrap();
    store.save_now().unwrap();

    let running = Arc::new(AtomicBool::new(true));
    let highest = Arc::new(AtomicU64::new(0));

    let writer = {
        let running = running.clone();
        let highest = highest.clone();
        let mut schedule = Schedule::new(seed);
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut written = 0u64;
            while started.elapsed() < DEADLINE {
                written += 1;
                n.set(written).unwrap();
                highest.store(written, Ordering::SeqCst);
                let _ = store.save_now();
                schedule.brief_pause(300);
            }
            running.store(false, Ordering::SeqCst);
            written
        })
    };

    let readers: Vec<_> = (0..3)
        .map(|reader| {
            let running = running.clone();
            let highest = highest.clone();
            let file = path.path().to_path_buf();
            let mut schedule = Schedule::new(seed ^ (reader + 1) * 0x9E37_79B9);
            std::thread::spawn(move || {
                let mut whole = 0usize;
                let mut unreadable = 0usize;
                while running.load(Ordering::SeqCst) {
                    let ceiling = highest.load(Ordering::SeqCst);
                    let Ok(bytes) = std::fs::read(&file) else {
                        unreadable += 1;
                        schedule.brief_pause(200);
                        continue;
                    };

                    let parsed: serde_json::Value =
                        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                            panic!(
                                "reader {reader} met a file no parser accepts, which is the \
                                 whole thing the temporary file exists to prevent: {e}\n\
                                 (seed {seed:#x}, {} bytes)\n{}",
                                bytes.len(),
                                String::from_utf8_lossy(&bytes)
                            )
                        });

                    if let Some(seen) = parsed.get("stress").and_then(|s| s.get("n")) {
                        let seen = seen.as_u64().unwrap_or_else(|| {
                            panic!("`stress.n` came back as {seen}, not a number (seed {seed:#x})")
                        });
                        assert!(
                            seen <= ceiling,
                            "the file held {seen} when at most {ceiling} had been written - \
                             a read spliced together two states (seed {seed:#x})"
                        );
                    }

                    whole += 1;
                    schedule.brief_pause(200);
                }
                (whole, unreadable)
            })
        })
        .collect();

    let written = writer.join().unwrap();
    let looks: usize = readers
        .into_iter()
        .map(|r| {
            let (whole, _unreadable) = r.join().unwrap();
            whole
        })
        .sum();

    assert_saw_enough(written as usize, 20, "writes", seed);
    assert_saw_enough(looks, 20, "whole documents read", seed);
}

/// Several writers on one store, each owning its own path, all flushing to the
/// same file whenever they feel like it.
///
/// What a race costs here is not a parse error but a lost write: the file ends
/// up holding neither what one writer put there nor what the other did. So the
/// store is reopened at the end and asked for every path, having been told each
/// one's last value - after every writer has finished and a final `save_now`
/// has returned success, so there is nothing left in flight to excuse it.
///
/// It used to fail about twice in ten runs, and what came back was a value from
/// earlier in the run rather than the default - the shape of a stale document
/// landing on a newer one rather than of a write never made. `StoreFile::persist`
/// rendered the document under a guard it then dropped, and replaced the file
/// holding nothing, so two flushes crossed.
#[test]
fn writers_racing_each_other_all_land() {
    let seed = seed();
    let path = TempPath::new("stress_racers");

    const WRITERS: u64 = 4;
    const EACH: u64 = 60;

    {
        let store = StoreBuilder::new(path.path())
            .backend(common::text_backend())
            .debounce(Duration::from_millis(1))
            .build()
            .unwrap();

        let racers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let store = store.clone();
                let mut schedule = Schedule::new(seed ^ (writer + 1) * 0x1000_0001);
                std::thread::spawn(move || {
                    let field = field_with_path::<u64>(
                        &store,
                        ["racers", &format!("w{writer}")],
                        0,
                        Uuid::new_v4(),
                    )
                    .unwrap();
                    for round in 1..=EACH {
                        field.set(round).unwrap();
                        if schedule.below(3) == 0 {
                            let _ = store.save_now();
                        }
                        schedule.brief_pause(150);
                    }
                })
            })
            .collect();

        for racer in racers {
            racer.join().unwrap();
        }
        store.save_now().unwrap();
    }

    let reopened = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .build()
        .expect("a file several writers flushed at once must still open");

    for writer in 0..WRITERS {
        let field = field_with_path::<u64>(
            &reopened,
            ["racers", &format!("w{writer}")],
            0,
            Uuid::new_v4(),
        )
        .unwrap();
        assert_eq!(
            field.get(),
            EACH,
            "writer {writer} finished at {EACH} and the file came back holding \
             something else - a flush landed on top of another (seed {seed:#x})"
        );
    }
}

/// A holder arriving and letting go at moments the store did not choose, some
/// of them longer than the retry budget and some shorter.
///
/// Whether any one save succeeds is the schedule's business. What is not the
/// schedule's business is the file: a save that failed must leave the previous
/// document where it was, whole, and the store must still be able to land the
/// value once the holder is gone for good.
#[cfg(all(windows, feature = "json"))]
#[test]
fn a_holder_coming_and_going_never_leaves_a_broken_file() {
    use std::os::windows::fs::OpenOptionsExt;

    let seed = seed();
    let path = TempPath::new("stress_holder");

    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .debounce(Duration::from_millis(1))
        .build()
        .unwrap();
    let n = field_with_path::<u64>(&store, ["stress", "n"], 0, Uuid::new_v4()).unwrap();
    store.save_now().unwrap();

    let running = Arc::new(AtomicBool::new(true));

    let chaos = {
        let running = running.clone();
        let file = path.path().to_path_buf();
        let mut schedule = Schedule::new(seed ^ 0xC0FF_EE00);
        std::thread::spawn(move || {
            const FILE_SHARE_READ: u32 = 1;
            let mut held = 0usize;
            while running.load(Ordering::SeqCst) {
                if let Ok(blocker) = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ)
                    .open(&file)
                {
                    held += 1;
                    std::thread::sleep(Duration::from_millis(schedule.below(700)));
                    drop(blocker);
                }
                std::thread::sleep(Duration::from_millis(schedule.below(120)));
            }
            held
        })
    };

    let mut schedule = Schedule::new(seed);
    let started = Instant::now();
    let mut written = 0u64;
    let mut refused = 0usize;
    while started.elapsed() < DEADLINE {
        written += 1;
        n.set(written).unwrap();
        if store.save_now().is_err() {
            refused += 1;
        }

        if let Ok(bytes) = std::fs::read(path.path()) {
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|e| {
                panic!(
                    "a save that met a holder left a file no parser accepts: {e}\n\
                     (seed {seed:#x}, {} bytes)\n{}",
                    bytes.len(),
                    String::from_utf8_lossy(&bytes)
                )
            });
        }
        schedule.brief_pause(400);
    }

    running.store(false, Ordering::SeqCst);
    let held = chaos.join().unwrap();

    assert_saw_enough(held, 5, "times the file was held", seed);
    assert!(
        refused > 0,
        "no save ever met the holder in {DEADLINE:?}, so nothing about failing under \
         one was shown (seed {seed:#x})"
    );

    store
        .save_now()
        .expect("with the holder gone for good, the buffered value must land");
    assert_eq!(
        n.get(),
        written,
        "the store lost the value it was holding while writes were failing (seed {seed:#x})"
    );

    let reopened = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .build()
        .expect("a file written through a coming and going holder must open");
    let recovered = field_with_path::<u64>(&reopened, ["stress", "n"], 0, Uuid::new_v4()).unwrap();
    assert_eq!(
        recovered.get(),
        written,
        "the last value reported as saved is not the one in the file (seed {seed:#x})"
    );
}

/// The metadata file is written the same way as the data file and is just as
/// able to be caught half-written - and a store whose schema bookkeeping is
/// torn cannot read the data file at all, so it is the worse of the two.
#[cfg(feature = "json")]
#[test]
fn the_metadata_file_is_never_half_written_either() {
    let seed = seed();
    let path = TempPath::new("stress_meta");

    let store = StoreBuilder::new(path.path())
        .backend(common::text_backend())
        .debounce(Duration::from_millis(1))
        .build()
        .unwrap();

    let Some(StoreLayout::Sidecars { meta, .. }) = StoreBackend::files(&store) else {
        panic!("a text store keeps its bookkeeping in a file of its own");
    };

    let running = Arc::new(AtomicBool::new(true));
    let reader = {
        let running = running.clone();
        let meta = meta.clone();
        let mut schedule = Schedule::new(seed ^ 0x0BAD_0BAD);
        std::thread::spawn(move || {
            let mut whole = 0usize;
            while running.load(Ordering::SeqCst) {
                if let Ok(bytes) = std::fs::read(&meta) {
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|e| {
                        panic!(
                            "the schema bookkeeping was caught half-written, which leaves a \
                             store unable to read its own data: {e}\n(seed {seed:#x}, {} bytes)\n{}",
                            bytes.len(),
                            String::from_utf8_lossy(&bytes)
                        )
                    });
                    whole += 1;
                }
                schedule.brief_pause(200);
            }
            whole
        })
    };

    let mut schedule = Schedule::new(seed);
    let started = Instant::now();
    let mut declared = 0u64;
    while started.elapsed() < DEADLINE / 2 {
        declared += 1;
        let field = field_with_path::<u64>(
            &store,
            ["meta_churn", &format!("f{declared}")],
            declared,
            Uuid::new_v4(),
        )
        .unwrap();
        field.set(declared).unwrap();
        let _ = store.save_now();
        schedule.brief_pause(300);
    }

    running.store(false, Ordering::SeqCst);
    let whole = reader.join().unwrap();

    assert_saw_enough(declared as usize, 10, "paths declared", seed);
    assert_saw_enough(whole, 10, "whole metadata documents read", seed);
}
