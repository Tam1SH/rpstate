//! What the sqlite engine takes on a write and does not hand back on a read.
//!
//! The engine is a binary store wearing a text codec: keys are sqlite `TEXT`,
//! values are JSON rendered by `sonic_rs` into a `BLOB`. So it can lose a
//! value the way JSON loses it, and lose a path the way a string comparison
//! loses it, and nothing in the engine's name says either.
//!
//! Every probe here writes through a store, flushes, drops it, opens the file
//! again and reads. What a probe asserts is the round trip; a failure is the
//! finding.

#![cfg(feature = "sqlite")]

use amethystate::Store;
use amethystate::store::builder::{Backend, StoreBuilder};
use amethystate::store::{StorageResult, StoreBackend};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::time::Duration;

/// A sqlite store with the debouncer pushed out, so nothing lands except at a
/// `save_now`.
fn open(path: &std::path::Path) -> StorageResult<Store> {
    StoreBuilder::new(path)
        .backend(Backend::Sqlite)
        .debounce(Duration::from_secs(60))
        .watch_interval(Duration::from_secs(60))
        .build()
}

fn opened(path: &std::path::Path) -> Store {
    open(path).expect("the store should open")
}

/// Writes `value` at `segments`, flushes, reopens the file and reads it back.
fn roundtrip<T>(label: &str, segments: &[&str], value: &T) -> Result<Option<T>, String>
where
    T: Serialize + DeserializeOwned,
{
    let file = TempPath::new(label);
    {
        let store = open(file.path()).map_err(|e| format!("open: {e:?}"))?;
        let path = StorePath::try_from_segments(segments).map_err(|e| format!("path: {e:?}"))?;
        store
            .set_owned(path, value)
            .map_err(|e| format!("write: {e:?}"))?;
        store.save_now().map_err(|e| format!("flush: {e:?}"))?;
    }
    let store = open(file.path()).map_err(|e| format!("reopen: {e:?}"))?;
    let path = StorePath::try_from_segments(segments).map_err(|e| format!("path: {e:?}"))?;
    store.get::<T>(&path).map_err(|e| format!("read: {e:?}"))
}

/// The same at a fixed, boring path, for probes about the value alone.
fn value_roundtrip<T>(label: &str, value: &T) -> Result<Option<T>, String>
where
    T: Serialize + DeserializeOwned,
{
    roundtrip(label, &["probe", "v"], value)
}

fn assert_kept<T>(label: &str, value: T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    match value_roundtrip(label, &value) {
        Ok(Some(back)) => assert_eq!(back, value, "{label}: read back a different value"),
        Ok(None) => panic!("{label}: silent alteration - the path holds nothing after the write"),
        Err(e) => panic!("{label}: {e}"),
    }
}

// ---------------------------------------------------------------------------
// 1. The json defects, asked of sqlite
// ---------------------------------------------------------------------------

#[test]
fn nan_reads_back() {
    let file = TempPath::new("sq_nan");
    {
        let store = opened(file.path());
        store
            .set(["probe", "v"], &f64::NAN)
            .expect("the write is accepted");
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    let raw = StoreBackend::get_raw(&store, &StorePath::from_segments(["probe", "v"])).unwrap();
    println!(
        "nan raw bytes: {:?}",
        raw.as_deref().map(String::from_utf8_lossy)
    );
    let read = store.get::<f64>(["probe", "v"]);
    let kept = matches!(&read, Ok(Some(v)) if v.is_nan());
    assert!(kept, "NaN did not survive: {read:?}");
}

#[test]
fn infinity_reads_back() {
    let file = TempPath::new("sq_inf");
    {
        let store = opened(file.path());
        store.set(["probe", "v"], &f64::INFINITY).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    let raw = StoreBackend::get_raw(&store, &StorePath::from_segments(["probe", "v"])).unwrap();
    println!(
        "infinity raw bytes: {:?}",
        raw.as_deref().map(String::from_utf8_lossy)
    );
    assert_eq!(store.get::<f64>(["probe", "v"]).unwrap(), Some(f64::INFINITY));
}

#[test]
fn neg_infinity_reads_back() {
    assert_kept("sq_neg_inf", f64::NEG_INFINITY);
}

/// A value nested `n` sequences deep, which reads back as the depth it was
/// written at - so a round trip says whether the nesting survived, and not
/// merely whether something did.
#[derive(Clone, Debug, Default, PartialEq)]
struct Nest(u32);

impl Serialize for Nest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        if self.0 == 0 {
            return s.serialize_u32(0);
        }
        let mut seq = s.serialize_seq(Some(1))?;
        seq.serialize_element(&Nest(self.0 - 1))?;
        seq.end()
    }
}

struct NestVisitor;

impl<'de> serde::de::Visitor<'de> for NestVisitor {
    type Value = Nest;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a number, or a sequence of one of these")
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Nest, E> {
        Ok(Nest(0))
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Nest, E> {
        Ok(Nest(0))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Nest, A::Error> {
        let inner: Nest = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::custom("a sequence with nothing in it"))?;
        Ok(Nest(inner.0 + 1))
    }
}

impl<'de> Deserialize<'de> for Nest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(NestVisitor)
    }
}

/// What a `Nest(value)` written at a path `segments` levels down comes back as.
fn nest_roundtrip(label: &str, segments: usize, value: u32) -> Result<u32, String> {
    let file = TempPath::new(label);
    let mut names: Vec<String> = (0..segments).map(|s| format!("s{s}")).collect();
    names.push("leaf".to_string());
    let path = StorePath::try_from_segments(&names).map_err(|e| format!("path: {e:?}"))?;

    {
        let store = open(file.path()).map_err(|e| format!("open: {e:?}"))?;
        store
            .set_owned(path.clone(), &Nest(value))
            .map_err(|e| format!("write: {e:?}"))?;
        store.save_now().map_err(|e| format!("flush: {e:?}"))?;
    }

    let store = open(file.path()).map_err(|e| format!("reopen: {e:?}"))?;
    match store.get::<Nest>(&path) {
        Ok(Some(n)) => Ok(n.0),
        Ok(None) => Err("read: the path holds nothing".to_string()),
        Err(e) => Err(format!("read: {}", first_line(&format!("{e:?}")))),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or_default().to_string()
}

fn nest_survives(label: &str, segments: usize, value: u32) -> bool {
    nest_roundtrip(label, segments, value) == Ok(value)
}

/// The depth a value may reach before sqlite stops reading it back, found by
/// bisection.
#[test]
fn value_depth_boundary() {
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| {
            let ceiling = 4096u32;
            if nest_survives("sq_depth_hi", 1, ceiling) {
                println!("value depth: {ceiling} still reads back");
                return;
            }
            println!(
                "value depth {ceiling}: {:?}",
                nest_roundtrip("sq_depth_hi_why", 1, ceiling)
            );

            let mut good = 0u32;
            let mut bad = ceiling;
            while bad - good > 1 {
                let mid = good + (bad - good) / 2;
                if nest_survives(&format!("sq_depth_{mid}"), 1, mid) {
                    good = mid;
                } else {
                    bad = mid;
                }
            }
            println!("value depth at a two-level path: last kept = {good}, first lost = {bad}");
            println!("what {bad} does: {:?}", nest_roundtrip("sq_depth_edge", 1, bad));
            panic!(
                "a value nested {bad} deep is accepted by the write and does not read \
                 back; {good} does"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

/// Whether the levels of the path are spent out of the same budget as the
/// value.
#[test]
fn path_depth_does_not_spend_the_value_budget() {
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| {
            let mut disagreed = Vec::new();
            for probe in [64u32, 120, 126, 127, 128, 200, 250, 500, 1000] {
                let shallow = nest_roundtrip(&format!("sq_pd_s{probe}"), 1, probe);
                let deeper = nest_roundtrip(&format!("sq_pd_d{probe}"), 60, probe);
                println!("Nest({probe}): at 1 level {shallow:?}, at 60 levels {deeper:?}");
                if shallow == Ok(probe) && deeper != Ok(probe) {
                    disagreed.push(format!("Nest({probe}): 1 level ok, 60 levels {deeper:?}"));
                }
            }
            assert!(
                disagreed.is_empty(),
                "the same value reads back at a shallow path and not at a deep one, \
                 so the path spends the value's budget:\n{}",
                disagreed.join("\n")
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

/// A store must still open after it has written a value itself.
#[test]
fn a_deeply_nested_value_leaves_a_store_that_opens() {
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| {
            let file = TempPath::new("sq_depth_reopen");
            {
                let store = opened(file.path());
                store
                    .set(["probe", "v"], &Nest(4096))
                    .expect("the write is accepted");
                store.save_now().unwrap();
            }
            open(file.path()).expect("the store cannot open the file it wrote itself");
        })
        .unwrap()
        .join()
        .unwrap();
}

// ---------------------------------------------------------------------------
// 2. Numbers
// ---------------------------------------------------------------------------

#[test]
fn u64_max_reads_back() {
    assert_kept("sq_u64_max", u64::MAX);
}

#[test]
fn u64_above_i64_max_reads_back() {
    assert_kept("sq_u64_hi", (i64::MAX as u64) + 1);
}

#[test]
fn i64_min_reads_back() {
    assert_kept("sq_i64_min", i64::MIN);
}

#[test]
fn i64_max_reads_back() {
    assert_kept("sq_i64_max", i64::MAX);
}

#[test]
fn u128_max_reads_back() {
    assert_kept("sq_u128_max", u128::MAX);
}

#[test]
fn i128_min_reads_back() {
    assert_kept("sq_i128_min", i128::MIN);
}

#[test]
fn negative_zero_keeps_its_sign() {
    let back = value_roundtrip("sq_negzero", &-0.0f64).expect("the round trip should not fail");
    let back = back.expect("the path should hold a value");
    assert_eq!(
        back.to_bits(),
        (-0.0f64).to_bits(),
        "-0.0 came back as {back} with bits {:#x}",
        back.to_bits()
    );
}

#[test]
fn float_precision_survives() {
    for v in [
        0.1f64,
        0.1 + 0.2,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::EPSILON,
        1.7976931348623157e308,
        5e-324,
        123456789.123456789,
    ] {
        let back = value_roundtrip("sq_f64_prec", &v).expect("round trip");
        assert_eq!(
            back.map(f64::to_bits),
            Some(v.to_bits()),
            "{v} came back as {back:?}"
        );
    }
}

#[test]
fn integers_near_the_double_boundary_survive() {
    for v in [
        (1u64 << 53) - 1,
        1u64 << 53,
        (1u64 << 53) + 1,
        (1u64 << 63) + 12345,
        u64::MAX - 1,
    ] {
        assert_kept("sq_u64_edge", v);
    }
}

// ---------------------------------------------------------------------------
// 3. Strings as values
// ---------------------------------------------------------------------------

#[test]
fn empty_string_reads_back() {
    assert_kept("sq_str_empty", String::new());
}

#[test]
fn string_with_embedded_nul_reads_back() {
    assert_kept("sq_str_nul", "a\u{0}b".to_string());
}

#[test]
fn string_of_every_control_character_reads_back() {
    let s: String = (1u32..0x20).map(|c| char::from_u32(c).unwrap()).collect();
    assert_kept("sq_str_ctrl", s);
}

#[test]
fn one_megabyte_string_reads_back() {
    let s = "x".repeat(1 << 20);
    match value_roundtrip("sq_str_1mb", &s) {
        Ok(Some(back)) => assert_eq!(back.len(), s.len(), "length changed"),
        Ok(None) => panic!("a 1 MB string was accepted and reads back as nothing"),
        Err(e) => panic!("a 1 MB string: {e}"),
    }
}

#[test]
fn unicode_string_reads_back() {
    assert_kept(
        "sq_str_uni",
        "\u{1F600} \u{202E}rtl\u{202C} caf\u{e9} cafe\u{301} \u{10FFFF} \u{FEFF}".to_string(),
    );
}

#[test]
fn a_string_that_is_itself_json_reads_back() {
    assert_kept(
        "sq_str_json",
        "{\"a\": 1, \"b\": [null, NaN]}\u{0}\\\"".to_string(),
    );
}

#[test]
fn arbitrary_bytes_read_back() {
    let bytes: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
    assert_kept("sq_bytes", bytes);
}

#[test]
fn a_lone_high_codepoint_string_reads_back() {
    assert_kept("sq_str_max_cp", "\u{10FFFF}".to_string());
}

// ---------------------------------------------------------------------------
// 4. Path segments
// ---------------------------------------------------------------------------

/// The names to try as a single level under a fixed parent.
fn tricky_names() -> Vec<(&'static str, String)> {
    vec![
        ("dot_segment", ".".to_string()),
        ("dotdot_segment", "..".to_string()),
        ("inner_dot", "a.b".to_string()),
        ("backslash", "a\\b".to_string()),
        ("double_quote", "a\"b".to_string()),
        ("single_quote", "a'b".to_string()),
        ("percent", "a%b".to_string()),
        ("underscore_wildcard", "a_b".to_string()),
        ("newline", "a\nb".to_string()),
        ("tab", "a\tb".to_string()),
        ("nul", "a\u{0}b".to_string()),
        ("star", "a*b".to_string()),
        ("question", "a?b".to_string()),
        ("brackets", "a[b]c".to_string()),
        ("unicode", "\u{1F600}\u{e9}".to_string()),
        ("max_codepoint", "\u{10FFFF}".to_string()),
        ("max_codepoint_then_more", "\u{10FFFF}z".to_string()),
        ("init_marker_shape", "__init::probe".to_string()),
        ("sql_injection_shape", "a'; DROP TABLE data; --".to_string()),
        ("long", "L".repeat(100_000)),
    ]
}

#[test]
fn a_tricky_name_addresses_its_own_value() {
    let mut broken = Vec::new();
    for (label, name) in tricky_names() {
        let file = TempPath::new(&format!("sq_name_{label}"));
        let path = match StorePath::try_from_segments(["probe", name.as_str()]) {
            Ok(p) => p,
            Err(e) => {
                broken.push(format!("{label}: path refused: {e:?}"));
                continue;
            }
        };
        let outcome = (|| -> Result<(), String> {
            {
                let store = open(file.path()).map_err(|e| format!("open: {e:?}"))?;
                store
                    .set_owned(path.clone(), &7u32)
                    .map_err(|e| format!("write: {e:?}"))?;
                store.save_now().map_err(|e| format!("flush: {e:?}"))?;
            }
            let store = open(file.path()).map_err(|e| format!("reopen: {e:?}"))?;
            match store.get::<u32>(&path) {
                Ok(Some(7)) => {}
                other => return Err(format!("read: {other:?}")),
            }
            let keys = StoreBackend::scan_keys(&store, &StorePath::segment("probe"))
                .map_err(|e| format!("scan: {e:?}"))?;
            if keys != vec![path.clone()] {
                return Err(format!("scan of `probe` listed {keys:?}"));
            }
            Ok(())
        })();
        if let Err(e) = outcome {
            broken.push(format!("{label} ({name:?}): {e}", name = truncate(&name)));
        }
    }
    assert!(broken.is_empty(), "names that did not round trip:\n{}", broken.join("\n"));
}

fn truncate(s: &str) -> String {
    if s.chars().count() > 40 {
        format!("{}...", s.chars().take(40).collect::<String>())
    } else {
        s.to_string()
    }
}

#[test]
fn an_empty_level_is_refused_and_changes_nothing() {
    let file = TempPath::new("sq_empty_seg");
    let store = opened(file.path());
    store.set(["probe", "kept"], &1u32).unwrap();

    assert!(
        store.set(["probe", ""], &1u32).is_err(),
        "a level with no name was accepted"
    );
    store.save_now().unwrap();

    let keys = StoreBackend::scan_keys(&store, &StorePath::root()).unwrap();
    assert_eq!(
        keys,
        vec![StorePath::from_segments(["probe", "kept"])],
        "the refused write left something behind"
    );
}

// ---------------------------------------------------------------------------
// 5. Prefix scans
// ---------------------------------------------------------------------------

/// A sibling whose key begins with the prefix's characters must not be listed
/// by a scan of that prefix, whatever character follows.
#[test]
fn a_scan_of_a_prefix_lists_only_what_is_under_it() {
    let mut leaked = Vec::new();
    for c in [
        '\u{0}', '\u{1}', ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-',
        '/', '0', 'z', '{', '~',
    ] {
        let file = TempPath::new(&format!("sq_sib_{:04x}", c as u32));
        let sibling = format!("ui{c}x");
        let store = opened(file.path());
        store.set(["ui", "width"], &1u32).unwrap();
        store
            .set_owned(StorePath::segment(&sibling), &2u32)
            .unwrap();
        store.save_now().unwrap();
        drop(store);

        let store = opened(file.path());
        let keys = StoreBackend::scan_keys(&store, &StorePath::segment("ui")).unwrap();
        let expected = vec![StorePath::from_segments(["ui", "width"])];
        if keys != expected {
            leaked.push(format!(
                "sibling `ui{}x` (U+{:04X}): scan of `ui` listed {:?}",
                c.escape_debug(),
                c as u32,
                keys
            ));
        }
    }
    assert!(
        leaked.is_empty(),
        "a scan picked up keys that are not under the prefix:\n{}",
        leaked.join("\n")
    );
}

/// The same question asked of `delete_prefix`, which is built on the scan.
#[test]
fn delete_prefix_takes_only_the_subtree() {
    let mut destroyed = Vec::new();
    for c in ['\u{0}', ' ', '!', '%', '*', '-', 'z'] {
        let file = TempPath::new(&format!("sq_del_{:04x}", c as u32));
        let sibling = format!("ui{c}x");
        let store = opened(file.path());
        store.set(["ui", "width"], &1u32).unwrap();
        store
            .set_owned(StorePath::segment(&sibling), &2u32)
            .unwrap();
        store.save_now().unwrap();

        StoreBackend::delete_prefix(&store, &StorePath::segment("ui")).unwrap();
        store.save_now().unwrap();
        drop(store);

        let store = opened(file.path());
        let left = store
            .get::<u32>(StorePath::segment(&sibling))
            .unwrap_or(None);
        if left != Some(2) {
            destroyed.push(format!(
                "`ui{}x` (U+{:04X}) after deleting the `ui` subtree: {:?}",
                c.escape_debug(),
                c as u32,
                left
            ));
        }
    }
    assert!(
        destroyed.is_empty(),
        "delete_prefix took keys outside the subtree:\n{}",
        destroyed.join("\n")
    );
}

/// A child whose name sorts above the range's upper bound.
#[test]
fn a_child_above_the_range_bound_is_still_scanned() {
    let file = TempPath::new("sq_high_child");
    let child = StorePath::from_segments(["ui", "\u{10FFFF}z"]);
    {
        let store = opened(file.path());
        store.set(["ui", "a"], &1u32).unwrap();
        store.set_owned(child.clone(), &2u32).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(
        store.get::<u32>(&child).unwrap(),
        Some(2),
        "a direct read of the child"
    );
    let keys = StoreBackend::scan_keys(&store, &StorePath::segment("ui")).unwrap();
    assert!(
        keys.contains(&child),
        "a scan of `ui` did not list its own child: {keys:?}"
    );
    let root = StoreBackend::scan_keys(&store, &StorePath::root()).unwrap();
    assert!(
        root.contains(&child),
        "a scan of the root did not list the child: {root:?}"
    );
}

/// A top-level key that sorts above the root range's upper bound.
#[test]
fn a_root_key_above_the_range_bound_is_still_scanned() {
    let file = TempPath::new("sq_high_root");
    let key = StorePath::segment("\u{10FFFF}z");
    {
        let store = opened(file.path());
        store.set(["ui"], &1u32).unwrap();
        store.set_owned(key.clone(), &2u32).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(store.get::<u32>(&key).unwrap(), Some(2));
    let root = StoreBackend::scan_keys(&store, &StorePath::root()).unwrap();
    assert!(
        root.contains(&key),
        "a scan of the root did not list a key that was written: {root:?}"
    );
}

/// Paths that are prefixes of each other coexist, and each scan lists its own.
#[test]
fn nested_and_overlapping_paths_stay_apart() {
    let file = TempPath::new("sq_overlap");
    {
        let store = opened(file.path());
        store.set(["ui"], &1u32).unwrap();
        store.set(["ui", "width"], &2u32).unwrap();
        store.set(["uix"], &3u32).unwrap();
        store.set(["ui.width"], &4u32).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(store.get::<u32>(["ui"]).unwrap(), Some(1));
    assert_eq!(store.get::<u32>(["ui", "width"]).unwrap(), Some(2));
    assert_eq!(store.get::<u32>(["uix"]).unwrap(), Some(3));
    assert_eq!(store.get::<u32>(["ui.width"]).unwrap(), Some(4));

    let keys = StoreBackend::scan_keys(&store, &StorePath::segment("ui")).unwrap();
    assert_eq!(
        keys,
        vec![
            StorePath::segment("ui"),
            StorePath::from_segments(["ui", "width"]),
        ],
        "a scan of `ui` picked up a sibling"
    );
}

// ---------------------------------------------------------------------------
// 6. Structures
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Default)]
struct Unit;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Default)]
struct Empty {}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
enum Shape {
    Unit,
    Newtype(u32),
    Tuple(u32, String),
    Struct { a: u32, b: Option<u32> },
}

#[test]
fn empty_shapes_read_back() {
    assert_kept("sq_unit_struct", Unit);
    assert_kept("sq_empty_struct", Empty {});
    assert_kept("sq_unit", ());
    assert_kept("sq_empty_map", BTreeMap::<String, u32>::new());
    assert_kept("sq_empty_vec", Vec::<u32>::new());
    assert_kept("sq_empty_tuple_vec", Vec::<Empty>::new());
}

#[test]
fn option_none_reads_back() {
    match value_roundtrip("sq_none", &Option::<u32>::None) {
        Ok(Some(None)) => {}
        other => panic!("Option::None came back as {other:?}"),
    }
}

#[test]
fn nested_option_keeps_its_layers() {
    let mut lost = Vec::new();
    for v in [Some(Some(1u32)), Some(None::<u32>), None::<Option<u32>>] {
        let back = value_roundtrip("sq_nested_opt", &v).expect("round trip");
        if back != Some(v) {
            lost.push(format!("{v:?} came back as {back:?}"));
        }
    }
    assert!(
        lost.is_empty(),
        "Option<Option<u32>> lost a layer:\n{}",
        lost.join("\n")
    );
}

#[test]
fn enum_variants_read_back() {
    assert_kept("sq_enum_unit", Shape::Unit);
    assert_kept("sq_enum_newtype", Shape::Newtype(3));
    assert_kept("sq_enum_tuple", Shape::Tuple(3, "x".into()));
    assert_kept("sq_enum_struct", Shape::Struct { a: 1, b: None });
}

#[test]
fn a_map_keyed_by_awkward_strings_reads_back() {
    let mut map = BTreeMap::new();
    map.insert("".to_string(), 1u32);
    map.insert(".".to_string(), 2);
    map.insert("a.b".to_string(), 3);
    map.insert("a\u{0}b".to_string(), 4);
    map.insert("\u{10FFFF}".to_string(), 5);
    assert_kept("sq_map_keys", map);
}

#[test]
fn a_map_with_many_thousands_of_entries_reads_back() {
    let map: BTreeMap<String, u32> = (0u32..50_000).map(|i| (format!("k{i}"), i)).collect();
    match value_roundtrip("sq_big_map", &map) {
        Ok(Some(back)) => assert_eq!(back.len(), map.len(), "entries changed"),
        Ok(None) => panic!("a 50k-entry map was accepted and reads back as nothing"),
        Err(e) => panic!("a 50k-entry map: {e}"),
    }
}

#[test]
fn many_thousands_of_separate_paths_read_back() {
    let file = TempPath::new("sq_many_paths");
    {
        let store = opened(file.path());
        for i in 0u32..20_000 {
            store
                .set_owned(StorePath::from_segments(["many", &format!("k{i}")]), &i)
                .unwrap();
        }
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    let keys = StoreBackend::scan_keys(&store, &StorePath::segment("many")).unwrap();
    assert_eq!(keys.len(), 20_000, "a scan lost keys");
    assert_eq!(store.get::<u32>(["many", "k19999"]).unwrap(), Some(19_999));
}

// ---------------------------------------------------------------------------
// 7. The store's own bookkeeping, sharing a namespace with data
// ---------------------------------------------------------------------------

/// A namespace flag and a value at the same string are two different things,
/// and neither may cost the other.
///
/// The write buffer is one `HashMap<Arc<str>, PendingOp>` holding both, keyed
/// by the path for data and by the bare namespace name for the flag.
#[test]
fn a_namespace_flag_and_a_value_at_its_name_coexist() {
    let file = TempPath::new("sq_ns_collision");
    {
        let store = opened(file.path());
        store.set(["cfg"], &7u32).unwrap();
        store.mark_initialized("cfg").unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(
        store.get::<u32>(["cfg"]).unwrap(),
        Some(7),
        "the value at `cfg` was lost to the namespace flag of the same name"
    );
    assert!(
        store.is_initialized("cfg").unwrap(),
        "the namespace flag was lost to the value at the same path"
    );
}

/// The other order.
#[test]
fn a_value_written_after_a_namespace_flag_survives() {
    let file = TempPath::new("sq_ns_collision2");
    {
        let store = opened(file.path());
        store.mark_initialized("cfg").unwrap();
        store.set(["cfg"], &7u32).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(store.get::<u32>(["cfg"]).unwrap(), Some(7));
    assert!(store.is_initialized("cfg").unwrap());
}

// ---------------------------------------------------------------------------
// 8. The migration adapter's GLOB scan
// ---------------------------------------------------------------------------

/// Every character a `GLOB` pattern reads as something other than itself,
/// checked against the scan the store itself does.
///
/// The migration adapter scans with `key GLOB prefix*`; the store scans by
/// comparison. Where the two disagree, a migration sees a different store than
/// the application does.
#[test]
fn a_glob_metacharacter_in_a_prefix_scans_as_itself() {
    let mut wrong = Vec::new();
    for name in ["p*q", "p?q", "p[a]q", "p]q", "p[q"] {
        let file = TempPath::new("sq_glob");
        let store = opened(file.path());
        store
            .set_owned(StorePath::from_segments([name, "leaf"]), &1u32)
            .unwrap();
        store.set(["pxq", "leaf"], &2u32).unwrap();
        store.set(["paq", "leaf"], &3u32).unwrap();
        store.save_now().unwrap();

        let keys = StoreBackend::scan_keys(&store, &StorePath::segment(name)).unwrap();
        let expected = vec![StorePath::from_segments([name, "leaf"])];
        if keys != expected {
            wrong.push(format!("prefix {name:?}: scan listed {keys:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "a scan read a prefix as a pattern:\n{}",
        wrong.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 9. Residue: nothing the store writes for itself is addressable as data
// ---------------------------------------------------------------------------

#[test]
fn an_empty_store_lists_nothing() {
    let file = TempPath::new("sq_empty_store");
    {
        let store = opened(file.path());
        store.mark_initialized("probe").unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    let keys = StoreBackend::scan_keys(&store, &StorePath::root()).unwrap();
    assert!(
        keys.is_empty(),
        "a store nobody wrote data into lists {keys:?}"
    );
}

/// A child a scan will not list is a child `delete_prefix` will not take, so
/// the subtree survives its own deletion.
#[test]
fn delete_prefix_reaches_a_child_above_the_range_bound() {
    let file = TempPath::new("sq_del_high_child");
    let child = StorePath::from_segments(["ui", "\u{10FFFF}"]);
    {
        let store = opened(file.path());
        store.set(["ui", "a"], &1u32).unwrap();
        store.set_owned(child.clone(), &2u32).unwrap();
        store.save_now().unwrap();

        StoreBackend::delete_prefix(&store, &StorePath::segment("ui")).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(
        store.get::<u32>(&child).unwrap(),
        None,
        "the `ui` subtree was deleted and this child of it is still readable"
    );
}

/// A scan must answer the same before a flush and after one.
///
/// The buffered half of a scan is filtered by level and the committed half by
/// string comparison, so the flush is what decides which answer a caller gets.
#[test]
fn a_flush_does_not_change_what_a_scan_lists() {
    let mut changed = Vec::new();
    for name in ["ui!x", "ui x", "ui-x", "ui\u{10FFFF}"] {
        let file = TempPath::new("sq_flush_scan");
        let store = opened(file.path());
        store.set(["ui", "width"], &1u32).unwrap();
        store.set_owned(StorePath::segment(name), &2u32).unwrap();

        let before = StoreBackend::scan_keys(&store, &StorePath::segment("ui")).unwrap();
        store.save_now().unwrap();
        let after = StoreBackend::scan_keys(&store, &StorePath::segment("ui")).unwrap();

        if before != after {
            changed.push(format!(
                "with `{}` also written: before the flush {before:?}, after it {after:?}",
                name.escape_debug()
            ));
        }
    }
    assert!(
        changed.is_empty(),
        "a scan answered differently either side of a flush:\n{}",
        changed.join("\n")
    );
}

/// Two names that differ only past an embedded NUL stay two names.
#[test]
fn a_nul_in_a_name_does_not_end_it() {
    let file = TempPath::new("sq_nul_names");
    let short = StorePath::from_segments(["probe", "a"]);
    let long = StorePath::from_segments(["probe", "a\u{0}b"]);
    let other = StorePath::from_segments(["probe", "a\u{0}c"]);
    {
        let store = opened(file.path());
        store.set_owned(short.clone(), &1u32).unwrap();
        store.set_owned(long.clone(), &2u32).unwrap();
        store.set_owned(other.clone(), &3u32).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert_eq!(store.get::<u32>(&short).unwrap(), Some(1));
    assert_eq!(store.get::<u32>(&long).unwrap(), Some(2));
    assert_eq!(store.get::<u32>(&other).unwrap(), Some(3));
    let keys = StoreBackend::scan_keys(&store, &StorePath::segment("probe")).unwrap();
    assert_eq!(keys.len(), 3, "three names collapsed into {keys:?}");
}

/// A map whose keys are not strings.
#[test]
fn a_map_with_integer_keys_reads_back() {
    let mut map = BTreeMap::new();
    map.insert(0u64, "a".to_string());
    map.insert(u64::MAX, "b".to_string());
    map.insert(1u64 << 53, "c".to_string());
    assert_kept("sq_map_int_keys", map);
}

/// A float used as a map key, which JSON has no place to put.
#[test]
fn a_map_with_float_keys_reads_back() {
    let mut map = BTreeMap::new();
    map.insert("1.5".to_string(), 1u32);
    let mut floats: Vec<(String, u32)> = Vec::new();
    for (k, v) in &map {
        floats.push((k.clone(), *v));
    }
    let back = value_roundtrip("sq_map_float_keys", &map).expect("round trip");
    assert_eq!(back, Some(map), "a stringly float key changed: {floats:?}");
}

/// A `char`, including the ones a JSON string spells with an escape.
#[test]
fn chars_read_back() {
    let mut lost = Vec::new();
    for c in ['\u{0}', '\n', '"', '\\', '\u{7f}', '\u{10FFFF}', '\u{FEFF}'] {
        let back = value_roundtrip("sq_char", &c).expect("round trip");
        if back != Some(c) {
            lost.push(format!("{:?} came back as {back:?}", c.escape_debug()));
        }
    }
    assert!(lost.is_empty(), "a char changed:\n{}", lost.join("\n"));
}

/// f32 asks the same question f64 does.
#[test]
fn non_finite_f32_reads_back() {
    let mut lost = Vec::new();
    for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let back = value_roundtrip("sq_f32", &v);
        let kept = match &back {
            Ok(Some(b)) => b.is_nan() == v.is_nan() && (v.is_nan() || *b == v),
            _ => false,
        };
        if !kept {
            lost.push(format!("{v} came back as {back:?}"));
        }
    }
    assert!(lost.is_empty(), "a non-finite f32:\n{}", lost.join("\n"));
}

/// A struct with a non-finite field, which is how one actually reaches a store.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Window {
    width: f64,
    ratio: f64,
}

#[test]
fn a_struct_carrying_a_non_finite_field_reads_back() {
    let w = Window {
        width: 1280.0,
        ratio: f64::NAN,
    };
    let back = value_roundtrip("sq_struct_nan", &w);
    match back {
        Ok(Some(b)) => assert!(
            b.ratio.is_nan() && b.width == 1280.0,
            "the struct came back as {b:?}"
        ),
        other => panic!(
            "one bad field cost the whole struct, the other field with it: {}",
            first_line(&format!("{other:?}"))
        ),
    }
}

/// The one place a lost float reports nothing at all.
///
/// Everywhere else the value that replaced it will not decode as the type
/// asked for, so a read fails loudly. `Option<f64>` decodes `null` perfectly
/// well - as `None` - so the write returns `Ok`, the read returns `Ok`, and
/// the value is gone.
#[test]
fn an_optional_non_finite_float_is_still_there() {
    let back = value_roundtrip("sq_opt_nan", &Some(f64::NAN)).expect("the read should not fail");
    let inner = back.expect("the path should hold a value");
    assert!(
        inner.map(f64::is_nan) == Some(true),
        "Some(NaN) came back as {inner:?}, and nothing anywhere reported it"
    );
}

/// The same, one level further out, where a collection carries it.
#[test]
fn a_collection_of_optional_floats_keeps_what_it_held() {
    let written = vec![Some(1.0f64), Some(f64::NAN), None, Some(f64::INFINITY)];
    let back = value_roundtrip("sq_vec_opt_nan", &written).expect("the read should not fail");
    let back = back.expect("the path should hold a value");
    let shape: Vec<bool> = back.iter().map(Option::is_some).collect();
    assert_eq!(
        shape,
        vec![true, true, true, false],
        "the vector came back as {back:?}: entries that held a number now hold nothing"
    );
}

/// A write at the root, which is a path with no levels.
#[test]
fn a_write_at_the_root_is_refused_or_readable() {
    let file = TempPath::new("sq_root_write");
    let store = opened(file.path());
    store.set(["ui", "width"], &1u32).unwrap();

    let written = store.set_owned(StorePath::root(), &7u32);
    if let Err(e) = &written {
        println!("a write at the root is refused: {}", first_line(&format!("{e:?}")));
        return;
    }
    println!("a write at the root was accepted");
    store.save_now().unwrap();
    drop(store);

    let store = open(file.path()).expect("the store must still open");
    let keys = StoreBackend::scan_keys(&store, &StorePath::root());
    assert!(
        keys.is_ok(),
        "a write the store accepted made every later scan fail: {:?}",
        keys.err().map(|e| first_line(&format!("{e:?}")))
    );
    assert_eq!(
        store.get::<u32>(StorePath::root()).unwrap(),
        Some(7),
        "a write at the root reads back as nothing"
    );
}

/// Clearing a namespace flag must clear the one on disk, not the one in the
/// cache that a fresh process does not have.
#[test]
fn a_namespace_can_be_returned_to_fresh_after_a_reopen() {
    use amethystate::store::InitState;

    let file = TempPath::new("sq_ns_fresh");
    {
        let store = opened(file.path());
        store.mark_initialized("cfg").unwrap();
        store.save_now().unwrap();
    }
    {
        let store = opened(file.path());
        store.set_initialized("cfg", InitState::Fresh).unwrap();
        store.save_now().unwrap();
    }
    let store = opened(file.path());
    assert!(
        !store.is_initialized("cfg").unwrap(),
        "the namespace was set back to fresh and still reads as seeded, so the \
         next construction will not put the defaults back"
    );
}

#[test]
fn a_write_and_a_delete_leave_the_store_as_it_was() {
    let file = TempPath::new("sq_write_delete");
    let store = opened(file.path());
    store.set(["a"], &1u32).unwrap();
    store.save_now().unwrap();
    let before = StoreBackend::scan_prefix(&store, &StorePath::root()).unwrap();

    store.set(["b", "c"], &2u32).unwrap();
    StoreBackend::delete(&store, &StorePath::from_segments(["b", "c"])).unwrap();
    store.save_now().unwrap();
    drop(store);

    let store = opened(file.path());
    let after = StoreBackend::scan_prefix(&store, &StorePath::root()).unwrap();
    assert_eq!(before, after, "a write and its delete left residue");
}
