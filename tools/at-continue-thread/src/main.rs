//! `at-continue-thread`'s entry point.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(at_continue_thread::run(&args));
}
