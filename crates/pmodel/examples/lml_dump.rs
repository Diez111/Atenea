//! Split a real `.LML` container.
//!
//! Usage: `cargo run -p pmodel --example lml_dump -- <file>`

use std::env;

fn main() {
    let path = env::args().nth(1).expect("usage: lml_dump <file>");
    let data = std::fs::read(&path).expect("read file");
    let decks = pmodel::lml::split(&data).expect("split LML");
    println!("decks={}", decks.len());
    for d in &decks {
        let first = d.text.lines().next().unwrap_or("");
        println!("  {} ({} bytes): {first}", d.name, d.text.len());
    }
}
