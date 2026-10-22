//! Coverage probe: walk directories, classify every file by magic bytes.
//!
//! Usage: `cargo run -p plib --example corpus_check -- <dir>...`
//!
//! Classification is evidence, not parsing: magic match only.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn kind_of(path: &Path, head: &[u8]) -> &'static str {
    if head.starts_with(b"DEVICE LIBRARY INDEX\x1a") {
        return "idx";
    }
    if head.starts_with(b"DEVICE LIBRARY\x1a") {
        return "lib";
    }
    if head.starts_with(b"PACKAGE LIBRARY\x1a") {
        return "package-lib";
    }
    if head.starts_with(b"MODEL LIBRARY\x1a") {
        return "lml";
    }
    if head.starts_with(b"SYMBOL LIBRARY INDEX\x1a") {
        return "symbol-idx";
    }
    if head.starts_with(b"SYMBOL LIBRARY\x1a") {
        return "symbol-lib";
    }
    // Visual/3D models: `MM<u16 version>` (`LX..` on older files).
    if head.starts_with(b"MM") || head.starts_with(b"LX") {
        return "vml";
    }
    if head.starts_with(b"ISIS SCHEMATIC FILE\x1a") {
        return "dsn";
    }
    if head.starts_with(b"PCB LAYOUT FILE\x1a") {
        return "lyt";
    }
    if head.starts_with(b"LISA MODEL DESCRIPTION FORMAT") {
        return "mdf";
    }
    if head.starts_with(b"ISIS SCHEMATIC DESCRIPTION FORMAT") {
        return "sdf";
    }
    if head.starts_with(b"SPICE LIBRARY\x1a") {
        return "sml";
    }
    if head.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return "zip(pdsprj?)";
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "qm" => "qm(qt)",
        "chm" => "chm(docs)",
        "dll" | "exe" => "pe-binary",
        "pdf" => "pdf",
        "txt" | "ini" | "sty" | "adi" => "text",
        "c" | "h" | "asm" | "inc" => "source",
        "hex" | "elf" | "bin" | "cod" | "lst" | "map" | "obj" => "firmware",
        "cof" | "mcp" | "mcw" | "lkr" | "err" | "sdi" | "d39" | "xcl" => "mplab",
        "bas" | "pbp" | "pbe" | "mci" => "basic",
        _ => "unknown",
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let dirs: Vec<String> = env::args().skip(1).collect();
    if dirs.is_empty() {
        eprintln!("usage: corpus_check <dir>...");
        std::process::exit(2);
    }
    let mut files = Vec::new();
    for d in &dirs {
        walk(Path::new(d), &mut files);
    }
    // Classify in parallel: one chunk per CPU. `kind_of` is pure over
    // (path, head bytes), so threads share nothing but join handles.
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = files.len().div_ceil(threads).max(1);
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut unknowns = Vec::new();
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for part in files.chunks(chunk) {
            handles.push(s.spawn(move || {
                let mut local: Vec<(&'static str, PathBuf)> = Vec::with_capacity(part.len());
                for path in part {
                    let head = fs::read(path)
                        .map(|b| b.into_iter().take(64).collect::<Vec<_>>())
                        .unwrap_or_default();
                    local.push((kind_of(path, &head), path.clone()));
                }
                local
            }));
        }
        for h in handles {
            for (kind, path) in h.join().unwrap_or_default() {
                *counts.entry(kind).or_insert(0) += 1;
                if kind == "unknown" && unknowns.len() < 200 {
                    unknowns.push(path);
                }
            }
        }
    });
    println!("files={}", files.len());
    for (kind, n) in &counts {
        println!("  {kind:>14}: {n}");
    }
    let covered: usize = [
        "idx",
        "lib",
        "package-lib",
        "lml",
        "sml",
        "symbol-lib",
        "symbol-idx",
        "dsn",
        "lyt",
        "mdf",
        "sdf",
    ]
    .iter()
    .map(|k| counts.get(k).copied().unwrap_or(0))
    .sum();
    let total = files.len().max(1);
    let pct10 = 1000 * covered / total;
    println!(
        "covered_by_parsers={covered}/{} ({}.{}%)",
        files.len(),
        pct10 / 10,
        pct10 % 10
    );
    for u in unknowns.iter().take(20) {
        println!("  unknown: {}", u.display());
    }
}
