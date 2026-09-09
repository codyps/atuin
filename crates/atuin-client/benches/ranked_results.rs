//! Rank restoration for the interactive UI's 200-result hydration limit.

use atuin_client::history::History;
use atuin_client::ordering::reorder_by_ids;

fn run(bencher: divan::Bencher, command_bytes: usize, clone_baseline: bool) {
    let histories: Vec<History> = (0..200)
        .map(|_| {
            History::import()
                .timestamp(time::OffsetDateTime::UNIX_EPOCH)
                .command("x".repeat(command_bytes))
                .cwd("/home/user/work/project")
                .build()
                .into()
        })
        .collect();
    let ids: Vec<_> = histories.iter().rev().map(|h| h.id).collect();
    bencher.with_inputs(|| histories.clone()).bench_values(|histories| {
        if clone_baseline {
            ids.iter()
                .filter_map(|id| histories.iter().find(|h| h.id == *id).cloned())
                .collect::<Vec<_>>()
        } else {
            reorder_by_ids(histories, &ids)
        }
    });
}

#[divan::bench(args = [32, 4096])]
fn move_ranked_results(bencher: divan::Bencher, command_bytes: usize) {
    run(bencher, command_bytes, false);
}

#[divan::bench(args = [32, 4096])]
fn clone_ranked_results_baseline(bencher: divan::Bencher, command_bytes: usize) {
    run(bencher, command_bytes, true);
}
