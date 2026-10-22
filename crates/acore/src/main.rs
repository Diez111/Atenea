//! `acore`: JSON sidecar over the Atenea Rust engine.
//!
//! The Go UI (`goatenea/`) shells out to this binary instead of linking
//! Rust: `acore <cmd> <file> [--vars ..] [--tstop ..]` prints one JSON
//! document to stdout. Errors go to stderr with a nonzero exit.

use std::path::Path;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn usage() -> ! {
    eprintln!("usage: acore <summary|tables|wires|sim> <file> [--vars VARS] [--tstop T]");
    std::process::exit(2);
}

fn kind_of(path: &str) -> &str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "pdsprj" => "pdsprj",
        "mdf" | "sdf" => "lisa",
        "dsn" | "lyt" => "dsn",
        "idx" => "idx",
        _ => "unknown",
    }
}

fn cmd_summary(path: &str) -> Result<String, String> {
    let kind = kind_of(path);
    let (pairs, counts) = match kind {
        "pdsprj" => {
            let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let p = pproj::open(f).map_err(|e| e.to_string())?;
            let title = p.title.unwrap_or_else(|| "(workspace)".to_owned());
            (
                vec![
                    ("project".to_owned(), title),
                    ("release".to_owned(), p.timestamp.release),
                    ("members".to_owned(), p.members.join(",")),
                ],
                vec![],
            )
        }
        "lisa" => {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let doc = pmodel::parse(&text).map_err(|e| e.to_string())?;
            let parts = doc.partlist().map_err(|e| e.to_string())?.len();
            let nets = doc.netlist().map_err(|e| e.to_string())?.len();
            (
                vec![
                    ("format".to_owned(), doc.format),
                    ("design".to_owned(), doc.preamble.get("Design").cloned().unwrap_or_default()),
                ],
                vec![("parts".to_owned(), parts), ("nets".to_owned(), nets)],
            )
        }
        "dsn" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            plib::dsn::check_magic(&data).map_err(|e| e.to_string())?;
            let wires = plib::dsn::wires(&data);
            let placed = plib::dsn::placements(&data);
            let bus = wires.iter().filter(|w| w.bus).count();
            (
                vec![("magic".to_owned(), "ok".to_owned())],
                vec![
                    ("wires".to_owned(), wires.len()),
                    ("bus".to_owned(), bus),
                    ("placements".to_owned(), placed.len()),
                ],
            )
        }
        "idx" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let idx = plib::libdir::parse_idx(&data, plib::LibKind::Idx)
                .map_err(|e| e.to_string())?;
            (
                vec![],
                vec![("entries".to_owned(), idx.len())],
            )
        }
        _ => return Err(format!("unsupported file: {path}")),
    };
    let mut o = format!("{{\"file\":\"{}\",\"kind\":\"{kind}\",\"summary\":{{", esc(path));
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("\"{}\":\"{}\",", esc(k), esc(v)));
    }
    o.push_str("\"_\":null},\"counts\":{");
    for (i, (k, v)) in counts.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("\"{k}\":{v}"));
    }
    o.push_str("}}");
    Ok(o)
}

fn cmd_tables(path: &str) -> Result<String, String> {
    let kind = kind_of(path);
    let mut parts: Vec<(String, String, String, Vec<String>)> = Vec::new();
    let mut nets: Vec<(String, Vec<(String, String, String)>)> = Vec::new();
    match kind {
        "lisa" => {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let doc = pmodel::parse(&text).map_err(|e| e.to_string())?;
            for p in doc.partlist().map_err(|e| e.to_string())? {
                parts.push((p.reference, p.kind, p.value, p.rest));
            }
            for n in doc.netlist().map_err(|e| e.to_string())? {
                nets.push((n.id, n.pins));
            }
        }
        "idx" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let idx = plib::libdir::parse_idx(&data, plib::LibKind::Idx)
                .map_err(|e| e.to_string())?;
            let libp = Path::new(path).with_extension("LIB");
            let dirs = std::fs::read(&libp)
                .map(|v| plib::libdir::parse_lib_dir(&v))
                .unwrap_or_default();
            let by_key: std::collections::HashMap<u32, &plib::libdir::DirEntry> =
                dirs.iter().map(|d| (d.key, d)).collect();
            for e in &idx {
                let name = by_key
                    .get(&e.key)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("key:{:08x}", e.key));
                let mut rest = vec![format!("key={:08x}", e.key), format!("CAT={}", e.cat)];
                if !e.mfr.is_empty() {
                    rest.push(format!("MFR={}", e.mfr));
                }
                if let Some(d) = by_key.get(&e.key) {
                    rest.push(format!("offset={}", d.offset));
                }
                parts.push((name, e.cat.clone(), e.desc.clone(), rest));
            }
        }
        _ => return Err(format!("tables needs .MDF/.SDF/.IDX, got: {path}")),
    }
    let mut o = format!("{{\"file\":\"{}\",\"parts\":[", esc(path));
    for (i, (r, k, v, rest)) in parts.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "{{\"ref\":\"{}\",\"kind\":\"{}\",\"value\":\"{}\",\"rest\":[",
            esc(r),
            esc(k),
            esc(v)
        ));
        for (j, f) in rest.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str(&format!("\"{}\"", esc(f)));
        }
        o.push_str("]}");
    }
    o.push_str("],\"nets\":[");
    for (i, (id, pins)) in nets.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("{{\"id\":\"{}\",\"pins\":[", esc(id)));
        for (j, (r, p, x)) in pins.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str(&format!("[\"{}\",\"{}\",\"{}\"]", esc(r), esc(p), esc(x)));
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    Ok(o)
}

fn cmd_wires(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    plib::dsn::check_magic(&data).map_err(|e| e.to_string())?;
    let wires = plib::dsn::wires(&data);
    let placed = plib::dsn::placements(&data);
    let mut o = format!("{{\"file\":\"{}\",\"wires\":[", esc(path));
    for (i, w) in wires.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let (x1, y1, x2, y2) = (
            plib::dsn::to_mm(w.x1),
            plib::dsn::to_mm(w.y1),
            plib::dsn::to_mm(w.x2),
            plib::dsn::to_mm(w.y2),
        );
        o.push_str(&format!("[{x1},{y1},{x2},{y2},{}]", w.bus as u8));
    }
    o.push_str("],\"placements\":[");
    for (i, p) in placed.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "[\"{}\",{},{}]",
            esc(&p.text),
            plib::dsn::to_mm(p.x),
            plib::dsn::to_mm(p.y)
        ));
    }
    o.push_str("]}");
    Ok(o)
}

fn cmd_sim(path: &str, vars: &[&str], tstop: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let doc = pmodel::parse(&text).map_err(|e| e.to_string())?;
    let trace = asim::simulate_tran(&doc, "acore", "10u", tstop, vars)
        .map_err(|e| format!("{e:?}"))?;
    let mut o = String::from("{\"vars\":[");
    for (i, v) in trace.vars.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("\"{}\"", esc(v)));
    }
    o.push_str("],\"rows\":[");
    for (i, (t, vals)) in trace.rows.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("[{t}"));
        for v in vals {
            o.push_str(&format!(",{v}"));
        }
        o.push(']');
    }
    o.push_str("]}");
    Ok(o)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| usage());
    let file = args.next().unwrap_or_else(|| usage());
    let mut vars: Vec<String> = vec!["v(OUT)".to_owned()];
    let mut tstop = "5m".to_owned();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--vars" => {
                if let Some(v) = args.next() {
                    vars = v.split_whitespace().map(str::to_owned).collect();
                }
            }
            "--tstop" => {
                if let Some(v) = args.next() {
                    tstop = v;
                }
            }
            _ => usage(),
        }
    }
    let out = match cmd.as_str() {
        "summary" => cmd_summary(&file),
        "tables" => cmd_tables(&file),
        "wires" => cmd_wires(&file),
        "sim" => cmd_sim(&file, &vars.iter().map(|s| s.as_str()).collect::<Vec<_>>(), &tstop),
        _ => usage(),
    };
    match out {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("acore: {e}");
            std::process::exit(1);
        }
    }
}
