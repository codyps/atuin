//! Full index construction, including row decoding, insertion, and teardown.
//! The grouped baseline preserves the former loader for comparison; it can return
//! fewer invocations and is deliberately not a correctness reference.
//!
//! Divan allocation counters cover Rust allocations on the benchmark thread, not SQLx's worker
//! threads. Use a process-wide heap profiler for total allocation or RSS data.

use atuin_client::database::Sqlite;
use atuin_client::history::History;
use atuin_daemon::search::SearchIndex;
use easy_cast::Conv;
use time::OffsetDateTime;

fn run(bencher: divan::Bencher, rows: usize, baseline: Option<bool>) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let db = runtime.block_on(async {
        let db = Sqlite::in_memory(std::time::Duration::from_secs(5)).await.unwrap();
        let histories: Vec<History> = (0..rows)
            .map(|i| {
                History::import()
                    .timestamp(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(i64::conv(i)))
                    .command(format!("git show {}", i % (rows / 4).max(1)))
                    .cwd("/work")
                    .session("00000000000000000000000000000001")
                    .build()
                    .into()
            })
            .collect();
        db.save_bulk(&histories).await.unwrap();
        db
    });

    bencher.bench(|| {
        let index = SearchIndex::default();
        runtime.block_on(async {
            if let Some(grouped) = baseline {
                let mut pages = db.all_paged(SearchIndex::HISTORY_LOAD_PAGE_SIZE, false, grouped);
                while let Some(histories) = pages.next().await.unwrap() {
                    index.add_histories(&histories);
                }
            } else {
                index.load_from_db(&db).await.unwrap();
            }
        });
        assert_eq!(index.command_count(), (rows / 4).max(1));
        divan::black_box(index);
    });
}

#[divan::bench(args = [1_000, 10_000, 100_000])]
fn load_index(bencher: divan::Bencher, rows: usize) {
    run(bencher, rows, None);
}

#[divan::bench(args = [1_000, 10_000, 100_000])]
fn grouped_load_baseline(bencher: divan::Bencher, rows: usize) {
    run(bencher, rows, Some(true));
}

/// Isolates borrowed decoding from the pagination change: both visit every row.
#[divan::bench(args = [1_000, 10_000, 100_000])]
fn owned_load_baseline(bencher: divan::Bencher, rows: usize) {
    run(bencher, rows, Some(false));
}
