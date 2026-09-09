mod _util;
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
