//! Invocation metadata is usually a singleton. Keep that case inline without
//! making every command-map bucket large enough to hold a hash table.

use std::collections::HashSet;
use std::hash::Hash;

#[derive(Debug)]
pub(super) enum SmallSet<T> {
    One(T),
    #[expect(
        clippy::box_collection,
        reason = "keep hash-table metadata out of the inline singleton and command-map bucket"
    )]
    Many(Box<HashSet<T>>),
}

impl<T: Copy + Eq + Hash> SmallSet<T> {
    pub(super) fn insert(&mut self, value: T) -> bool {
        match self {
            Self::One(existing) if *existing == value => false,
            Self::One(existing) => {
                *self = Self::Many(Box::new(HashSet::from([*existing, value])));
                true
            }
            Self::Many(values) => values.insert(value),
        }
    }

    pub(super) fn contains(&self, value: &T) -> bool {
        match self {
            Self::One(existing) => existing == value,
            Self::Many(values) => values.contains(value),
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        let (one, many) = match self {
            Self::One(value) => (Some(value), None),
            Self::Many(values) => (None, Some(values)),
        };
        one.into_iter().chain(many.into_iter().flat_map(|values| values.iter()))
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Many(values) => values.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn singleton_promotes_only_for_a_distinct_value() {
        let mut set = SmallSet::One(1);
        assert!(!set.insert(1));
        assert!(matches!(set, SmallSet::One(1)));
        assert!(set.insert(2));
        assert!(matches!(set, SmallSet::Many(_)));
        assert!(!set.insert(2));
        assert_eq!(set.len(), 2);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(!set.contains(&3));
    }

    #[rstest]
    fn matches_hash_set() {
        proptest!(|(first: u16, rest in prop::collection::vec(any::<u16>(), 0..200))| {
            let mut compact = SmallSet::One(first);
            let mut reference = HashSet::from([first]);
            for value in rest {
                prop_assert_eq!(compact.insert(value), reference.insert(value));
                prop_assert!(compact.contains(&value));
            }
            prop_assert_eq!(compact.len(), reference.len());
            prop_assert_eq!(compact.iter().copied().collect::<HashSet<_>>(), reference);
        });
    }

    #[rstest]
    fn representation_is_smaller_than_hash_sets() {
        assert!(size_of::<SmallSet<u32>>() < size_of::<HashSet<u32>>());
        assert!(size_of::<SmallSet<uuid::Uuid>>() < size_of::<HashSet<uuid::Uuid>>());
    }
}
