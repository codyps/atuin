//! Full index construction, including row decoding, insertion, and teardown.
//! The grouped baseline preserves the former loader for comparison; it returns
//! fewer invocations and is deliberately not a correctness reference.

use atuin_client::database::Sqlite;
use atuin_client::history::History;
use atuin_daemon::search::SearchIndex;
use easy_cast::Conv;
use time::OffsetDateTime;

fn run(bencher: divan::Bencher, rows: usize, grouped: bool) {
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
            if grouped {
                let mut pages = db.all_paged(SearchIndex::HISTORY_LOAD_PAGE_SIZE, false, true);
                while let Some(histories) = pages.next().await.unwrap() {
                    index.add_histories(&histories);
                }
            } else {
                index.load_from_db(&db).await.unwrap();
            }
        });
        divan::black_box(index);
    });
}

#[divan::bench(args = [1_000, 10_000, 100_000])]
fn load_index(bencher: divan::Bencher, rows: usize) {
    run(bencher, rows, false);
}

#[divan::bench(args = [1_000, 10_000, 100_000])]
fn grouped_load_baseline(bencher: divan::Bencher, rows: usize) {
    run(bencher, rows, true);
}
