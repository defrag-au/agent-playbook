//! `at-reach` — see `docs/fleet.md` in the playbook for the fleet-root contract.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(at_reach::run(&args));
}
