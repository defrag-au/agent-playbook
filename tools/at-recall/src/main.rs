//! `at-recall` — see `docs/inspection-tools.md` in the playbook for why this exists.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(at_recall::run(&args));
}
