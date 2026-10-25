//! `skill-store` — see README.md for the store layout and the source grammar.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(skill_store::run(&args));
}
