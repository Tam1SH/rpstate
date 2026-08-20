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
pub struct StorePath {
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

impl StorePath {
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
    /// `joined` must be what [`StorePath::as_str`] would produce for `segments`.
    pub const fn from_static(segments: &'static [&'static str], joined: &'static str) -> Self {
        Self {
            segments: Segments::Static(segments),
            joined: Joined::Static(joined),
        }
    }

    /// One level named `name`, whatever `name` contains - except nothing.
    ///
    /// # Panics
    ///
    /// If `name` is empty. Use [`StorePath::try_segment`] for a name that comes
    /// from data rather than from the source.
    pub fn segment(name: impl AsRef<str>) -> Self {
        Self::from_segments([name])
    }

    /// [`StorePath::segment`] for a name that can turn out to be empty.
    pub fn try_segment(name: impl AsRef<str>) -> Result<Self, StorePathError> {
        Self::try_from_segments([name])
    }

    /// A path out of the levels it is under, outermost first.
    ///
    /// # Panics
    ///
    /// If any level is empty, which is a path that cannot exist - see
    /// [`StorePathError::EmptySegment`]. Written-out levels are the source's to
    /// get right; levels that come from data go through
    /// [`StorePath::try_from_segments`] instead, and every call that builds a
    /// path out of a caller's strings - [`IntoStorePath`], and so `Store::get`,
    /// `Kv::set`, `ReactiveMap::insert` - already does.
    pub fn from_segments<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::try_from_segments(segments).expect("a path segment cannot be empty")
    }

    /// [`StorePath::from_segments`] for levels that can turn out to be empty.
    pub fn try_from_segments<I, S>(segments: I) -> Result<Self, StorePathError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut collected: Vec<Arc<str>> = Vec::new();

        for segment in segments {
            let segment = segment.as_ref();
            if segment.is_empty() {
                return Err(StorePathError::EmptySegment);
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
    ///
    /// # Panics
    ///
    /// If `name` is empty. Use [`StorePath::try_push`] for a name that comes
    /// from data.
    pub fn push(&self, name: impl AsRef<str>) -> Self {
        self.try_push(name).expect("a path segment cannot be empty")
    }

    /// [`StorePath::push`] for a name that can turn out to be empty.
    pub fn try_push(&self, name: impl AsRef<str>) -> Result<Self, StorePathError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err(StorePathError::EmptySegment);
        }

        let mut segments = self.segments.to_owned_vec();
        segments.push(Arc::from(name));
        Ok(Self::from_checked(segments))
    }

    /// This path with `other`'s levels under it.
    pub fn join(&self, other: &StorePath) -> Self {
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
    pub fn starts_with(&self, prefix: &StorePath) -> bool {
        prefix.segments.len() <= self.segments.len()
            && prefix.segments().zip(self.segments()).all(|(a, b)| a == b)
    }

    /// The levels below `prefix`, or `None` when `prefix` does not start this
    /// path.
    pub fn strip_prefix(&self, prefix: &StorePath) -> Option<StorePath> {
        self.starts_with(prefix).then(|| {
            StorePath::from_checked(self.segments.to_owned_vec()[prefix.segments.len()..].to_vec())
        })
    }

    /// The path one level up, or `None` at the root.
    pub fn parent(&self) -> Option<StorePath> {
        (!self.is_root()).then(|| {
            let mut segments = self.segments.to_owned_vec();
            segments.pop();
            StorePath::from_checked(segments)
        })
    }

    /// The last level, or `None` at the root.
    pub fn name(&self) -> Option<&str> {
        self.segments.get(self.segments.len().checked_sub(1)?)
    }

    /// The name `key` is stored under, below this path.
    ///
    /// What a flat engine's scan needs: it hands back whole keys, and a caller
    /// that addressed a subtree wants the name of each thing in it. `None` when
    /// `key` is not a path this type could have written, or is not under this
    /// one.
    pub fn entry_name(&self, key: &str) -> Option<String> {
        StorePath::parse_joined(key)
            .ok()
            .and_then(|path| path.strip_prefix(self))
            .and_then(|rest| rest.name().map(str::to_string))
    }

    /// Reads back what [`StorePath::as_str`] wrote.
    ///
    /// Only for data already on disk: a path in code is built from its
    /// segments, so nothing else needs to parse one. Fallible because a key
    /// this library did not write can hold a level with no name, and such a
    /// path is not one.
    pub fn parse_joined(joined: &str) -> Result<Self, StorePathError> {
        let mut segments: Vec<Arc<str>> = Vec::new();
        let mut current = String::new();
        let mut escaped = false;

        for ch in joined.chars() {
            match ch {
                _ if escaped => {
                    if ch != SEPARATOR && ch != ESCAPE {
                        return Err(StorePathError::DanglingEscape);
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
            return Err(StorePathError::DanglingEscape);
        }

        if !joined.is_empty() {
            segments.push(Arc::from(current.as_str()));
        }

        if segments.iter().any(|s| s.is_empty()) {
            return Err(StorePathError::EmptySegment);
        }

        Ok(Self::from_checked(segments))
    }
}

/// Why a set of segments is not a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorePathError {
    /// A level with no name. It would be indistinguishable from the root once
    /// joined, and there is nothing a store could address by it.
    EmptySegment,

    /// An escape that escapes nothing. No key this type wrote holds one, and
    /// reading it leniently would let two different keys name one path.
    DanglingEscape,
}

impl std::fmt::Display for StorePathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorePathError::EmptySegment => f.write_str("a path segment cannot be empty"),
            StorePathError::DanglingEscape => {
                f.write_str("an escape must be followed by a separator or another escape")
            }
        }
    }
}

impl std::error::Error for StorePathError {}

/// What a call can be given where a path is wanted.
///
/// A list of levels, or a path already built. Deliberately not `&str`: a string
/// is a name, and letting one stand in for a path is the confusion this type
/// exists to end - `store.get(["ui", "width"])` and `store.get("ui.width")`
/// would otherwise look alike and mean different things.
pub trait IntoStorePath {
    fn into_store_path(self) -> Result<StorePath, StorePathError>;
}

impl IntoStorePath for StorePath {
    fn into_store_path(self) -> Result<StorePath, StorePathError> {
        Ok(self)
    }
}

impl IntoStorePath for &StorePath {
    fn into_store_path(self) -> Result<StorePath, StorePathError> {
        Ok(self.clone())
    }
}

impl<S: AsRef<str>, const N: usize> IntoStorePath for [S; N] {
    fn into_store_path(self) -> Result<StorePath, StorePathError> {
        StorePath::try_from_segments(self)
    }
}

impl<S: AsRef<str>> IntoStorePath for &[S] {
    fn into_store_path(self) -> Result<StorePath, StorePathError> {
        StorePath::try_from_segments(self)
    }
}

impl<S: AsRef<str>> IntoStorePath for Vec<S> {
    fn into_store_path(self) -> Result<StorePath, StorePathError> {
        StorePath::try_from_segments(self)
    }
}

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

impl PartialEq for StorePath {
    fn eq(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self.segments().zip(other.segments()).all(|(a, b)| a == b)
    }
}

impl Eq for StorePath {}

impl std::hash::Hash for StorePath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.segments.len());
        for segment in self.segments() {
            segment.hash(state);
        }
    }
}

impl std::fmt::Debug for StorePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StorePath({:?})", self.as_str())
    }
}

impl std::fmt::Display for StorePath {
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
            let path = StorePath::from_segments(&segments);

            prop_assert_eq!(path.len(), segments.len());
            prop_assert_eq!(
                path.segments().map(|s| s.to_string()).collect::<Vec<_>>(),
                segments.clone()
            );

            let split: Vec<&str> = segments.iter().flat_map(|s| s.split('.')).collect();
            if let Ok(taken_apart) = StorePath::try_from_segments(&split) {
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
            let path = StorePath::from_segments(&segments);
            prop_assert_eq!(StorePath::parse_joined(path.as_str()).unwrap(), path);
        }

        /// The property the whole design rests on: two different sets of levels
        /// never land on the same key. Without it a name holding a separator
        /// could collide with a nesting a caller meant.
        #[test]
        fn different_levels_never_join_to_one_key(a in path_strategy(), b in path_strategy()) {
            let pa = StorePath::from_segments(&a);
            let pb = StorePath::from_segments(&b);

            prop_assert_eq!(a == b, pa.as_str() == pb.as_str());
        }

        /// Prefix matching is over levels, and stripping one is its inverse.
        #[test]
        fn a_prefix_is_stripped_back_off(head in path_strategy(), tail in path_strategy()) {
            let prefix = StorePath::from_segments(&head);
            let full = prefix.join(&StorePath::from_segments(&tail));

            prop_assert!(full.starts_with(&prefix));
            prop_assert_eq!(
                full.strip_prefix(&prefix).unwrap(),
                StorePath::from_segments(&tail)
            );
        }

        /// One level at a time or all at once is the same path, and joining a
        /// single-level path is what pushing means. Equality is over the
        /// levels, so how a path was built never shows.
        #[test]
        fn a_path_does_not_remember_how_it_was_built(segments in path_strategy()) {
            let all_at_once = StorePath::from_segments(&segments);

            let mut one_at_a_time = StorePath::root();
            for segment in &segments {
                one_at_a_time = one_at_a_time.push(segment);
            }

            let by_joining = segments.iter().fold(StorePath::root(), |acc, segment| {
                acc.join(&StorePath::segment(segment))
            });

            prop_assert_eq!(&one_at_a_time, &all_at_once);
            prop_assert_eq!(&by_joining, &all_at_once);
            prop_assert_eq!(one_at_a_time.as_str(), all_at_once.as_str());
        }

        /// Whatever spells the levels, the path is the same one.
        #[test]
        fn any_spelling_of_the_levels_gives_the_same_path(segments in path_strategy()) {
            let built = StorePath::from_segments(&segments);
            let refs: Vec<&str> = segments.iter().map(String::as_str).collect();

            prop_assert_eq!(refs.as_slice().into_store_path().unwrap(), built.clone());
            prop_assert_eq!(segments.clone().into_store_path().unwrap(), built.clone());
            prop_assert_eq!((&built).into_store_path().unwrap(), built.clone());
            prop_assert_eq!(built.clone().into_store_path().unwrap(), built);
        }

        /// The root is under everything, so stripping it is the identity.
        #[test]
        fn every_path_is_under_the_root(segments in path_strategy()) {
            let path = StorePath::from_segments(&segments);
            let root = StorePath::root();

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
                StorePath::try_from_segments(&with_a_hole),
                Err(StorePathError::EmptySegment)
            );
            prop_assert_eq!(
                StorePath::from_segments(&segments).try_push(""),
                Err(StorePathError::EmptySegment)
            );
        }

        /// Any name is a name, however many separators it holds: building
        /// always succeeds, and however many of them the names contain, exactly
        /// one per gap between levels survives unescaped in the joined form.
        #[test]
        fn every_separator_inside_a_name_is_escaped(segments in path_strategy()) {
            let path = StorePath::from_segments(&segments);

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
            let base = StorePath::from_segments(&head);

            let mut grown = head.clone();
            grown.last_mut().unwrap().push_str(&extra);
            let longer = StorePath::from_segments(&grown);

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
            let path = StorePath::from_segments(&a);
            let candidate = StorePath::from_segments(&b);

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
            if let Ok(path) = StorePath::parse_joined(&key) {
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
            let prefix = StorePath::from_segments(&head);
            let boundary = format!("{}{}", prefix.as_str(), SEPARATOR);

            let candidates = [
                prefix.join(&StorePath::from_segments(&tail)),
                StorePath::from_segments(&other),
                prefix.clone(),
                StorePath::from_segments(&tail).join(&prefix),
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
            let pa = StorePath::from_segments(&a);
            let pb = StorePath::from_segments(&b);

            let mut derived = vec![
                pa.join(&pb),
                pa.push("x"),
                StorePath::parse_joined(pa.as_str()).unwrap(),
                pa.join(&StorePath::root()),
            ];
            derived.extend(pa.parent());
            derived.extend(pa.join(&pb).strip_prefix(&pa));

            for path in derived {
                let rebuilt = StorePath::try_from_segments(path.segments())
                    .expect("a derived path holds no empty level");
                prop_assert_eq!(&rebuilt, &path);
                prop_assert_eq!(rebuilt.as_str(), path.as_str());
            }
        }

        /// Equality, hashing and the key never disagree, over paths built every
        /// way there is - including the root and the paths the derived calls
        /// hand back, which the other properties never reach.
        #[test]
        fn equality_hashing_and_the_key_agree(a in path_strategy(), b in path_strategy()) {
            let pa = StorePath::from_segments(&a);
            let pb = StorePath::from_segments(&b);

            let left = [pa.clone(), pa.parent().unwrap_or_else(StorePath::root), StorePath::root()];
            let right = [pb.clone(), pa.join(&pb).strip_prefix(&pa).unwrap(), StorePath::root()];

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
        assert_eq!(StorePath::segment("dark.mode").as_str(), "dark\\.mode");
        assert_eq!(
            StorePath::from_segments(["dark", "mode"]).as_str(),
            "dark.mode"
        );
    }

    #[test]
    fn the_root_is_empty_and_stays_empty() {
        let root = StorePath::root();

        assert!(root.is_root());
        assert_eq!(root.as_str(), "");
        assert_eq!(StorePath::parse_joined("").unwrap(), root);
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
            StorePath::parse_joined("a\\b").ok(),
            StorePath::parse_joined("ab").ok()
        );
        assert_ne!(
            StorePath::parse_joined("a\\").ok(),
            StorePath::parse_joined("a").ok()
        );
    }

    /// The boundary that makes a flat scan safe cannot be spelled at the root.
    /// Every path is under the root, and no key can begin with a separator - a
    /// name that begins with one escapes it - so an engine that derives its
    /// scan bound the same way at every depth scans nothing at the top.
    #[test]
    fn the_root_has_no_separator_boundary() {
        let root = StorePath::root();
        let child = StorePath::from_segments(["ui", "width"]);

        assert!(child.starts_with(&root));
        assert_eq!(format!("{}{}", root.as_str(), SEPARATOR), ".");
        assert!(!child.as_str().starts_with(SEPARATOR));
        assert!(
            !StorePath::segment(".hidden")
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
        let node = StorePath::segment("ui");
        let boundary = format!("{}{}", node.as_str(), SEPARATOR);

        assert!(node.push("width").as_str().starts_with(&boundary));
        assert!(!node.as_str().starts_with(&boundary));
        assert!(node.starts_with(&node));
    }

    /// Escaping covers the separator and the escape and nothing else, so a key
    /// carries whatever else a name held. The sqlite engine builds its scan out
    /// of `key GLOB prefix*`, where every one of these means something other
    /// than itself.
    #[test]
    fn a_key_carries_what_a_glob_pattern_reads() {
        let path = StorePath::segment("a*b[c]?\u{0}d");

        assert_eq!(path.as_str(), "a*b[c]?\u{0}d");
    }

    /// Keys the reader in use today accepts and this one refuses. `.` is the
    /// sentinel `split_path` maps to the whole document, and a trailing
    /// separator is what `join_path` writes for an empty key: both are already
    /// in stores, and neither is a path.
    #[test]
    fn keys_already_written_that_are_not_paths() {
        assert_eq!(
            StorePath::parse_joined("."),
            Err(StorePathError::EmptySegment)
        );
        assert_eq!(
            StorePath::parse_joined("ui."),
            Err(StorePathError::EmptySegment)
        );
        assert_eq!(
            StorePath::parse_joined("ui..width"),
            Err(StorePathError::EmptySegment)
        );
    }

    /// The hash is taken over the levels, not over the key, so the two do not
    /// agree even though equality does. A map keyed by a path cannot be probed
    /// with the string a flat engine already holds - `Borrow<str>` would be
    /// unsound - so every lookup from a stored key has to build a path first.
    #[test]
    fn a_path_does_not_hash_like_its_key() {
        let path = StorePath::segment("ui");

        assert_eq!(path.as_str(), "ui");
        assert_ne!(hash_of(&path), hash_of(&"ui"));
    }

    /// A name is bytes, so two spellings a person and most editors read as one
    /// name are two levels with two keys. Normalising a file on save moves
    /// every value under such a name to a path nothing looks up.
    #[test]
    fn two_spellings_of_one_name_are_two_paths() {
        let precomposed = StorePath::segment("caf\u{e9}");
        let decomposed = StorePath::segment("cafe\u{301}");

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
        let path = StorePath::from_segments(["ui", "window", "width"]);

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
        static UI_WIDTH: StorePath = StorePath::from_static(&["ui", "width"], "ui.width");

        assert_eq!(UI_WIDTH, StorePath::from_segments(["ui", "width"]));
        assert_eq!(UI_WIDTH.as_str(), "ui.width");
        assert_eq!(UI_WIDTH.name(), Some("width"));
        assert!(UI_WIDTH.starts_with(&StorePath::from_static(&["ui"], "ui")));
    }
}
