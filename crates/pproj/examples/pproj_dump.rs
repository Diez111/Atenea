//! Open a real `.pdsprj` and summarize it.
//!
//! Usage: `cargo run -p pproj --example pproj_dump -- <file>`

use std::env;
use std::fs::File;

fn main() {
    let path = env::args().nth(1).expect("usage: pproj_dump <file>");
    let proj = pproj::open(File::open(&path).expect("open file")).expect("open pdsprj");
    println!("title={}", proj.title.as_deref().unwrap_or("(workspace)"));
    println!(
        "release={} filever={} modified={}",
        proj.timestamp.release, proj.timestamp.filever, proj.timestamp.modified
    );
    let dsn_len = proj.dsn.as_ref().map_or(0, Vec::len);
    let cdb_len = proj.cdb.as_ref().map_or(0, Vec::len);
    println!("dsn_bytes={dsn_len} cdb_bytes={cdb_len}");
    let blocks = proj.dsn.as_deref().map_or(Vec::new(), plib::dsn::blocks);
    println!("dsn_blocks={}", blocks.len());
    if let Some(ref cdb) = proj.cdb {
        let parsed = pproj::cdb::parse(cdb);
        println!(
            "cdb_components={} cdb_records={}",
            parsed.components.len(),
            parsed.records.len()
        );
    }
    for b in blocks.iter().take(10) {
        let body: String = b.body.chars().take(60).collect();
        println!("@{:>7} {{{}}} {body}", b.offset, b.head);
    }
}
