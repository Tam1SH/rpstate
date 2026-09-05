use crate::store::error::{StorageError, StorageResult};
use amethystate_core::path::StorePath;
use error_stack::Report;
use parking_lot::RwLock;
use std::fmt;

/// A stored path and the schema that claimed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    pub path: StorePath,
    pub by: &'static str,
}

impl fmt::Display for Claimed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} claims {}", self.by, self.path)
    }
}

/// Who owns what, so that two owners cannot write over each other.
///
/// A claim is made where a path is composed - the constructor of a field or a
/// map - and belongs to the *name* that made it. So the same schema claiming
/// the same path again is a no-op, reconstructing a struct in one process
/// works, and a claim stands for the life of the process.
#[derive(Default)]
pub struct Owners {
    /// Sorted by path, always.
    ///
    /// Nothing sorts this: it starts empty and the one insertion goes to the
    /// position [`Vec::partition_point`] names, which is the sorted one. Every
    /// lookup below assumes it - two binary searches and a run - so an
    /// insertion anywhere else would not be slower, it would be wrong.
    claims: RwLock<Vec<Claimed>>,
}

fn refused(standing: &Claimed, path: &StorePath, by: &'static str) -> Report<StorageError> {
    Report::new(StorageError::Claimed)
        .attach(standing.clone())
        .attach(Claimed {
            path: path.clone(),
            by,
        })
}

impl Owners {
    /// Records that `by` owns `path` and everything under it, or refuses.
    ///
    /// Two places meet when one holds the other, so a standing claim can meet
    /// this one in exactly two ways, and the search is those two:
    ///
    /// ```text
    ///   claims, sorted            claiming `pot.ato`
    ///   ------------------------------------------------------------
    ///   lid                       before it, and cannot hold it
    ///   pot                       ABOVE - looked up, by name
    ///   pot!luck                  sorts between, holds nothing here
    ///   pot.ato                   the place itself
    ///   pot.ato.skin              BELOW - found by walking the run
    ///   potato                    past the run, and cannot be under it
    /// ```
    ///
    /// Upwards is a lookup per ancestor rather than a walk backwards, because
    /// the keys between an ancestor and this path are not all ancestors -
    /// `pot!luck` sits between `pot` and `pot.ato` - so a walk that stopped at
    /// the first key that was not one would stop before reaching `pot`.
    ///
    /// Downwards is a walk, because everything under this path sorts after it
    /// and contiguously. [`Subtree::may_still_reach`] says where the run ends,
    /// and it is deliberately wider than the subtree: `overlaps` separates a
    /// descendant from a name that merely sorts inside.
    ///
    /// [`Subtree::may_still_reach`]: amethystate_core::path::Subtree::may_still_reach
    pub fn claim(&self, path: &StorePath, by: &'static str) -> StorageResult<()> {
        let mut claims = self.claims.write();

        // A claim that holds `path` sits at `path` itself or at one of its
        // ancestors, since containment is a prefix at a level boundary. They
        // are looked up rather than scanned back to, because the run between an
        // ancestor and `path` is not all ancestors: `ui!x` sorts between `ui`
        // and `ui.theme`, so a walk that stops at the first non-prefix stops
        // before it reaches `ui`.
        let mut above = Some(path.clone());
        while let Some(one) = above {
            if let Ok(found) = claims.binary_search_by(|c| c.path.cmp(&one)) {
                let other = &claims[found];
                if other.by != by {
                    return Err(refused(other, path, by));
                }
                if other.path == *path {
                    return Ok(());
                }
            }
            above = one.parent();
        }

        // Downwards it is a run, and `may_still_reach` says where it ends -
        // wider than the subtree, because names sort between a path and its
        // children. `overlaps` then separates a descendant from one of those.
        let subtree = path.subtree();
        let at = claims.partition_point(|c| c.path < *path);
        for other in claims[at..]
            .iter()
            .take_while(|c| subtree.may_still_reach(&c.path))
        {
            if other.by != by && path.overlaps(&other.path) {
                return Err(refused(other, path, by));
            }
        }

        claims.insert(
            at,
            Claimed {
                path: path.clone(),
                by,
            },
        );
        Ok(())
    }

    /// The schema that claimed `path`, for a report or the inspector.
    pub fn declared_by(&self, path: &StorePath) -> Option<&'static str> {
        let claims = self.claims.read();
        claims
            .binary_search_by(|c| c.path.cmp(path))
            .ok()
            .map(|at| claims[at].by)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(joined: &str) -> StorePath {
        StorePath::parse_joined(joined).unwrap()
    }

    #[test]
    fn one_schema_claiming_the_same_path_twice_is_the_same_claim() {
        let owners = Owners::default();

        owners.claim(&path("ui.theme"), "Ui").unwrap();
        owners.claim(&path("ui.theme"), "Ui").unwrap();

        assert_eq!(
            owners.claims.read().len(),
            1,
            "reconstructing a struct must not pile up claims"
        );
    }

    #[test]
    fn two_schemas_cannot_claim_one_path() {
        let owners = Owners::default();
        owners.claim(&path("ui.theme"), "Ui").unwrap();

        let refused = owners.claim(&path("ui.theme"), "Theme").unwrap_err();

        let named: Vec<&Claimed> = amethystate_core::facts::all(&refused).collect();
        assert_eq!(named.len(), 2, "the report names both: {refused:?}");
    }

    #[test]
    fn a_claim_covers_what_is_under_it() {
        let owners = Owners::default();
        owners.claim(&path("widths"), "Columns").unwrap();

        assert!(
            owners.claim(&path("widths.left"), "Panels").is_err(),
            "a map owns its entries, so nobody else may put one there"
        );
    }

    #[test]
    fn a_claim_is_refused_by_one_already_inside_it() {
        let owners = Owners::default();
        owners.claim(&path("ui.panels.left"), "Panels").unwrap();

        assert!(
            owners.claim(&path("ui.panels"), "Ui").is_err(),
            "the outer one would take the level the inner one lives on"
        );
    }

    #[test]
    fn a_level_may_be_shared_when_the_claims_are_not() {
        let owners = Owners::default();

        owners.claim(&path("ui.accent"), "UiColors").unwrap();
        owners
            .claim(&path("ui.density"), "UiLayout")
            .expect("two schemas may sit on one level while owning different keys");
    }

    #[test]
    fn a_sibling_that_sorts_inside_the_run_does_not_hide_an_ancestor() {
        let owners = Owners::default();

        owners.claim(&path("ui"), "Ui").unwrap();
        owners.claim(&path("ui!x"), "Other").unwrap();

        assert!(
            owners.claim(&path("ui.theme"), "Theme").is_err(),
            "`ui` holds `ui.theme`, and `ui!x` sitting between them must not hide it"
        );
    }

    #[test]
    fn a_string_prefix_is_not_a_claim() {
        let owners = Owners::default();
        owners.claim(&path("ui"), "Ui").unwrap();

        owners
            .claim(&path("uix.width"), "Uix")
            .expect("`ui` does not hold `uix.width`");
    }

    #[test]
    fn a_claim_names_who_made_it() {
        let owners = Owners::default();
        owners.claim(&path("ui.theme"), "Ui").unwrap();

        assert_eq!(owners.declared_by(&path("ui.theme")), Some("Ui"));
        assert_eq!(owners.declared_by(&path("ui.other")), None);
    }
}

/// What counts as owning a place, held against the rule rather than against
/// examples.
///
/// [`Owners::claim`] does not walk its claims. It binary-searches the
/// ancestors and takes a contiguous run downwards, which is what makes it
/// cheap and what makes it possible to be subtly wrong: the run is over the
/// *string*, and the strings between two paths are not all under the shorter
/// one.
///
/// ```text
///   claims, sorted by key      a run downwards from `pot`
///   -------------------------------------------------------
///   pot                        starts here
///   pot!luck                   in the run, NOT under `pot`
///   pot.ato                    in the run, under `pot`
///   potato                     out of the run
/// ```
///
/// So the properties below hold the fast answer against the slow one: is
/// there a standing claim, by somebody else, whose subtree meets this one.
#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    /// Names that make the shortcut and the answer disagree.
    ///
    /// Whether one place holds another is [`StorePath::overlaps`]'s to say,
    /// and it is held to that by its own properties. What is under test here
    /// is that `claim` never concludes anything from the string bounds it
    /// searches by - so every name below lands inside a bound and outside the
    /// answer, or the other way round:
    ///
    /// ```text
    ///   pot         holds pot.ato
    ///   pot!luck    sorts between them, so a walk downwards passes it
    ///   pot.ato     `pot` and `ato`, two levels
    ///   potato      inside `starts_with("pot")`, and `pot` does not hold it
    /// ```
    fn a_level() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("pot"),
            Just("potato"),
            Just("pot!luck"),
            Just("ato"),
            Just("lid"),
        ]
    }

    fn a_path() -> impl Strategy<Value = StorePath> {
        prop::collection::vec(a_level(), 1..4).prop_map(StorePath::from_segments)
    }

    fn an_owner() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("A"), Just("B")]
    }

    /// The rule itself, walked: a claim meets a standing one when they are the
    /// same place or one holds the other, and only another owner's claim
    /// refuses.
    fn met_by<'a>(standing: &'a [Claimed], path: &StorePath, by: &str) -> Option<&'a Claimed> {
        standing
            .iter()
            .find(|other| other.by != by && path.overlaps(&other.path))
    }

    /// What the claims would be after taking each in turn, by the rule.
    fn taken_by_the_rule(claims: &[(StorePath, &'static str)]) -> Vec<Claimed> {
        let mut standing: Vec<Claimed> = Vec::new();

        for (path, by) in claims {
            if met_by(&standing, path, by).is_some() {
                continue;
            }
            if standing.iter().any(|c| c.path == *path && c.by == *by) {
                continue;
            }
            standing.push(Claimed {
                path: path.clone(),
                by,
            });
        }

        standing.sort_by(|a, b| a.path.cmp(&b.path));
        standing
    }

    proptest! {
        /// A claim is refused exactly when the rule says it meets another
        /// owner's, and the shortcut has to agree with the walk on every case.
        ///
        /// This is where the sorted run earns its keep or does not: a claim
        /// hidden behind `ui!x` is refused by the walk, and a search that
        /// stopped at the first key not starting with `ui` would take it.
        #[test]
        fn a_claim_is_refused_exactly_when_it_meets_another_owners(
            claims in prop::collection::vec((a_path(), an_owner()), 0..8)
        ) {
            let owners = Owners::default();
            let mut standing: Vec<Claimed> = Vec::new();

            for (path, by) in &claims {
                let expected = met_by(&standing, path, by).cloned();
                let answer = owners.claim(path, by);

                match (expected, answer.is_err()) {
                    (Some(other), true) => {
                        prop_assert!(
                            standing.iter().any(|c| *c == other),
                            "refused by a claim nothing made"
                        );
                    }
                    (None, false) => {
                        if !standing.iter().any(|c| c.path == *path && c.by == *by) {
                            standing.push(Claimed { path: path.clone(), by });
                        }
                    }
                    (Some(other), false) => prop_assert!(
                        false,
                        "`{path}` was taken by `{by}` while `{other}` stands"
                    ),
                    (None, true) => prop_assert!(
                        false,
                        "`{path}` was refused for `{by}` and nothing stands over it: {:?}",
                        standing
                    ),
                }
            }
        }

        /// Which places end up owned does not depend on the order the claims
        /// arrived in.
        ///
        /// Not that the same claims are *refused* - they are not, and cannot
        /// be: of two owners meeting on one place, whoever asked first keeps
        /// it. What holds is that the two runs agree about the rule, so the
        /// set the rule admits is the set the store admits.
        #[test]
        fn the_shortcut_and_the_walk_agree_whatever_the_order(
            claims in prop::collection::vec((a_path(), an_owner()), 0..8)
        ) {
            let owners = Owners::default();
            for (path, by) in &claims {
                let _ = owners.claim(path, by);
            }

            let mut taken = owners.claims.read().clone();
            taken.sort_by(|a, b| a.path.cmp(&b.path));

            prop_assert_eq!(taken, taken_by_the_rule(&claims));
        }

        /// One owner may take any set of places at all. A struct is not in its
        /// own way: it claims its prefix and then every field under it, and
        /// each of those is inside the one before.
        #[test]
        fn one_owner_may_take_any_set_of_places(
            paths in prop::collection::vec(a_path(), 0..8)
        ) {
            let owners = Owners::default();

            for path in &paths {
                prop_assert!(
                    owners.claim(path, "Alone").is_ok(),
                    "`{path}` was refused to the owner that already held it"
                );
            }
        }

        /// The claims stay sorted, whatever arrives and in whatever order.
        ///
        /// Nothing sorts them - the one insertion goes where
        /// `partition_point` says - and every lookup is a binary search or a
        /// run, so this is the invariant all of them rest on and the only
        /// thing keeping it is that insertion.
        #[test]
        fn the_claims_are_sorted_however_they_arrived(
            claims in prop::collection::vec((a_path(), an_owner()), 0..12)
        ) {
            let owners = Owners::default();
            for (path, by) in &claims {
                let _ = owners.claim(path, by);
            }

            let held = owners.claims.read();
            let sorted: Vec<&StorePath> = {
                let mut at: Vec<&StorePath> = held.iter().map(|c| &c.path).collect();
                at.sort();
                at
            };

            prop_assert_eq!(
                held.iter().map(|c| &c.path).collect::<Vec<_>>(),
                sorted
            );
        }

        /// Claiming is idempotent for the owner that already holds the place,
        /// however many times a struct is built.
        #[test]
        fn taking_a_place_twice_leaves_one_claim(path in a_path()) {
            let owners = Owners::default();

            owners.claim(&path, "Twice").unwrap();
            owners.claim(&path, "Twice").unwrap();

            prop_assert_eq!(owners.claims.read().len(), 1);
        }
    }
}
