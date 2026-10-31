//! `fleet-sync` — the writer, and the only binary in this package.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(fleet_sync::run(&args));
}
