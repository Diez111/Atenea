//! Dump a real `ROOT.CDB`.
//!
//! Usage: `cargo run -p pproj --example cdb_dump -- <file>`

use std::env;

fn main() {
    let path = env::args().nth(1).expect("usage: cdb_dump <file>");
    let data = std::fs::read(&path).expect("read file");
    let cdb = pproj::cdb::parse(&data);
    println!("size={} header={:?}", data.len(), cdb.header);
    println!("components={}", cdb.components.len());
    for c in cdb.components.iter().take(20) {
        let pins: Vec<String> = c.pins.iter().map(|(n, num)| format!("{n}={num}")).collect();
        println!("  {} [{}]", c.name, pins.join(" "));
    }
    println!("records={}", cdb.records.len());
    for r in cdb.records.iter().take(10) {
        println!("  {{{r}}}");
    }
}
