//! Parse a real LISA-family file (`.MDF`/`.SDF`) and summarize it.
//!
//! Usage: `cargo run -p pmodel --example lisa_dump -- <file>`

use std::env;

fn main() {
    let path = env::args().nth(1).expect("usage: lisa_dump <file>");
    let text = std::fs::read_to_string(&path).expect("read file");
    let doc = pmodel::parse(&text).expect("parse LISA");
    println!("format={} version={}", doc.format, doc.version);
    for sec in &doc.sections {
        println!("section *{} rows={}", sec.name, sec.lines.len());
    }
    let parts = doc.partlist().expect("partlist");
    println!("parts={}", parts.len());
    for p in parts.iter().take(5) {
        println!("  {} {} {}", p.reference, p.kind, p.value);
    }
    let nets = doc.netlist().expect("netlist");
    let pins: usize = nets.iter().map(|n| n.pins.len()).sum();
    println!("nets={} pins={}", nets.len(), pins);
    for n in nets.iter().take(3) {
        println!("  {} ({} pins)", n.id, n.pins.len());
    }
    let defs = doc.modeldefs();
    println!("modeldefs={}", defs.len());
}
