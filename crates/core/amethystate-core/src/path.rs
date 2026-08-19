use std::sync::Arc;

const SEPARATOR: char = '.';
const ESCAPE: char = '\\';

/// Where a value lives in the store, as the levels it is under rather than as
/// one string.
///
/// A path is built from segments and only from segments, so a name is a name:
/// putting `"dark.mode"` in one addresses a single value with a dot in it, not
/// two levels. Both forms are kept - the segments for engines that walk a
/// document tree, and the joined string for engines that store a key whole - so
/// neither costs an allocation to read.
#[derive(Clone)]
pub struct Path {
    segments: Segments,
    joined: Joined,
}

#[derive(Clone)]
enum Segments {
    Static(&'static [&'static str]),
    Owned(Arc<[Arc<str>]>),
}

#[derive(Clone)]
enum Joined {
    Static(&'static str),
    Owned(Arc<str>),
}

impl Segments {
    fn len(&self) -> usize {
        match self {
            Segments::Static(s) => s.len(),
            Segments::Owned(s) => s.len(),
        }
    }

    fn get(&self, index: usize) -> Option<&str> {
        match self {
            Segments::Static(s) => s.get(index).copied(),
            Segments::Owned(s) => s.get(index).map(|s| &**s),
        }
    }

    fn to_owned_vec(&self) -> Vec<Arc<str>> {
        match self {
            Segments::Static(s) => s.iter().map(|s| Arc::from(*s)).collect(),
            Segments::Owned(s) => s.to_vec(),
        }
    }
}

impl Path {
    /// The path that is under nothing.
    pub const fn root() -> Self {
        Self {
            segments: Segments::Static(&[]),
            joined: Joined::Static(""),
        }
    }

    /// A path whose levels are known when the code is compiled.
    ///
    /// Both forms are handed over ready, so this allocates nothing and cannot
    /// fail: whoever writes them - the macro, in practice - is the one that
    /// checks them, and does it at expansion time rather than at startup.
    ///
    /// `joined` must be what [`Path::as_str`] would produce for `segments`.
    pub const fn from_static(segments: &'static [&'static str], joined: &'static str) -> Self {
        Self {
            segments: Segments::Static(segments),
            joined: Joined::Static(joined),
        }
    }

    /// One level named `name`, whatever `name` contains - except nothing.
    pub fn segment(name: impl AsRef<str>) -> Result<Self, PathError> {
        Self::from_segments([name])
    }

    pub fn from_segments<I, S>(segments: I) -> Result<Self, PathError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut collected: Vec<Arc<str>> = Vec::new();

        for segment in segments {
            let segment = segment.as_ref();
            if segment.is_empty() {
                return Err(PathError::EmptySegment);
            }
            collected.push(Arc::from(segment));
        }

        Ok(Self::from_checked(collected))
    }

    fn from_checked(segments: Vec<Arc<str>>) -> Self {
        let joined = join(&segments);

        Self {
            segments: Segments::Owned(Arc::from(segments)),
            joined: Joined::Owned(joined),
        }
    }

    /// This path with one more level under it.
    pub fn push(&self, name: impl AsRef<str>) -> Result<Self, PathError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err(PathError::EmptySegment);
        }

        let mut segments = self.segments.to_owned_vec();
        segments.push(Arc::from(name));
        Ok(Self::from_checked(segments))
    }

    /// This path with `other`'s levels under it.
    pub fn join(&self, other: &Path) -> Self {
        let mut segments = self.segments.to_owned_vec();
        segments.extend(other.segments.to_owned_vec());
        Self::from_checked(segments)
    }

    /// The levels, outermost first.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        (0..self.segments.len()).map(|i| self.segments.get(i).unwrap())
    }

    /// One level, or `None` past the end.
    pub fn segment_at(&self, index: usize) -> Option<&str> {
        self.segments.get(index)
    }

    /// The whole path as one string, with the separator escaped inside names.
    ///
    /// This is what a flat engine stores as its key. Reading it costs nothing:
    /// it is built once, when the path is.
    pub fn as_str(&self) -> &str {
        match &self.joined {
            Joined::Static(s) => s,
            Joined::Owned(s) => s,
        }
    }

    pub fn is_root(&self) -> bool {
        self.segments.len() == 0
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.is_root()
    }

    /// Whether every level of `prefix` starts this path.
    ///
    /// Compared level by level, so `ui` does not start `uix.width` - which
    /// comparing the joined strings would say it does.
    pub fn starts_with(&self, prefix: &Path) -> bool {
        prefix.segments.len() <= self.segments.len()
            && prefix.segments().zip(self.segments()).all(|(a, b)| a == b)
    }

    /// The levels below `prefix`, or `None` when `prefix` does not start this
    /// path.
    pub fn strip_prefix(&self, prefix: &Path) -> Option<Path> {
        self.starts_with(prefix).then(|| {
            Path::from_checked(self.segments.to_owned_vec()[prefix.segments.len()..].to_vec())
        })
    }

    /// The path one level up, or `None` at the root.
    pub fn parent(&self) -> Option<Path> {
        (!self.is_root()).then(|| {
            let mut segments = self.segments.to_owned_vec();
            segments.pop();
            Path::from_checked(segments)
        })
    }

    /// The last level, or `None` at the root.
    pub fn name(&self) -> Option<&str> {
        self.segments.get(self.segments.len().checked_sub(1)?)
    }

    /// Reads back what [`Path::as_str`] wrote.
    ///
    /// Only for data already on disk: a path in code is built from its
    /// segments, so nothing else needs to parse one. Fallible because a key
    /// this library did not write can hold a level with no name, and such a
    /// path is not one.
    pub fn parse_joined(joined: &str) -> Result<Self, PathError> {
        let mut segments: Vec<Arc<str>> = Vec::new();
        let mut current = String::new();
        let mut escaped = false;

        for ch in joined.chars() {
            match ch {
                _ if escaped => {
                    if ch != SEPARATOR && ch != ESCAPE {
                        return Err(PathError::DanglingEscape);
                    }
                    current.push(ch);
                    escaped = false;
                }
                ESCAPE => escaped = true,
                SEPARATOR => segments.push(Arc::from(std::mem::take(&mut current).as_str())),
                _ => current.push(ch),
            }
        }

        if escaped {
            return Err(PathError::DanglingEscape);
        }

        if !joined.is_empty() {
            segments.push(Arc::from(current.as_str()));
        }

        if segments.iter().any(|s| s.is_empty()) {
            return Err(PathError::EmptySegment);
        }

        Ok(Self::from_checked(segments))
    }
}

/// Why a set of segments is not a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// A level with no name. It would be indistinguishable from the root once
    /// joined, and there is nothing a store could address by it.
    EmptySegment,

    /// An escape that escapes nothing. No key this type wrote holds one, and
    /// reading it leniently would let two different keys name one path.
    DanglingEscape,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::EmptySegment => f.write_str("a path segment cannot be empty"),
            PathError::DanglingEscape => {
                f.write_str("an escape must be followed by a separator or another escape")
            }
        }
    }
}

impl std::error::Error for PathError {}

fn join(segments: &[Arc<str>]) -> Arc<str> {
    let mut out = String::new();

    for (i, segment) in segments.iter().enumerate() {
        if i > 0 {
            out.push(SEPARATOR);
        }
        for ch in segment.chars() {
            if ch == SEPARATOR || ch == ESCAPE {
                out.push(ESCAPE);
            }
            out.push(ch);
        }
    }

    Arc::from(out.as_str())
}

impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self.segments().zip(other.segments()).all(|(a, b)| a == b)
    }
}

impl Eq for Path {}

impl std::hash::Hash for Path {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.segments.len());
        for segment in self.segments() {
            segment.hash(state);
        }
    }
}

impl std::fmt::Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Path({:?})", self.as_str())
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Weighted towards what turns up in a segment and towards what can break
    /// one. The letter range is deliberately tiny so that different segments
    /// collide often; digits get their own arm because a map keyed by a number
    /// stores exactly those, and `any::<char>()` alone would sample ten of them
    /// out of a million code points.
    fn segment_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                4 => Just(SEPARATOR),
                4 => Just(ESCAPE),
                4 => prop::char::range('a', 'c'),
                4 => prop::char::range('0', '9'),
                2 => prop_oneof![Just('_'), Just('-'), Just(' '), Just('/')],
                1 => any::<char>(),
            ],
            1..6,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn dotted_segment() -> impl Strategy<Value = String> {
        (segment_strategy(), segment_strategy()).prop_map(|(a, b)| format!("{a}.{b}"))
    }

    fn path_strategy() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(segment_strategy(), 1..5)
    }

    /// Whole keys rather than segments: what a flat engine hands back, which
    /// includes strings this library never wrote. Weighted the same way as a
    /// segment and allowed to be empty, since the root's key is.
    fn key_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                4 => Just(SEPARATOR),
                4 => Just(ESCAPE),
                4 => prop::char::range('a', 'c'),
                4 => prop::char::range('0', '9'),
                1 => any::<char>(),
            ],
            0..8,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn hash_of(value: &impl std::hash::Hash) -> u64 {
        use std::hash::Hasher;

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    proptest! {
        /// The point of the type, at any depth: a name is whatever the caller
        /// passed, and a separator inside it stays a character. The levels come
        /// back exactly as they went in, never more of them, and taking the
        /// names apart at the separator never lands on the same path - when it
        /// lands on one at all, since a name of nothing but separators splits
        /// into levels with no names.
        #[test]
        fn a_separator_inside_a_name_is_never_a_level(
            segments in prop::collection::vec(dotted_segment(), 1..16)
        ) {
            let path = Path::from_segments(&segments).unwrap();

            prop_assert_eq!(path.len(), segments.len());
            prop_assert_eq!(
                path.segments().map(|s| s.to_string()).collect::<Vec<_>>(),
                segments.clone()
            );

            let split: Vec<&str> = segments.iter().flat_map(|s| s.split('.')).collect();
            if let Ok(taken_apart) = Path::from_segments(&split) {
                prop_assert_ne!(taken_apart, path);
            }
        }
        /// The joined form is the only thing a flat engine keeps, so the
        /// segments have to be recoverable from it - whatever the names hold.
        ///
        /// Follows from the two below: a key round trips, and the join is
        /// injective, so the levels recovered from `join(s)` can only be `s`.
        /// Kept because it fails saying that directly rather than leaving the
        /// inference to the reader.
        #[test]
        fn the_joined_form_round_trips(segments in path_strategy()) {
            let path = Path::from_segments(&segments).unwrap();
            prop_assert_eq!(Path::parse_joined(path.as_str()).unwrap(), path);
        }

        /// The property the whole design rests on: two different sets of levels
        /// never land on the same key. Without it a name holding a separator
        /// could collide with a nesting a caller meant.
        #[test]
        fn different_levels_never_join_to_one_key(a in path_strategy(), b in path_strategy()) {
            let pa = Path::from_segments(&a).unwrap();
            let pb = Path::from_segments(&b).unwrap();

            prop_assert_eq!(a == b, pa.as_str() == pb.as_str());
        }

        /// Prefix matching is over levels, and stripping one is its inverse.
        #[test]
        fn a_prefix_is_stripped_back_off(head in path_strategy(), tail in path_strategy()) {
            let prefix = Path::from_segments(&head).unwrap();
            let full = prefix.join(&Path::from_segments(&tail).unwrap());

            prop_assert!(full.starts_with(&prefix));
            prop_assert_eq!(
                full.strip_prefix(&prefix).unwrap(),
                Path::from_segments(&tail).unwrap()
            );
        }

        /// One level at a time or all at once is the same path, and joining a
        /// single-level path is what pushing means. Equality is over the
        /// levels, so how a path was built never shows.
        #[test]
        fn a_path_does_not_remember_how_it_was_built(segments in path_strategy()) {
            let all_at_once = Path::from_segments(&segments).unwrap();

            let mut one_at_a_time = Path::root();
            for segment in &segments {
                one_at_a_time = one_at_a_time.push(segment).unwrap();
            }

            let by_joining = segments.iter().fold(Path::root(), |acc, segment| {
                acc.join(&Path::segment(segment).unwrap())
            });

            prop_assert_eq!(&one_at_a_time, &all_at_once);
            prop_assert_eq!(&by_joining, &all_at_once);
            prop_assert_eq!(one_at_a_time.as_str(), all_at_once.as_str());
        }

        /// The root is under everything, so stripping it is the identity.
        #[test]
        fn every_path_is_under_the_root(segments in path_strategy()) {
            let path = Path::from_segments(&segments).unwrap();
            let root = Path::root();

            prop_assert!(path.starts_with(&root));
            prop_assert_eq!(path.strip_prefix(&root).unwrap(), path.clone());
            prop_assert!(!root.starts_with(&path));
        }

        /// A level with no name would join to the same string as the root, so
        /// it is not a path at all - wherever in the list it turns up.
        #[test]
        fn a_level_with_no_name_is_not_a_path(
            segments in path_strategy(),
            at in 0usize..8
        ) {
            let mut with_a_hole = segments.clone();
            let at = at % (with_a_hole.len() + 1);
            with_a_hole.insert(at, String::new());

            prop_assert_eq!(
                Path::from_segments(&with_a_hole),
                Err(PathError::EmptySegment)
            );
            prop_assert_eq!(
                Path::from_segments(&segments).unwrap().push(""),
                Err(PathError::EmptySegment)
            );
        }

        /// Any name is a name, however many separators it holds: building
        /// always succeeds, and however many of them the names contain, exactly
        /// one per gap between levels survives unescaped in the joined form.
        #[test]
        fn every_separator_inside_a_name_is_escaped(segments in path_strategy()) {
            let path = Path::from_segments(&segments).unwrap();

            let mut unescaped = 0usize;
            let mut escaped = false;
            for ch in path.as_str().chars() {
                match ch {
                    _ if escaped => escaped = false,
                    ESCAPE => escaped = true,
                    SEPARATOR => unescaped += 1,
                    _ => {}
                }
            }

            prop_assert_eq!(unescaped, segments.len() - 1);
        }

        /// Growing a name always leaves the joined form a string prefix of the
        /// longer one, and the answer over levels is always still no. Reading
        /// that string prefix as a path prefix is what makes an unrelated
        /// subtree get scanned, or deleted.
        #[test]
        fn growing_a_name_never_makes_it_a_prefix(
            head in path_strategy(),
            extra in segment_strategy()
        ) {
            let base = Path::from_segments(&head).unwrap();

            let mut grown = head.clone();
            grown.last_mut().unwrap().push_str(&extra);
            let longer = Path::from_segments(&grown).unwrap();

            prop_assert!(longer.as_str().starts_with(base.as_str()));

            prop_assert_ne!(&longer, &base);
            prop_assert!(!longer.starts_with(&base));
            prop_assert!(!base.starts_with(&longer));
        }

        /// And the two answers never disagree: stripping succeeds exactly when
        /// the prefix is one, so nothing that is not under a prefix can be
        /// mistaken for something that is.
        #[test]
        fn stripping_succeeds_exactly_when_the_prefix_matches(
            a in path_strategy(),
            b in path_strategy()
        ) {
            let path = Path::from_segments(&a).unwrap();
            let candidate = Path::from_segments(&b).unwrap();

            prop_assert_eq!(
                path.strip_prefix(&candidate).is_some(),
                path.starts_with(&candidate)
            );
        }

        /// The other half of the round trip, over strings this library did not
        /// write. A key that parses at all has to join back to the key it came
        /// from, because a flat engine addresses a value by that string: where
        /// two keys parse to one path, one of the two values is unreachable and
        /// the next write over that path destroys the other.
        #[test]
        fn a_key_that_parses_joins_back_to_itself(key in key_strategy()) {
            if let Ok(path) = Path::parse_joined(&key) {
                prop_assert_eq!(path.as_str(), key.as_str());
            }
        }


        /// What a flat engine has to do with `as_str`, in both directions: a key
        /// is strictly under a path exactly when it starts with that path's key
        /// and a separator. The forward half is the escaping; the backward half
        /// is what stops a scan for `ui` reaching `uix.width`, and it is the
        /// only thing making the joined form safe to range over.
        #[test]
        fn a_key_is_under_a_path_exactly_when_it_starts_with_it_and_a_separator(
            head in path_strategy(),
            tail in path_strategy(),
            other in path_strategy()
        ) {
            let prefix = Path::from_segments(&head).unwrap();
            let boundary = format!("{}{}", prefix.as_str(), SEPARATOR);

            let candidates = [
                prefix.join(&Path::from_segments(&tail).unwrap()),
                Path::from_segments(&other).unwrap(),
                prefix.clone(),
                Path::from_segments(&tail).unwrap().join(&prefix),
            ];

            for candidate in candidates {
                let under = candidate.starts_with(&prefix) && candidate != prefix;
                prop_assert_eq!(under, candidate.as_str().starts_with(&boundary));
            }
        }

        /// Nothing that skips the check can make a path the check would refuse:
        /// whatever `join`, `push`, `parent`, `strip_prefix` and `parse_joined`
        /// hand back is rebuildable from its own levels, unchanged.
        #[test]
        fn no_call_makes_a_path_from_segments_would_refuse(
            a in path_strategy(),
            b in path_strategy()
        ) {
            let pa = Path::from_segments(&a).unwrap();
            let pb = Path::from_segments(&b).unwrap();

            let mut derived = vec![
                pa.join(&pb),
                pa.push("x").unwrap(),
                Path::parse_joined(pa.as_str()).unwrap(),
                pa.join(&Path::root()),
            ];
            derived.extend(pa.parent());
            derived.extend(pa.join(&pb).strip_prefix(&pa));

            for path in derived {
                let rebuilt = Path::from_segments(path.segments()).unwrap();
                prop_assert_eq!(&rebuilt, &path);
                prop_assert_eq!(rebuilt.as_str(), path.as_str());
            }
        }

        /// Equality, hashing and the key never disagree, over paths built every
        /// way there is - including the root and the paths the derived calls
        /// hand back, which the other properties never reach.
        #[test]
        fn equality_hashing_and_the_key_agree(a in path_strategy(), b in path_strategy()) {
            let pa = Path::from_segments(&a).unwrap();
            let pb = Path::from_segments(&b).unwrap();

            let left = [pa.clone(), pa.parent().unwrap_or_else(Path::root), Path::root()];
            let right = [pb.clone(), pa.join(&pb).strip_prefix(&pa).unwrap(), Path::root()];

            for x in &left {
                for y in &right {
                    prop_assert_eq!(x == y, x.as_str() == y.as_str());
                    if x == y {
                        prop_assert_eq!(hash_of(x), hash_of(y));
                    }
                }
            }
        }
    }

    /// A golden for one decision, not for a rule: that separators inside a name
    /// are escaped is a property and covered by one, and that two different sets
    /// of levels never share a key is another. All this pins is which character
    /// does the escaping - changing it silently renames every key in every file
    /// already written.
    #[test]
    fn the_encoding_is_a_backslash_before_the_separator() {
        assert_eq!(Path::segment("dark.mode").unwrap().as_str(), "dark\\.mode");
        assert_eq!(
            Path::from_segments(["dark", "mode"]).unwrap().as_str(),
            "dark.mode"
        );
    }

    #[test]
    fn the_root_is_empty_and_stays_empty() {
        let root = Path::root();

        assert!(root.is_root());
        assert_eq!(root.as_str(), "");
        assert_eq!(Path::parse_joined("").unwrap(), root);
        assert_eq!(root.parent(), None);
        assert_eq!(root.name(), None);
    }

    /// The two smallest keys that are not the joined form of anything: an
    /// escape before an ordinary character, and an escape at the end. Both are
    /// read as if the escape were not there, so each is a second key for a path
    /// that already has one.
    #[test]
    fn a_key_no_join_could_have_written_is_not_a_second_name_for_one() {
        assert_ne!(
            Path::parse_joined("a\\b").ok(),
            Path::parse_joined("ab").ok()
        );
        assert_ne!(Path::parse_joined("a\\").ok(), Path::parse_joined("a").ok());
    }

    /// The boundary that makes a flat scan safe cannot be spelled at the root.
    /// Every path is under the root, and no key can begin with a separator - a
    /// name that begins with one escapes it - so an engine that derives its
    /// scan bound the same way at every depth scans nothing at the top.
    #[test]
    fn the_root_has_no_separator_boundary() {
        let root = Path::root();
        let child = Path::from_segments(["ui", "width"]).unwrap();

        assert!(child.starts_with(&root));
        assert_eq!(format!("{}{}", root.as_str(), SEPARATOR), ".");
        assert!(!child.as_str().starts_with(SEPARATOR));
        assert!(
            !Path::segment(".hidden")
                .unwrap()
                .as_str()
                .starts_with(SEPARATOR)
        );
    }

    /// A node's own key is not under its own boundary, so the string test and
    /// the level test disagree on exactly one path: the prefix itself. An
    /// engine that deletes a subtree by that boundary leaves the value stored at
    /// the node behind, and one that scans by it never lists that value.
    #[test]
    fn a_subtree_boundary_does_not_cover_the_node_itself() {
        let node = Path::segment("ui").unwrap();
        let boundary = format!("{}{}", node.as_str(), SEPARATOR);

        assert!(node.push("width").unwrap().as_str().starts_with(&boundary));
        assert!(!node.as_str().starts_with(&boundary));
        assert!(node.starts_with(&node));
    }

    /// Escaping covers the separator and the escape and nothing else, so a key
    /// carries whatever else a name held. The sqlite engine builds its scan out
    /// of `key GLOB prefix*`, where every one of these means something other
    /// than itself.
    #[test]
    fn a_key_carries_what_a_glob_pattern_reads() {
        let path = Path::segment("a*b[c]?\u{0}d").unwrap();

        assert_eq!(path.as_str(), "a*b[c]?\u{0}d");
    }

    /// Keys the reader in use today accepts and this one refuses. `.` is the
    /// sentinel `split_path` maps to the whole document, and a trailing
    /// separator is what `join_path` writes for an empty key: both are already
    /// in stores, and neither is a path.
    #[test]
    fn keys_already_written_that_are_not_paths() {
        assert_eq!(Path::parse_joined("."), Err(PathError::EmptySegment));
        assert_eq!(Path::parse_joined("ui."), Err(PathError::EmptySegment));
        assert_eq!(
            Path::parse_joined("ui..width"),
            Err(PathError::EmptySegment)
        );
    }

    /// The hash is taken over the levels, not over the key, so the two do not
    /// agree even though equality does. A map keyed by a path cannot be probed
    /// with the string a flat engine already holds - `Borrow<str>` would be
    /// unsound - so every lookup from a stored key has to build a path first.
    #[test]
    fn a_path_does_not_hash_like_its_key() {
        let path = Path::segment("ui").unwrap();

        assert_eq!(path.as_str(), "ui");
        assert_ne!(hash_of(&path), hash_of(&"ui"));
    }

    /// A name is bytes, so two spellings a person and most editors read as one
    /// name are two levels with two keys. Normalising a file on save moves
    /// every value under such a name to a path nothing looks up.
    #[test]
    fn two_spellings_of_one_name_are_two_paths() {
        let precomposed = Path::segment("caf\u{e9}").unwrap();
        let decomposed = Path::segment("cafe\u{301}").unwrap();

        assert_ne!(precomposed, decomposed);
        assert_ne!(precomposed.as_str(), decomposed.as_str());
        assert_eq!(precomposed.to_string().chars().count(), 4);
        assert_eq!(decomposed.to_string().chars().count(), 5);
    }

    /// A document engine walks levels as `&[&str]`; the segments are
    /// `&[Arc<str>]` and nothing converts between them, so every get, set and
    /// delete allocates a vector to pass the path it was given.
    /// The document engines walk a level at a time, and now get `&str` without
    /// anything in between.
    #[test]
    fn walking_a_document_borrows_each_level() {
        let path = Path::from_segments(["ui", "window", "width"]).unwrap();

        assert_eq!(
            path.segments().collect::<Vec<_>>(),
            ["ui", "window", "width"]
        );
        assert_eq!(path.segment_at(1), Some("window"));
        assert_eq!(path.segment_at(3), None);
    }

    /// A path the compiler knows costs nothing to build and cannot be wrong,
    /// because whoever wrote the levels checked them where they were written.
    #[test]
    fn a_static_path_is_the_same_path() {
        static UI_WIDTH: Path = Path::from_static(&["ui", "width"], "ui.width");

        assert_eq!(UI_WIDTH, Path::from_segments(["ui", "width"]).unwrap());
        assert_eq!(UI_WIDTH.as_str(), "ui.width");
        assert_eq!(UI_WIDTH.name(), Some("width"));
        assert!(UI_WIDTH.starts_with(&Path::from_static(&["ui"], "ui")));
    }
}
