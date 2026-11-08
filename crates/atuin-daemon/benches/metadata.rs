//! Compare the private compact collection with the former three-HashSet layout.

use std::collections::HashSet;

use easy_cast::Conv;
use uuid::Uuid;

// Benchmark the actual implementation without exposing it as public API.
#[allow(
    dead_code,
    unused_imports,
    clippy::redundant_pub_crate,
    reason = "reuse private library code; this custom benchmark harness does not collect its \
              inline tests"
)]
#[path = "../src/search/small_set.rs"]
mod small_set;
use small_set::SmallSet;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    type Compact = (SmallSet<u32>, SmallSet<u32>, SmallSet<Uuid>);
    type Baseline = (HashSet<u32>, HashSet<u32>, HashSet<Uuid>);
    eprintln!(
        "inline metadata: compact {} bytes, baseline {} bytes",
        size_of::<Compact>(),
        size_of::<Baseline>()
    );
    divan::main();
}

#[divan::bench(args = [1, 2, 4, 16, 128])]
fn compact_metadata(bencher: divan::Bencher, contexts: usize) {
    bencher.bench(|| {
        let mut directories = SmallSet::One(0_u32);
        let mut hosts = SmallSet::One(0_u32);
        let mut sessions = SmallSet::One(Uuid::nil());
        for i in 0..contexts {
            directories.insert(u32::conv(i));
            hosts.insert(u32::conv(i));
            sessions.insert(Uuid::from_u128(u128::conv(i)));
        }
        divan::black_box(directories.contains(&0));
        divan::black_box(hosts.iter().copied().sum::<u32>());
        divan::black_box(sessions.contains(&Uuid::nil()));
        divan::black_box((directories, hosts, sessions));
    });
}

#[divan::bench(args = [1, 2, 4, 16, 128])]
fn hash_set_metadata_baseline(bencher: divan::Bencher, contexts: usize) {
    bencher.bench(|| {
        let mut directories = HashSet::from([0_u32]);
        let mut hosts = HashSet::from([0_u32]);
        let mut sessions = HashSet::from([Uuid::nil()]);
        for i in 0..contexts {
            directories.insert(u32::conv(i));
            hosts.insert(u32::conv(i));
            sessions.insert(Uuid::from_u128(u128::conv(i)));
        }
        divan::black_box(directories.contains(&0));
        divan::black_box(hosts.iter().copied().sum::<u32>());
        divan::black_box(sessions.contains(&Uuid::nil()));
        divan::black_box((directories, hosts, sessions));
    });
}
