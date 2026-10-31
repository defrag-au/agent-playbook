//! `at-cost`'s entry point.
//!
//! The work lives in the library so `at-describe` can link the verb table without a second copy
//! of the grammar — the same arrangement every other member of the toolkit uses.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(at_cost::run(&args));
}
