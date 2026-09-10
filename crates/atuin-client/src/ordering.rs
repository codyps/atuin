mod by_ids;

pub use by_ids::reorder_by_ids;

use super::database::DbSearchMode;
use super::history::History;

#[must_use]
pub fn reorder_fuzzy(mode: DbSearchMode, query: &str, res: Vec<History>) -> Vec<History> {
    match mode {
        DbSearchMode::Fuzzy => reorder(query, |x| &x.command, res),
        DbSearchMode::Prefix | DbSearchMode::FullText => res,
    }
}

fn reorder<F, A>(query: &str, f: F, res: Vec<A>) -> Vec<A>
where
    F: Fn(&A) -> &String,
{
    let mut r = res;
    if r.len() < 2 {
        return r;
    }
    let qvec: Vec<_> = query.chars().collect();
    let mut starts = vec![usize::MAX; qvec.len()];
    let ascii_query = (!query.is_empty() && query.is_ascii()).then(|| AsciiQuery::new(query));
    r.sort_by_cached_key(|h| {
        // TODO for fzf search we should sum up scores for each matched term
        // A non-matching row sorts last rather than by a sentinel that can undercut a real match.
        let command = f(h);
        if let Some(query) = &ascii_query
            && command.is_ascii()
        {
            query.minimum_span(command.as_bytes(), &mut starts)
        } else {
            minimum_span(&qvec, command.chars(), &mut starts)
        }
    });
    r
}

/// Chains of matching query positions, highest index first. One lookup per
/// ASCII command byte replaces scanning the entire query, including repeated
/// letters and arbitrarily long queries. The table uses 1 KiB on 64-bit targets;
/// the predecessor vector is allocated once per query, never per result.
struct AsciiQuery {
    last: [usize; 128],
    previous: Vec<usize>,
}

impl AsciiQuery {
    fn new(query: &str) -> Self {
        let mut last = [usize::MAX; 128];
        let previous = query
            .bytes()
            .enumerate()
            .map(|(i, byte)| std::mem::replace(&mut last[usize::from(byte)], i))
            .collect();
        Self { last, previous }
    }

    /// Both query and command must be ASCII, and the query must be nonempty.
    fn minimum_span(&self, command: &[u8], starts: &mut [usize]) -> usize {
        starts.fill(usize::MAX);
        let mut best = usize::MAX;
        for (end, byte) in command.iter().copied().enumerate() {
            let mut i = self.last[usize::from(byte)];
            while i != usize::MAX {
                let start = if i == 0 {
                    end
                } else {
                    starts[i - 1]
                };
                starts[i] = start;
                if i == starts.len() - 1 && start != usize::MAX {
                    best = best.min(end - start + 1);
                    if best == starts.len() {
                        return best;
                    }
                }
                // Following predecessors keeps updates in descending query
                // order: a command character cannot satisfy two positions.
                i = self.previous[i];
            }
        }
        best
    }
}

/// Length of the shortest subsequence match, measured in characters.
///
/// `starts[i]` tracks the latest start of a match for the query prefix ending
/// at `i`. Update backwards so a character cannot match two query positions.
/// Scratch storage belongs to the query and is reset between commands.
fn minimum_span<T: Eq>(
    query: &[T],
    mut command: impl Iterator<Item = T>,
    starts: &mut [usize],
) -> usize {
    // Preserve minspan's empty-input behavior, including its one-character
    // score for an empty query against a nonempty command.
    if query.is_empty() {
        return command.next().map_or(usize::MAX, |_| 1);
    }

    starts.fill(usize::MAX);
    let mut best = usize::MAX;
    for (end, ch) in command.enumerate() {
        // Most command characters need no state updates. Slice membership can
        // use a vectorized scan (especially for ASCII) without touching scratch.
        if !query.contains(&ch) {
            continue;
        }
        for (i, key) in query.iter().enumerate().rev() {
            if *key != ch {
                continue;
            }
            let start = if i == 0 {
                end
            } else {
                starts[i - 1]
            };
            starts[i] = start;
            if i == query.len() - 1 && start != usize::MAX {
                best = best.min(end - start + 1);
                // A contiguous match cannot be improved by scanning the rest.
                if best == query.len() {
                    return best;
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use time::OffsetDateTime;

    use super::*;
    use crate::history::History;

    fn hist(command: &str) -> History {
        History::capture()
            .timestamp(OffsetDateTime::now_utc())
            .command(command)
            .cwd("/")
            .build()
            .into()
    }

    fn commands(res: Vec<History>) -> Vec<String> {
        res.into_iter().map(|h| h.command).collect()
    }

    #[rstest]
    // A non-matching row must sort last, not ahead of a genuine match.
    #[case::nonmatch_sorts_last("screen", vec!["screen", "hello"], vec!["screen", "hello"])]
    // The unchanged match path: a tight match outranks a loose one.
    #[case::matches_by_span("curl", vec!["central urllib", "curl"], vec!["curl", "central urllib"])]
    fn reorder_ranks(#[case] query: &str, #[case] input: Vec<&str>, #[case] expected: Vec<&str>) {
        let res = input.into_iter().map(hist).collect();
        let out = reorder_fuzzy(DbSearchMode::Fuzzy, query, res);
        assert_eq!(commands(out), expected);
    }

    #[rstest]
    #[case("", "", usize::MAX)]
    #[case("", "abc", 1)]
    #[case("a", "", usize::MAX)]
    #[case("aa", "a", usize::MAX)]
    #[case("aa", "aba", 3)]
    #[case("ab", "a---b ab", 2)]
    #[case("é日", "é x 日", 5)]
    #[case("ab", "a😀b", 3)]
    #[case("A", "a", usize::MAX)]
    fn span_edge_cases(#[case] query: &str, #[case] command: &str, #[case] expected: usize) {
        let query: Vec<_> = query.chars().collect();
        let mut starts = vec![usize::MAX; query.len()];
        assert_eq!(minimum_span(&query, command.chars(), &mut starts), expected);
    }

    #[rstest]
    fn stable_ties_and_empty_commands() {
        for query in ["", "ab"] {
            let input = vec!["ab1".to_owned(), "ab2".to_owned(), String::new()];
            assert_eq!(reorder(query, |s| s, input.clone()), input);
        }
    }

    #[rstest]
    #[case::ascii(false)]
    #[case::unicode(true)]
    fn matches_minspan_scores_and_ordering(#[case] unicode: bool) {
        let character = if unicode {
            prop_oneof![Just('a'), Just('b'), Just('é'), Just('日'), any::<char>()].boxed()
        } else {
            prop_oneof![Just('a'), Just('b'), Just(' '), Just('A')].boxed()
        };
        proptest!(|(query in prop::collection::vec(character.clone(), 0..12),
                    commands in prop::collection::vec(prop::collection::vec(character, 0..128), 0..16))| {
            let mut starts = vec![usize::MAX; query.len()];
            for command in &commands {
                let expected = minspan::minspan::span(&query, command)
                    .map_or(usize::MAX, |(start, end)| end - start + 1);
                prop_assert_eq!(minimum_span(&query, command.iter().copied(), &mut starts), expected);
                let query_text: String = query.iter().collect();
                let command_text: String = command.iter().collect();
                if !query_text.is_empty() && query_text.is_ascii() && command_text.is_ascii() {
                    prop_assert_eq!(AsciiQuery::new(&query_text)
                        .minimum_span(command_text.as_bytes(), &mut starts), expected);
                }
            }
            let input: Vec<String> = commands.into_iter().map(|s| s.into_iter().collect()).collect();
            let mut expected = input.clone();
            expected.sort_by_cached_key(|s| minspan::minspan::span(&query, &s.chars().collect())
                .map_or(usize::MAX, |(start, end)| end - start + 1));
            let query: String = query.into_iter().collect();
            prop_assert_eq!(reorder(&query, |s| s, input), expected);
        });
    }

    #[rstest]
    #[case(1)]
    #[case(64)]
    #[case(65)]
    #[case(256)]
    fn ascii_repeated_positions_have_no_word_size_limit(#[case] length: usize) {
        let query = "a".repeat(length);
        let scorer = AsciiQuery::new(&query);
        let mut starts = vec![usize::MAX; length];
        assert_eq!(scorer.minimum_span(query.as_bytes(), &mut starts), length);
        assert_eq!(scorer.minimum_span(&query.as_bytes()[1..], &mut starts), usize::MAX);
    }
}
