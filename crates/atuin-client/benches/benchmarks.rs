mod _util;
mod fuzzy_scoring;
mod history;
mod history_store;
mod ordering;
mod ranked_results;
mod record;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}
