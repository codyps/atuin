//! Compare complete fuzzy rank restoration with the former allocating scorer.
//!
//! Histories and their clones are constructed outside timing. Scoring and stable
//! sorting are timed; Divan also reports allocation counts/bytes. The 200-row case
//! is the interactive limit; 10 and 1,000 exercise smaller and CLI-sized batches.

use atuin_client::database::DbSearchMode;
use atuin_client::history::History;
use atuin_client::ordering::reorder_fuzzy;

const SCENARIOS: &[&str] = &[
    "ascii_short",
    "ascii_long",
    "ascii_dense",
    "unicode_short",
    "unicode_long",
    "ascii_query_unicode",
    "long_query",
    "long_query_dense",
    "repeated_query",
    "no_match",
    "empty_query",
];

fn run(bencher: divan::Bencher, rows: usize, scenario: &str, baseline: bool) {
    let (query, padding) = match scenario {
        "ascii_short" => ("curl", 64),
        "ascii_long" => ("curl", 4096),
        "unicode_short" => ("é日", 64),
        "unicode_long" => ("é日", 4096),
        "ascii_query_unicode" | "ascii_dense" | "no_match" => ("curl", 256),
        "long_query" | "long_query_dense" => ("abcdefghijklmnopqrstuvwx", 256),
        "repeated_query" => ("aaaaaaaa", 256),
        "empty_query" => ("", 256),
        _ => unreachable!("unknown benchmark scenario"),
    };
    let padding = match scenario {
        "ascii_dense" => "cargo build --release && git status; ".repeat(8),
        "long_query_dense" => "xwvutsrqponmlkjihgfedcba ".repeat(12),
        _ => "~".repeat(padding),
    };
    let loose = query.chars().map(|ch| ch.to_string()).collect::<Vec<_>>().join(" ");
    let histories: Vec<History> = (0..rows)
        .map(|i| {
            // Mix optimal early matches, loose matches, late matches and misses.
            let mut command = match if scenario == "no_match" {
                3
            } else {
                i % 4
            } {
                0 => format!("{query} {padding} {i}"),
                1 => format!("{loose} {padding} {i}"),
                2 => format!("{padding} {query} {i}"),
                _ => format!("{padding} {i}"),
            };
            if scenario == "ascii_query_unicode" {
                command.insert_str(0, "é日 ");
            }
            History::import()
                .timestamp(time::OffsetDateTime::UNIX_EPOCH)
                .command(command)
                .cwd("/work")
                .build()
                .into()
        })
        .collect();

    // Ensure both measured paths return exactly the same ranked histories.
    assert_eq!(
        reorder_fuzzy(DbSearchMode::Fuzzy, query, histories.clone()),
        allocating_baseline(query, histories.clone())
    );
    bencher.with_inputs(|| histories.clone()).bench_values(|histories| {
        if baseline {
            allocating_baseline(query, histories)
        } else {
            reorder_fuzzy(DbSearchMode::Fuzzy, query, histories)
        }
    });
}

fn allocating_baseline(query: &str, mut histories: Vec<History>) -> Vec<History> {
    let query = query.chars().collect();
    histories.sort_by_cached_key(|h| {
        minspan::minspan::span(&query, &h.command.chars().collect())
            .map_or(usize::MAX, |(from, to)| 1 + to - from)
    });
    histories
}

#[divan::bench(consts = [10, 200, 1_000], args = SCENARIOS)]
fn optimized<const N: usize>(bencher: divan::Bencher, scenario: &str) {
    run(bencher, N, scenario, false);
}

#[divan::bench(consts = [10, 200, 1_000], args = SCENARIOS)]
fn baseline<const N: usize>(bencher: divan::Bencher, scenario: &str) {
    run(bencher, N, scenario, true);
}
