//! Classify each command line from stdin, one per line, as a tab-separated row.
//!
//! The first field is the line's effect; each later field names a segment and its own effect. A
//! line the parser refused starts with `unknown` and carries the parser's message.
//!
//! ```sh
//! cargo run -p command-classify --example classify -- < commands.txt
//! ```

use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }

        let analysis = command_classify::classify(&line);
        print!("{:?}", analysis.effect);
        if analysis.writes != command_classify::Writes::None {
            print!("/{:?}", analysis.writes);
        }
        for segment in &analysis.segments {
            print!("\t{}={:?}", segment.program, segment.effect);
        }
        if let Some(error) = analysis.parse_error {
            print!("\t{error}");
        }
        println!("\t< {line}");
    }
}
