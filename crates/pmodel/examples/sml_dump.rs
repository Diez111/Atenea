//! Split a real `.SML` container.
//!
//! Usage: `cargo run -p pmodel --example sml_dump -- <file>`

use std::env;

fn main() {
    let path = env::args().nth(1).expect("usage: sml_dump <file>");
    let data = std::fs::read(&path).expect("read file");
    let decks = pmodel::sml::split(&data).expect("split SML");
    println!("decks={}", decks.len());
    for d in &decks {
        let first = d.text.lines().next().unwrap_or("");
        println!("  {} ({} bytes): {first}", d.name, d.text.len());
    }
}
