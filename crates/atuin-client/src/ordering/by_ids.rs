use std::collections::HashMap;

use crate::history::{History, HistoryId};

/// Restore ranked ID order after hydrating histories from the database.
///
/// Missing IDs are skipped. Repeated IDs repeat the history, matching the
/// search protocol's previous behavior; only those repetitions require a clone.
#[must_use]
pub fn reorder_by_ids(histories: Vec<History>, ids: &[HistoryId]) -> Vec<History> {
    // Keep large History values in contiguous storage; the hash table only
    // needs small indices and occurrence counts, not a History in every bucket.
    let mut by_id = HashMap::with_capacity(histories.len());
    for (index, history) in histories.iter().enumerate() {
        by_id.entry(history.id).or_insert((index, 0_usize));
    }
    for id in ids {
        if let Some((_, remaining)) = by_id.get_mut(id) {
            *remaining += 1;
        }
    }
    let mut histories: Vec<_> = histories.into_iter().map(Some).collect();
    let mut ordered = Vec::with_capacity(ids.len().min(by_id.len()));
    ordered.extend(ids.iter().filter_map(|id| {
        let (index, remaining) = by_id.get_mut(id)?;
        let history = &mut histories[*index];
        let next = if *remaining == 1 {
            history.take()
        } else {
            history.clone()
        };
        *remaining -= 1;
        next
    }));
    ordered
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn histories() -> Vec<History> {
        (0..4)
            .map(|i| {
                History::import()
                    .timestamp(time::OffsetDateTime::UNIX_EPOCH)
                    .command(format!("command {i}"))
                    .cwd("/work")
                    .build()
                    .into()
            })
            .collect()
    }

    #[rstest]
    #[case::empty(vec![])]
    #[case::reverse(vec![3, 2, 1, 0])]
    #[case::subset(vec![2, 0])]
    #[case::repeated(vec![1, 1, 0, 1])]
    fn preserves_rank_and_repetitions(histories: Vec<History>, #[case] order: Vec<usize>) {
        let mut ids: Vec<_> = order.iter().map(|&i| histories[i].id).collect();
        ids.insert(0, HistoryId::from_bytes([0; 16]));
        let expected: Vec<_> = order.iter().map(|&i| histories[i].clone()).collect();
        assert_eq!(reorder_by_ids(histories, &ids), expected);
    }

    #[rstest]
    fn duplicate_hydrated_ids_use_the_first_row(mut histories: Vec<History>) {
        let first = histories[0].clone();
        let mut duplicate = first.clone();
        duplicate.command = "different command".into();
        histories.push(duplicate);
        assert_eq!(reorder_by_ids(histories, &[first.id]), vec![first]);
    }

    #[rstest]
    fn moves_unique_results_without_copying_strings(histories: Vec<History>) {
        let ids: Vec<_> = histories.iter().rev().map(|h| h.id).collect();
        let pointers: Vec<_> = histories.iter().rev().map(|h| h.command.as_ptr()).collect();
        let ordered = reorder_by_ids(histories, &ids);
        for (history, pointer) in ordered.iter().zip(pointers) {
            assert_eq!(history.command.as_ptr(), pointer);
        }
    }

    #[rstest]
    fn matches_previous_reordering(histories: Vec<History>) {
        proptest!(|(order in prop::collection::vec(0_usize..6, 0..50))| {
            let ids: Vec<_> = order.into_iter().map(|i| {
                histories.get(i).map_or(HistoryId::from_bytes([0; 16]), |h| h.id)
            }).collect();
            let expected: Vec<_> = ids.iter()
                .filter_map(|id| histories.iter().find(|h| h.id == *id).cloned())
                .collect();
            prop_assert_eq!(reorder_by_ids(histories.clone(), &ids), expected);
        });
    }
}
