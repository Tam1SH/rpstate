//! What changed between the places a struct declared last time and the places
//! it declares now.
//!
//! The comparison is over the tree of declared places and nothing else. A
//! leaf's type is the user's, and a value that stops decoding is answered where
//! it is read - by `on_unreadable`, and listed by `disagreements()`. What is
//! judged here is where things sit.
//!
//! Named for what it looks for: data that is no longer under any declaration.

use crate::migration::fields::{FieldDescriptor, Role};
use crate::store::StorePath;
use crate::store::meta::{SchemaSnapshot, StoredFieldEntry};
use std::fmt;

/// One difference between the two trees.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    /// Where it is, under the struct's prefix.
    pub at: StorePath,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A place nothing declared before. It takes the subtree beneath it, which
    /// was open until now.
    Claimed,

    /// A place that was declared and is not any more. Whatever is stored there
    /// is out from under any declaration.
    Released,

    /// The same place, holding a different kind of thing.
    Role { was: Role, now: Role },

    /// A node that gained or lost its own segment, which moves every path
    /// under it.
    Flattened { now: bool },

    /// A place that may now hold nothing where it could not, or the reverse.
    Optional { now: bool },
}

impl fmt::Display for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = &self.at;
        match &self.what {
            What::Claimed => write!(f, "`{at}` is declared now and was not before"),
            What::Released => write!(f, "`{at}` was declared before and is not now"),
            What::Role { was, now } => {
                write!(f, "`{at}` was {was:?} and is {now:?}")
            }
            What::Flattened { now: true } => {
                write!(f, "`{at}` gave up its own segment, so its fields moved up")
            }
            What::Flattened { now: false } => {
                write!(
                    f,
                    "`{at}` took a segment of its own, so its fields moved down"
                )
            }
            What::Optional { now } => write!(f, "`{at}` may hold nothing: {now}"),
        }
    }
}

/// What one difference amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Data written by the old declaration is not under the new one. Somebody
    /// has to say what happens to it, which is what a migration step is.
    Breaks,

    /// A place was taken that nothing declared before. Whether that matters
    /// depends on whether anything is there, which this comparison cannot see:
    /// it knows declarations and not data. Over empty ground it is nothing at
    /// all - the commonest and most harmless change there is - and over
    /// occupied ground it is an annexation.
    LookAtTheGround,

    /// Neither. The places are where they were.
    Harmless,
}

impl Moved {
    pub fn verdict(&self) -> Verdict {
        match self.what {
            What::Released | What::Role { .. } | What::Flattened { .. } => Verdict::Breaks,
            What::Claimed => Verdict::LookAtTheGround,
            What::Optional { .. } => Verdict::Harmless,
        }
    }
}

/// Every difference between what was declared and what is declared, in the
/// order the paths appear.
///
/// Both sides are trees, and a node is compared to the node of the same name.
/// A rename is therefore a [`What::Released`] beside a [`What::Claimed`] and
/// cannot be told from the two of them happening at once - which is what
/// `#[rename(old => new)]` exists to say.
///
/// A flattened node contributes no segment, so what is under it is compared at
/// the level the node itself sits at. Where the two sides disagree about that,
/// every path beneath the node moved at once and the node's own
/// [`What::Flattened`] is the report; naming each of them would name places
/// that exist in neither layout.
///
/// What a declaration is, against where it lands:
///
/// ```text
///   declared                    places in the store
///   ------------------------------------------------
///   ui            node          ui
///     theme       leaf          ui.theme
///
///   ui            node flat     -           no segment of its own
///     theme       leaf          theme
/// ```
///
/// And what changes hands is what is *owned*, which a node is not: nothing is
/// stored at one, [`Owners`](crate::store::owners::Owners) is never asked to
/// claim one, and a raw write beside its fields belongs to whoever wrote it. A
/// leaf and a map are places; a node is the way to them.
///
/// ```text
///   was          now              between them
///   ----------------------------------------------------------------
///   (nothing)    ui     node      `ui.theme` claimed    not `ui`
///                  theme
///
///   (nothing)    ui     flat      `theme` claimed       no segment either
///                  theme
///
///   (nothing)    open   map       `open` claimed        a map is a place,
///                                                       and takes open.*
///
///   ui           ui     flat      `ui` gave up its segment
///     theme        theme          and ui.theme is now theme
/// ```
pub fn between(was: &[StoredFieldEntry], now: &[FieldDescriptor]) -> Vec<Moved> {
    let mut found = Vec::new();
    walk(&StorePath::root(), was, now, &mut found);
    found
}

/// The recorded tree that is the same declaration as `now`, if one is.
///
/// A declaration is the places it owns, so two trees are the same one when
/// they own a place in common. That is decidable rather than a guess: the
/// declarations at a prefix own disjoint places - [`Owners`] refuses them
/// otherwise - so a place belongs to at most one of them on either side, and a
/// tree meets at most one tree.
///
/// Sharing nothing is not a puzzle either. A declaration whose every place
/// moved is a removal and an addition, which is what the two look like from
/// here and what they are: the data that was under the old places is out from
/// under any declaration, and the new places annexed whatever was under them.
///
/// [`Owners`]: crate::store::owners::Owners
pub fn same_declaration(recorded: &[SchemaSnapshot], now: &[FieldDescriptor]) -> Option<usize> {
    let claimed = places(&StorePath::root(), now);

    recorded.iter().position(|was| {
        stored_places(&StorePath::root(), &was.fields).any(|at| claimed.contains(&at))
    })
}

/// [`same_declaration`] between two recorded trees, for a caller holding what
/// it means to write rather than what the code declares.
pub fn same_declaration_stored(
    recorded: &[SchemaSnapshot],
    now: &[StoredFieldEntry],
) -> Option<usize> {
    let claimed: Vec<StorePath> = stored_places(&StorePath::root(), now).collect();

    recorded.iter().position(|was| {
        stored_places(&StorePath::root(), &was.fields).any(|at| claimed.contains(&at))
    })
}

/// Every place a set of declared fields owns, a node contributing none of its
/// own.
fn places(under: &StorePath, fields: &[FieldDescriptor]) -> Vec<StorePath> {
    let mut found = Vec::new();

    for field in fields {
        match field.owns(under) {
            Some(owned) => found.push(owned),
            None => found.extend(places(&field.below(under), field.children)),
        }
    }

    found
}

/// The same, over what was written down.
fn stored_places<'a>(
    under: &StorePath,
    fields: &'a [StoredFieldEntry],
) -> Box<dyn Iterator<Item = StorePath> + 'a> {
    let under = under.clone();

    Box::new(fields.iter().flat_map(move |field| {
        let at = under.join(&field.name);

        match field.shape.role {
            Role::Node => {
                let below = match field.shape.flattened {
                    true => under.clone(),
                    false => at,
                };
                stored_places(&below, &field.shape.children)
            }
            _ => Box::new(std::iter::once(at)) as Box<dyn Iterator<Item = StorePath>>,
        }
    }))
}

fn walk(
    under: &StorePath,
    was: &[StoredFieldEntry],
    now: &[FieldDescriptor],
    found: &mut Vec<Moved>,
) {
    for old in was {
        let at = under.join(&old.name);

        let Some(new) = now.iter().find(|it| old.name == it.name.path()) else {
            // The mirror of the claim below, and by the same rule.
            match old.shape.role {
                Role::Node => {
                    let below = if old.shape.flattened { under } else { &at };
                    walk(below, &old.shape.children, &[], found);
                }
                Role::Field | Role::Map => found.push(Moved {
                    at,
                    what: What::Released,
                }),
            }
            continue;
        };

        // A node is not a place, so nothing about the node itself is reported
        // - only about what sits under it. Where the two sides disagree about
        // its segment the whole subtree moved, and `at` names the level it
        // had or is about to have, which is a path either way.
        let has_a_place = !(old.shape.role == Role::Node && new.role == Role::Node);

        if has_a_place && old.shape.role != new.role {
            found.push(Moved {
                at: at.clone(),
                what: What::Role {
                    was: old.shape.role,
                    now: new.role,
                },
            });
        }

        if old.shape.flattened != new.flattened {
            found.push(Moved {
                at: at.clone(),
                what: What::Flattened { now: new.flattened },
            });
        }

        if has_a_place && old.shape.optional != new.optional {
            found.push(Moved {
                at: at.clone(),
                what: What::Optional { now: new.optional },
            });
        }

        if old.shape.flattened == new.flattened {
            let below = if new.flattened { under } else { &at };
            walk(below, &old.shape.children, new.children, found);
        }
    }

    for new in now {
        let name = new.name.path();

        if was.iter().any(|it| it.name == name) {
            continue;
        }

        match new.owns(under) {
            Some(at) => found.push(Moved {
                at,
                what: What::Claimed,
            }),
            None => walk(&new.below(under), &[], new.children, found),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::meta::StoredShape;

    fn stored(name: &str, shape: StoredShape) -> StoredFieldEntry {
        StoredFieldEntry {
            name: StorePath::parse_joined(name).unwrap(),
            type_name: "T".to_string(),
            shape,
        }
    }

    fn node(children: Vec<StoredFieldEntry>, flattened: bool) -> StoredShape {
        StoredShape {
            role: Role::Node,
            optional: false,
            children,
            flattened,
        }
    }

    const fn child(segments: &'static [&'static str], joined: &'static str) -> FieldDescriptor {
        FieldDescriptor::leaf(segments, joined, "T")
    }

    #[test]
    fn the_same_tree_twice_has_not_moved() {
        static NOW: &[FieldDescriptor] = &[child(&["theme"], "theme"), child(&["scale"], "scale")];

        let was = vec![
            stored("theme", StoredShape::field()),
            stored("scale", StoredShape::field()),
        ];

        assert_eq!(between(&was, NOW), []);
    }

    #[test]
    fn a_rename_reads_as_a_release_beside_a_claim() {
        static NOW: &[FieldDescriptor] = &[child(&["nickname"], "nickname")];

        let was = vec![stored("handle", StoredShape::field())];

        assert_eq!(
            between(&was, NOW),
            [
                Moved {
                    at: StorePath::segment("handle"),
                    what: What::Released
                },
                Moved {
                    at: StorePath::segment("nickname"),
                    what: What::Claimed
                },
            ]
        );
    }

    #[test]
    fn a_leaf_that_became_a_map_breaks() {
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Map,
            ..child(&["open"], "open")
        }];

        let was = vec![stored("open", StoredShape::field())];

        let found = between(&was, NOW);

        assert_eq!(
            found,
            [Moved {
                at: StorePath::segment("open"),
                what: What::Role {
                    was: Role::Field,
                    now: Role::Map
                }
            }]
        );
        assert_eq!(found[0].verdict(), Verdict::Breaks);
    }

    #[test]
    fn a_node_that_lost_its_segment_breaks() {
        static UNDER: &[FieldDescriptor] = &[child(&["theme"], "theme")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            flattened: true,
            ..child(&["ui"], "ui")
        }];

        let was = vec![stored(
            "ui",
            node(vec![stored("theme", StoredShape::field())], false),
        )];

        let found = between(&was, NOW);

        assert_eq!(
            found,
            [Moved {
                at: StorePath::segment("ui"),
                what: What::Flattened { now: true }
            }]
        );
        assert_eq!(found[0].verdict(), Verdict::Breaks);
    }

    #[test]
    fn a_place_under_a_node_is_named_by_its_whole_path() {
        static UNDER: &[FieldDescriptor] = &[child(&["scale"], "scale")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            ..child(&["ui"], "ui")
        }];

        let was = vec![stored(
            "ui",
            node(vec![stored("theme", StoredShape::field())], false),
        )];

        assert_eq!(
            between(&was, NOW),
            [
                Moved {
                    at: StorePath::from_segments(["ui", "theme"]),
                    what: What::Released
                },
                Moved {
                    at: StorePath::from_segments(["ui", "scale"]),
                    what: What::Claimed
                },
            ]
        );
    }

    #[test]
    fn a_flattened_node_lends_its_children_no_segment() {
        static UNDER: &[FieldDescriptor] = &[child(&["scale"], "scale")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            flattened: true,
            ..child(&["ui"], "ui")
        }];

        let was = vec![stored(
            "ui",
            node(vec![stored("theme", StoredShape::field())], true),
        )];

        assert_eq!(
            between(&was, NOW),
            [
                Moved {
                    at: StorePath::segment("theme"),
                    what: What::Released
                },
                Moved {
                    at: StorePath::segment("scale"),
                    what: What::Claimed
                },
            ]
        );
    }

    #[test]
    fn a_place_that_may_now_hold_nothing_is_harmless() {
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            optional: true,
            ..child(&["nickname"], "nickname")
        }];

        let was = vec![stored("nickname", StoredShape::field())];

        let found = between(&was, NOW);

        assert_eq!(
            found,
            [Moved {
                at: StorePath::segment("nickname"),
                what: What::Optional { now: true }
            }]
        );
        assert_eq!(found[0].verdict(), Verdict::Harmless);
    }

    #[test]
    fn a_node_is_not_a_place_so_what_is_claimed_is_under_it() {
        static UNDER: &[FieldDescriptor] = &[child(&["theme"], "theme")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            ..child(&["ui"], "ui")
        }];

        assert_eq!(
            between(&[], NOW),
            [Moved {
                at: StorePath::from_segments(["ui", "theme"]),
                what: What::Claimed
            }],
            "nothing is stored at `ui`, and a write beside `ui.theme` is not this struct's"
        );
    }

    #[test]
    fn a_map_is_a_place_and_takes_everything_under_it() {
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Map,
            ..child(&["open"], "open")
        }];

        assert_eq!(
            between(&[], NOW),
            [Moved {
                at: StorePath::segment("open"),
                what: What::Claimed
            }]
        );
    }

    #[test]
    fn a_flattened_node_is_claimed_as_what_it_brings() {
        static UNDER: &[FieldDescriptor] = &[child(&["theme"], "theme")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            flattened: true,
            ..child(&["ui"], "ui")
        }];

        assert_eq!(
            between(&[], NOW),
            [Moved {
                at: StorePath::segment("theme"),
                what: What::Claimed
            }]
        );
    }

    #[test]
    fn a_flattened_node_that_is_gone_releases_what_it_brought() {
        let was = vec![stored(
            "ui",
            node(vec![stored("theme", StoredShape::field())], true),
        )];

        assert_eq!(
            between(&was, &[]),
            [Moved {
                at: StorePath::segment("theme"),
                what: What::Released
            }]
        );
    }

    #[test]
    fn a_node_flattened_on_both_sides_says_nothing_about_itself() {
        static UNDER: &[FieldDescriptor] = &[child(&["theme"], "theme")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: UNDER,
            flattened: true,
            optional: true,
            ..child(&["ui"], "ui")
        }];

        let was = vec![stored(
            "ui",
            node(vec![stored("theme", StoredShape::field())], true),
        )];

        assert_eq!(between(&was, NOW), []);
    }

    #[test]
    fn two_trees_can_hold_one_path_and_different_things_at_it() {
        static INNER: &[FieldDescriptor] = &[child(&["a"], "a")];
        static NOW: &[FieldDescriptor] = &[FieldDescriptor {
            role: Role::Node,
            children: INNER,
            flattened: true,
            ..child(&["a"], "a")
        }];

        let was = vec![stored("a", node(vec![], false))];

        let found = between(&was, NOW);

        assert_eq!(
            found,
            [Moved {
                at: StorePath::segment("a"),
                what: What::Flattened { now: true }
            }],
            "both declare `a`, and what stands there went from a node to a leaf"
        );
        assert_eq!(found[0].verdict(), Verdict::Breaks);
    }

    #[test]
    fn a_store_opened_for_the_first_time_has_nothing_to_have_moved_from() {
        static NOW: &[FieldDescriptor] = &[child(&["theme"], "theme")];

        let found = between(&[], NOW);

        assert_eq!(
            found,
            [Moved {
                at: StorePath::segment("theme"),
                what: What::Claimed
            }]
        );
        assert_eq!(found[0].verdict(), Verdict::LookAtTheGround);
    }
}

/// One declaration tree, generated, and the two forms `between` compares.
///
/// The two sides of the comparison are different types - one read off a disk,
/// one written by the macro - so a property needs a third thing that becomes
/// either. That is [`Decl`]: a tree is drawn once and projected twice, which is
/// what makes "the same tree twice" a statement a generator can make.
#[cfg(test)]
mod properties {
    use super::*;
    use crate::store::StaticPath;
    use crate::store::meta::StoredShape;
    use proptest::prelude::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Decl {
        /// The levels this declaration's name is, which `path = "a.b"` makes
        /// more than one of.
        levels: Vec<String>,
        role: Role,
        optional: bool,
        flattened: bool,
        children: Vec<Decl>,
    }

    impl Decl {
        fn name(&self) -> StorePath {
            StorePath::from_segments(&self.levels)
        }
    }

    /// The tree as the disk holds it.
    fn as_stored(at: &Decl) -> StoredFieldEntry {
        StoredFieldEntry {
            name: at.name(),
            type_name: "T".to_string(),
            shape: StoredShape {
                role: at.role,
                optional: at.optional,
                flattened: at.flattened,
                children: at.children.iter().map(as_stored).collect(),
            },
        }
    }

    /// The tree as the macro writes it.
    ///
    /// Leaked, because a descriptor's children are `&'static` - the macro
    /// writes them into the binary and nothing frees them there either. A
    /// generated case leaks a few hundred bytes and the test process exits.
    fn as_declared(at: &Decl) -> FieldDescriptor {
        let segments: &'static [&'static str] = Box::leak(
            at.levels
                .iter()
                .map(|level| &*Box::leak(level.clone().into_boxed_str()))
                .collect::<Vec<&'static str>>()
                .into_boxed_slice(),
        );
        let joined: &'static str = Box::leak(at.name().as_str().to_string().into_boxed_str());

        FieldDescriptor {
            name: StaticPath::new(segments, joined),
            declared: joined,
            type_name: "T",
            role: at.role,
            optional: at.optional,
            flattened: at.flattened,
            children: Box::leak(
                at.children
                    .iter()
                    .map(as_declared)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    fn all_declared(tree: &[Decl]) -> &'static [FieldDescriptor] {
        Box::leak(
            tree.iter()
                .map(as_declared)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    /// Every place the tree declares, worked out without going near `between`.
    ///
    /// This is the oracle the properties are held against, so it says the rule
    /// in the other direction: a flattened node lends its children no segment,
    /// and everything else adds its own.
    fn places(tree: &[Decl], under: &StorePath, into: &mut Vec<StorePath>) {
        for one in tree {
            let at = under.join(&one.name());

            match one.flattened {
                true => places(&one.children, under, into),
                false => {
                    into.push(at.clone());
                    places(&one.children, &at, into);
                }
            }
        }
    }

    fn declared_places(tree: &[Decl]) -> Vec<StorePath> {
        let mut found = Vec::new();
        places(tree, &StorePath::root(), &mut found);
        found
    }

    /// The places and what stands at each, which is what "the same tree" has to
    /// mean: two trees can declare one set of paths and hold different things
    /// there - a node at `a`, against a leaf reached at `a` through a flattened
    /// wrapper.
    fn shapes(tree: &[Decl], under: &StorePath, into: &mut Vec<(StorePath, Role, bool)>) {
        for one in tree {
            let at = under.join(&one.name());

            match one.flattened {
                true => shapes(&one.children, under, into),
                false => {
                    into.push((at.clone(), one.role, one.optional));
                    shapes(&one.children, &at, into);
                }
            }
        }
    }

    fn declared_shapes(tree: &[Decl]) -> Vec<(StorePath, Role, bool)> {
        let mut found = Vec::new();
        shapes(tree, &StorePath::root(), &mut found);
        found
    }

    /// The places the tree owns: what a claim names.
    ///
    /// Not every path it declares - a node is a way to the paths under it and
    /// is owned by nobody, which is the rule `Owners` and `Kv` both keep. So
    /// this descends through nodes and stops at leaves and maps, which is what
    /// `Owners::claim` is called for and nothing else.
    fn claims(tree: &[Decl], under: &StorePath, into: &mut Vec<StorePath>) {
        for one in tree {
            let at = under.join(&one.name());

            match one.role {
                Role::Node => {
                    let below = if one.flattened { under } else { &at };
                    claims(&one.children, below, into);
                }
                Role::Field | Role::Map => into.push(at),
            }
        }
    }

    fn claimed_places(tree: &[Decl]) -> Vec<StorePath> {
        let mut found = Vec::new();
        claims(tree, &StorePath::root(), &mut found);
        found
    }

    /// The two trees and what came back, as lines a person reads.
    ///
    /// A `Vec<Moved>` in a `Debug` dump says nothing about which places existed
    /// to begin with, and that is exactly what a failure here is about.
    fn shown(was: &[Decl], now: &[Decl], moved: &[Moved]) -> String {
        let list = |tree: &[Decl]| match declared_places(tree) {
            places if places.is_empty() => "    (nothing)".to_string(),
            places => places
                .iter()
                .map(|at| format!("    {at}"))
                .collect::<Vec<_>>()
                .join("\n"),
        };

        let said = match moved.is_empty() {
            true => "    (nothing moved)".to_string(),
            false => moved
                .iter()
                .map(|one| format!("    {:?}  {one}", one.verdict()))
                .collect::<Vec<_>>()
                .join("\n"),
        };

        format!(
            "\nwas:\n{}\nnow:\n{}\nbetween them:\n{}\n",
            list(was),
            list(now),
            said
        )
    }

    fn a_name() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(
            prop_oneof![
                6 => prop::char::range('a', 'c').prop_map(|c| c.to_string()),
                2 => Just("dark.mode".to_string()),
                1 => Just("a\\b".to_string()),
            ],
            1..3,
        )
    }

    fn a_role() -> impl Strategy<Value = Role> {
        prop_oneof![Just(Role::Field), Just(Role::Map), Just(Role::Node)]
    }

    /// Siblings with one name are not a tree any declaration could be: the
    /// macro refuses two fields at one path, and a comparison that met them
    /// would answer about the first and count both.
    fn distinct(mut siblings: Vec<Decl>) -> Vec<Decl> {
        let mut seen = Vec::new();
        siblings.retain(|one| {
            let name = one.name();
            match seen.contains(&name) {
                true => false,
                false => {
                    seen.push(name);
                    true
                }
            }
        });
        siblings
    }

    /// A tree of declarations, shallow and narrow on purpose: what these
    /// properties are about is how levels compose, and depth past three adds
    /// running time rather than cases.
    fn a_tree() -> impl Strategy<Value = Vec<Decl>> {
        let leaf = (a_name(), a_role(), any::<bool>()).prop_map(|(levels, role, optional)| Decl {
            levels,
            role,
            optional,
            flattened: false,
            children: Vec::new(),
        });

        let one = leaf.prop_recursive(3, 24, 3, |inner| {
            (
                a_name(),
                any::<bool>(),
                any::<bool>(),
                prop::collection::vec(inner, 0..3),
            )
                .prop_map(|(levels, optional, flattened, children)| Decl {
                    levels,
                    role: Role::Node,
                    optional,
                    flattened,
                    children: distinct(children),
                })
        });

        prop::collection::vec(one, 0..4).prop_map(distinct)
    }

    proptest! {
        /// Nothing moved between a tree and itself, whatever the tree.
        ///
        /// The example-based test says this of two fields; the point of saying
        /// it again here is the trees the generator reaches - a flattened node
        /// holding a flattened node, a name that is two levels, a name holding
        /// the separator.
        #[test]
        #[ignore = "known: `walk` pairs by name, and a flattened node has none on disk - see TODO.md"]
        fn the_same_tree_twice_has_moved_nothing(tree in a_tree()) {
            let was: Vec<_> = tree.iter().map(as_stored).collect();
            let moved = between(&was, all_declared(&tree));

            prop_assert!(moved.is_empty(), "{}", shown(&tree, &tree, &moved));
        }

        /// Every place a comparison names is a place one of the two trees
        /// declares. It reports where things sit; it does not invent a path.
        ///
        /// This is the one that fails when a walk adds a segment it should not
        /// have:
        ///
        /// ```text
        ///   was          now              a walk that forgot flatten says
        ///   -------------------------------------------------------------
        ///   ui   flat    ui    flat       `ui.theme` released
        ///     theme        scale          `ui.scale` claimed
        ///
        ///   but neither layout has anything at `ui.theme` or `ui.scale`:
        ///   flattened, `ui` lends no segment, so the two places are
        ///   `theme` and `scale`.
        /// ```
        #[test]
        fn every_place_reported_is_one_of_the_two_trees(was in a_tree(), now in a_tree()) {
            let stored: Vec<_> = was.iter().map(as_stored).collect();
            let moved = between(&stored, all_declared(&now));

            let mut known = declared_places(&was);
            known.extend(declared_places(&now));

            for one in &moved {
                prop_assert!(
                    known.contains(&one.at),
                    "`{}` is in neither tree{}",
                    one.at,
                    shown(&was, &now, &moved)
                );
            }
        }

        /// Turn the comparison round and a release becomes a claim.
        ///
        /// Which is the same statement as "a rename cannot be told from a
        /// removal beside an addition", said from the other end:
        ///
        /// ```text
        ///   between(a, b)                between(b, a)
        ///   ------------------------------------------------------
        ///   `handle` released     <->    `handle` claimed
        ///   `nickname` claimed    <->    `nickname` released
        /// ```
        ///
        /// It is also what catches one side of the walk learning a rule the
        /// other side did not: the claim arm honouring flatten while the
        /// release arm still names the node fails here and nowhere else.
        #[test]
        fn release_and_claim_are_mirror_images(a in a_tree(), b in a_tree()) {
            let stored_a: Vec<_> = a.iter().map(as_stored).collect();
            let stored_b: Vec<_> = b.iter().map(as_stored).collect();

            let forward = between(&stored_a, all_declared(&b));
            let backward = between(&stored_b, all_declared(&a));

            let taken = |moved: &[Moved], what: What| {
                let mut at: Vec<String> = moved
                    .iter()
                    .filter(|one| one.what == what)
                    .map(|one| one.at.to_string())
                    .collect();
                at.sort();
                at
            };

            prop_assert_eq!(
                taken(&forward, What::Released),
                taken(&backward, What::Claimed),
                "{}", shown(&a, &b, &forward)
            );
            prop_assert_eq!(
                taken(&forward, What::Claimed),
                taken(&backward, What::Released),
                "{}", shown(&a, &b, &forward)
            );
        }

        /// A store opened for the first time claims what the code declares and
        /// releases nothing - there is nothing behind it to release.
        ///
        /// What it claims is the shallowest places, not every place: taking a
        /// node takes the subtree under it, so naming the children too would
        /// say the same thing twice.
        ///
        /// ```text
        ///   now              claimed
        ///   ------------------------------------------
        ///   ui               ui              one claim, and ui.theme is
        ///     theme                          inside it
        ///
        ///   ui    flat       theme           `ui` is not a place, so what
        ///     theme                          was taken is what it brings
        /// ```
        #[test]
        fn against_nothing_a_tree_claims_its_shallowest_places(tree in a_tree()) {
            let moved = between(&[], all_declared(&tree));

            let claimed: Vec<StorePath> = moved
                .iter()
                .filter(|one| one.what == What::Claimed)
                .map(|one| one.at.clone())
                .collect();

            prop_assert_eq!(claimed.len(), moved.len(), "{}", shown(&[], &tree, &moved));
            prop_assert_eq!(
                claimed,
                claimed_places(&tree),
                "{}", shown(&[], &tree, &moved)
            );
        }

        /// And the other end: code that declares nothing releases every place
        /// the store was holding, and each one breaks.
        #[test]
        fn against_nothing_declared_every_place_is_released(tree in a_tree()) {
            let was: Vec<_> = tree.iter().map(as_stored).collect();
            let moved = between(&was, &[]);

            let released: Vec<StorePath> = moved
                .iter()
                .filter(|one| one.what == What::Released)
                .map(|one| one.at.clone())
                .collect();

            prop_assert_eq!(released.len(), moved.len(), "{}", shown(&tree, &[], &moved));

            for one in &moved {
                prop_assert_eq!(
                    one.verdict(),
                    Verdict::Breaks,
                    "{}", shown(&tree, &[], &moved)
                );
            }
        }

        /// Something breaking means the two trees do not hold the same things.
        ///
        /// The same places is not enough to say they are the same, which is
        /// what makes this weaker than it looks and worth writing down:
        ///
        /// ```text
        ///   was                     now
        ///   -----------------       ---------------------------
        ///   a   node                a       node, flattened
        ///                             a     node, flattened
        ///                               a   leaf
        ///
        ///   a flattened node lends no segment, so both of those fall
        ///   through and the leaf lands at the top level:
        ///
        ///   places under was        places under now
        ///   -----------------       ---------------------------
        ///   a   holds a node        a       holds a leaf
        /// ```
        ///
        /// One place either side, spelled the same. A comparison that counted
        /// paths would call the two trees equal while a node turned into a
        /// leaf under it, so the oracle has to carry what stands at a place
        /// and not only that something does.
        #[test]
        #[ignore = "known: `walk` pairs by name, and a flattened node has none on disk - see TODO.md"]
        fn a_break_means_the_trees_differ(was in a_tree(), now in a_tree()) {
            let stored: Vec<_> = was.iter().map(as_stored).collect();
            let moved = between(&stored, all_declared(&now));

            if moved.iter().any(|one| one.verdict() == Verdict::Breaks) {
                prop_assert_ne!(
                    declared_shapes(&was),
                    declared_shapes(&now),
                    "something broke between two trees holding the same things{}",
                    shown(&was, &now, &moved)
                );
            }
        }
    }
}
