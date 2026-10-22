//! Dump `{...}` blocks of a real `.DSN`/`.LYT` file.
//!
//! Usage: `cargo run -p plib --example dsn_dump -- <file> [limit]`

use std::env;

fn main() {
    let path = env::args().nth(1).expect("usage: dsn_dump <file> [limit]");
    let limit: usize = env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let data = std::fs::read(&path).expect("read file");
    let header = plib::dsn::check_magic(&data).expect("valid DSN/LYT magic");
    println!("header_len={header} size={} bytes", data.len());
    let blocks = plib::dsn::blocks(&data);
    println!("blocks={}", blocks.len());
    for b in blocks.iter().take(limit) {
        let body: String = b.body.chars().take(100).collect();
        println!("@{:>8} {{{}}} {body}", b.offset, b.head);
    }
    let wires = plib::dsn::wires(&data);
    println!("wires={}", wires.len());
    if !wires.is_empty() {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for w in &wires {
            for (x, y) in [(w.x1, w.y1), (w.x2, w.y2)] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        println!(
            "bbox_units x:[{x0},{x1}] y:[{y0},{y1}] bbox_mm {:.2}x{:.2}",
            plib::dsn::to_mm(x1 - x0),
            plib::dsn::to_mm(y1 - y0)
        );
        for w in wires.iter().take(limit) {
            println!(
                "  ({:.2},{:.2})-({:.2},{:.2}) mm",
                plib::dsn::to_mm(w.x1),
                plib::dsn::to_mm(w.y1),
                plib::dsn::to_mm(w.x2),
                plib::dsn::to_mm(w.y2)
            );
        }
    }
}
