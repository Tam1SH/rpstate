//! A value the application defined, written by a `Serialize` the application
//! wrote, into a file the store has to be able to read back.
//!
//! Everything else that can break a text file arrives from outside: a person
//! editing it, a process dying, a holder blocking a replacement. This one
//! arrives through the front door, in a type the store was asked to persist,
//! and the store is the one that writes it.

#![cfg(any(feature = "json", feature = "toml", feature = "ron"))]

use amethystate::store::builder::StoreBuilder;
use amethystate::store::field_with_path;
use amethystate::uuid::Uuid;
use amethystate_core::test_utils::TempPath;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};

mod common;
use common::text_backend;

fn meta_path(path: &Path) -> PathBuf {
    path.with_extension("meta")
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// A type whose `Serialize` refuses some of its own values. Not a contrived
/// failure: a serializer that validates, one that meets a map key its format
/// cannot spell, or one that runs out of something all end here.
///
/// It refuses selectively so that declaring the path works and only a later
/// write fails. A type that refused everything would be turned away when its
/// default was written, and the interesting half - a store that has been
/// running and is asked to hold something it cannot - would never be reached.
#[derive(Clone, Debug, Default, PartialEq)]
struct Fussy(u32);

impl Serialize for Fussy {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            0 => s.serialize_u32(0),
            other => Err(serde::ser::Error::custom(format!(
                "this value will not be written today: {other}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Fussy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        u32::deserialize(d).map(Fussy)
    }
}

/// A value that cannot be rendered must be refused where it is written, and
/// the file must be exactly as it was.
///
/// The alternative - noticing at the flush - means the store carries a change
/// it can never persist, and every later flush fails for a reason the caller
/// has long since walked away from.
#[test]
fn a_value_that_cannot_be_written_does_not_reach_the_file() {
    let path = TempPath::new("ser_refused");

    let store = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .unwrap();

    let good = field_with_path::<u32>(&store, ["ser", "good"], 1, Uuid::new_v4()).unwrap();
    good.set(7).unwrap();
    store.save_now().unwrap();

    let fussy =
        field_with_path::<Fussy>(&store, ["ser", "fussy"], Fussy(0), Uuid::new_v4()).unwrap();
    store.save_now().unwrap();

    let before = std::fs::read(path.path()).unwrap();
    let meta_before = std::fs::read(meta_path(path.path())).unwrap();

    let report = fussy
        .set(Fussy(1))
        .expect_err("a value whose serializer refuses was accepted as a write");
    let flushed = store.save_now();

    let rendered = format!("{report:?}");
    assert!(
        rendered.contains("ser.fussy"),
        "the refusal must name the path it was written to: {rendered}"
    );
    assert!(
        rendered.contains("will not be written today"),
        "the refusal must carry what the serializer said: {rendered}"
    );
    assert!(
        flushed.is_ok(),
        "a value the store never accepted made the next flush fail: {:?}",
        flushed.err()
    );

    assert_eq!(
        std::fs::read(path.path()).unwrap(),
        before,
        "the file changed for a value that could not be written"
    );
    assert_eq!(
        std::fs::read(meta_path(path.path())).unwrap(),
        meta_before,
        "the schema bookkeeping changed for a value that could not be written"
    );
}

/// The file must still be a file after the refusal - which is a different
/// question from whether its bytes moved, because a store that keeps a broken
/// document in memory writes it out on the next flush.
#[test]
fn a_refused_value_leaves_a_store_that_still_opens_and_still_writes() {
    let path = TempPath::new("ser_refused_reopen");

    {
        let store = StoreBuilder::new(path.path())
            .backend(text_backend())
            .build()
            .unwrap();
        let good = field_with_path::<u32>(&store, ["ser", "good"], 1, Uuid::new_v4()).unwrap();
        good.set(7).unwrap();
        store.save_now().unwrap();

        let fussy =
            field_with_path::<Fussy>(&store, ["ser", "fussy"], Fussy(0), Uuid::new_v4()).unwrap();
        let _ = fussy.set(Fussy(1));
        let _ = store.save_now();

        good.set(8).unwrap();
        store
            .save_now()
            .expect("a refusal must not poison every later flush");
    }

    let reopened = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .expect("a store that refused a value must still open");
    let good = field_with_path::<u32>(&reopened, ["ser", "good"], 1, Uuid::new_v4()).unwrap();
    assert_eq!(good.get(), 8, "the write after the refusal was lost");
}

/// A `Serialize` that succeeds and still ruins the file.
///
/// `serde_json` bounds how deeply it will read, and does not bound how deeply
/// it will write - so a type that nests further than the reader allows is
/// written without complaint and cannot be read back. Nothing about it is
/// malformed; it is a document the writer will produce and the reader will
/// not accept.
#[cfg(feature = "json")]
#[derive(Clone, Debug, Default, PartialEq)]
struct Deep(u32);

/// Past `serde_json`'s reading limit, which is 128.
#[cfg(feature = "json")]
const TOO_DEEP: u32 = 200;

#[cfg(feature = "json")]
impl Serialize for Deep {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        if self.0 == 0 {
            return s.serialize_u32(0);
        }
        let mut seq = s.serialize_seq(Some(1))?;
        seq.serialize_element(&Deep(self.0 - 1))?;
        seq.end()
    }
}

#[cfg(feature = "json")]
impl<'de> Deserialize<'de> for Deep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        u32::deserialize(d).map(Deep)
    }
}

/// The store writes it, and then cannot open what it wrote.
///
/// The place to catch this is the write, not a copy taken before it: at the
/// moment the value arrives the file is still good, and what the store lacks
/// is not a spare but a reason to say no. The test below records how far the
/// backup was ever meant to reach.
#[cfg(feature = "json")]
#[test]
#[ignore = "known: a value that writes deeper than the reader accepts is taken \
            without complaint, and the file it lands in cannot be opened again"]
fn a_value_the_writer_accepts_can_always_be_read_back() {
    let path = TempPath::new("ser_too_deep");

    {
        let store = StoreBuilder::new(path.path())
            .backend(text_backend())
            .build()
            .unwrap();
        let deep =
            field_with_path::<Deep>(&store, ["ser", "deep"], Deep(0), Uuid::new_v4()).unwrap();

        deep.set(Deep(TOO_DEEP))
            .expect("the writer took a value it will not read back");
        store.save_now().unwrap();
    }

    StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .expect("the store cannot open the file it wrote itself");
}

/// Writes `value` at a path `segments` levels down, and says whether the store
/// can open the file afterwards.
#[cfg(feature = "json")]
fn survives_a_reopen(label: &str, segments: usize, value: u32) -> bool {
    let path = TempPath::new(label);
    let mut names: Vec<String> = (0..segments).map(|s| format!("s{s}")).collect();
    names.push("leaf".to_string());

    {
        let store = StoreBuilder::new(path.path())
            .backend(text_backend())
            .build()
            .unwrap();
        let field = field_with_path::<Deep>(
            &store,
            names.iter().map(String::as_str).collect::<Vec<_>>(),
            Deep(0),
            Uuid::new_v4(),
        )
        .unwrap();
        field.set(Deep(value)).unwrap();
        store.save_now().unwrap();
    }

    StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .is_ok()
}

/// The reader's budget is spent by the whole document, so where a value is put
/// decides whether it can be read back.
///
/// The same `Deep(120)` survives at a two-level path and does not at a
/// ten-level one. Nothing about the value changed; the levels the store nests
/// it under to spell its path came out of the same allowance.
///
/// This is the reason a check cannot look at the value alone. Whatever refuses
/// the write has to weigh the path with it, which the store knows at `set` -
/// so the check is affordable, but only if it is asked the right question.
#[cfg(feature = "json")]
#[test]
#[ignore = "known: a value the store accepts at one path makes the file \
            unreadable at a deeper one, and nothing weighs the two together"]
fn where_a_value_is_written_does_not_decide_whether_it_can_be_read() {
    assert!(
        survives_a_reopen("depth_shallow", 2, 120),
        "the shallow case has stopped working, so the contrast below says nothing \
         and the numbers here need choosing again"
    );
    assert!(
        survives_a_reopen("depth_deeper", 10, 120),
        "the same value the store reads back at a two-level path cannot be read at \
         a ten-level one"
    );
}

/// How far the backup reaches, pinned so that the answer is stated somewhere
/// rather than inferred from where `clean_backups` is called.
///
/// It covers the open: a migration transforms data the store did not write,
/// and a failure part of the way through leaves a document that is neither the
/// old shape nor the new one. That is rare, bounded, and worth a copy of the
/// whole file. Ordinary writes are none of those things - a copy before each
/// replacement would double the I/O of every flush - and the atomic
/// replacement already means a write cannot cost the previous file.
///
/// So a value that serialises and does not read back is outside what any
/// backup was going to catch. Refusing that write is the answer to it; a copy
/// taken before it is not, since the copy would be of a file that was still
/// good and the damage arrives with the write itself.
#[test]
fn the_backup_covers_the_open_and_ends_with_it() {
    let path = TempPath::new("ser_backup_scope");

    let store = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .unwrap();
    let good = field_with_path::<u32>(&store, ["ser", "good"], 1, Uuid::new_v4()).unwrap();
    good.set(7).unwrap();
    store.save_now().unwrap();

    assert!(
        !backup_path(path.path()).exists(),
        "a backup outlived the open that took it - which is a change of policy, not \
         a bug, and this test is where it should be written down"
    );
    assert!(
        !backup_path(&meta_path(path.path())).exists(),
        "a metadata backup outlived the open that took it"
    );
}
