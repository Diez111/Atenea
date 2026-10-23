//! `aengine`: async engine facade for the Atenea UI.
//!
//! All heavy work (parsing, ngspice, file I/O) runs on a dedicated worker
//! thread. The UI only sends [`Job`]s and receives [`Outcome`]s, so the
//! render thread never blocks.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

/// One decoded part row for tables/BOM views.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartView {
    /// Reference (`R1`, `UNO`, ...).
    pub reference: String,
    /// Type (`RES`, `Arduino`, ...).
    pub kind: String,
    /// Value (`9k1`, `Arduino UNO`, ...).
    pub value: String,
    /// Extra fields (`EID=`, `PACKAGE=`, `key=`, ...).
    pub rest: Vec<String>,
}

/// One net with pin list for tables/graph views.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetView {
    /// Net id (`#00000`, `VCC`, ...).
    pub id: String,
    /// `(reference, pin, extra)` triples.
    pub pins: Vec<(String, String, String)>,
}

/// Whole opened document, ready for UI views.
#[derive(Debug, Clone, Default)]
pub struct DocSnapshot {
    /// File name for display.
    pub name: String,
    /// `pdsprj` | `lisa` | `dsn` | `idx` | `unknown`.
    pub kind: String,
    /// Summary rows (`format`, `project`, `wires`, ...).
    pub summary: Vec<(String, String)>,
    /// `*PARTLIST` rows.
    pub parts: Vec<PartView>,
    /// `*NETLIST` nets + pin counts.
    pub nets: Vec<NetView>,
    /// Wire segments in mm: `(x1, y1, x2, y2, bus)`.
    pub wires: Vec<(f64, f64, f64, f64, bool)>,
    /// Placed labels in mm: `(text, x, y)`.
    pub placements: Vec<(String, f64, f64)>,
    /// ERC issues: `(severity 'E'|'W', message)`.
    pub erc: Vec<(char, String)>,
}

/// One transient simulation result for plots.
#[derive(Debug, Clone, Default)]
pub struct TraceView {
    /// Variable names in column order.
    pub vars: Vec<String>,
    /// `(time_seconds, values)` rows.
    pub rows: Vec<(f64, Vec<f64>)>,
    /// Last values per visible series.
    pub last: Vec<f64>,
    /// Model substitutions (D/Q defaults) — never silent.
    pub subs: Vec<String>,
}

/// Work request for the engine thread.
#[derive(Debug)]
pub enum Job {
    /// Open any supported file into a [`DocSnapshot`].
    Open {
        path: PathBuf,
        reply: Sender<Result<DocSnapshot, String>>,
    },
    /// Run a transient sim on loaded SDF text.
    Simulate {
        name: String,
        sdf_text: String,
        vars: Vec<String>,
        tstop: String,
        reply: Sender<Result<TraceView, String>>,
    },
    /// Validate SDF text without running ngspice.
    Validate {
        sdf_text: String,
        reply: Sender<(Vec<String>, Vec<String>)>,
    },
}

/// The engine handle: clone it freely, it only holds a job sender.
#[derive(Debug, Clone)]
pub struct Engine {
    tx: Sender<Job>,
}

impl Engine {
    /// Spawn the worker thread and return a handle.
    ///
    /// # Panics
    ///
    /// Panics if the OS refuses to spawn the thread (out of resources).
    #[must_use]
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("atenea-engine".to_owned())
            .spawn(move || worker(&rx))
            .expect("engine thread spawns");
        Self { tx }
    }

    /// Non-blocking open: the reply arrives on the returned channel.
    #[must_use]
    pub fn open(&self, path: &Path) -> Receiver<Result<DocSnapshot, String>> {
        let (reply, rx) = mpsc::channel();
        let _ = self.tx.send(Job::Open {
            path: path.to_path_buf(),
            reply,
        });
        rx
    }

    /// Non-blocking transient simulation.
    #[must_use]
    pub fn simulate(
        &self,
        name: &str,
        sdf_text: &str,
        vars: &[String],
        tstop: &str,
    ) -> Receiver<Result<TraceView, String>> {
        let (reply, rx) = mpsc::channel();
        let _ = self.tx.send(Job::Simulate {
            name: name.to_owned(),
            sdf_text: sdf_text.to_owned(),
            vars: vars.to_vec(),
            tstop: tstop.to_owned(),
            reply,
        });
        rx
    }

    /// Non-blocking validation (no ngspice involved).
    #[must_use]
    pub fn validate(&self, sdf_text: &str) -> Receiver<(Vec<String>, Vec<String>)> {
        let (reply, rx) = mpsc::channel();
        let _ = self.tx.send(Job::Validate {
            sdf_text: sdf_text.to_owned(),
            reply,
        });
        rx
    }
}

fn kind_of(path: &Path) -> &str {
    match path
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

fn load_pdsprj(path: &Path, snap: &mut DocSnapshot) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let proj = pproj::open(file).map_err(|e| e.to_string())?;
    snap.summary.push((
        "project".to_owned(),
        proj.title.unwrap_or_else(|| "(workspace)".to_owned()),
    ));
    snap.summary.push((
        "timestamp".to_owned(),
        format!(
            "rel {} filever {} modified {}",
            proj.timestamp.release, proj.timestamp.filever, proj.timestamp.modified
        ),
    ));
    snap.summary
        .push(("members".to_owned(), proj.members.join(",")));
    Ok(())
}

fn load_lisa(path: &Path, snap: &mut DocSnapshot) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let doc = pmodel::parse(&text).map_err(|e| e.to_string())?;
    snap.summary.push(("format".to_owned(), doc.format.clone()));
    if let Some(d) = doc.preamble.get("Design") {
        snap.summary.push(("design".to_owned(), d.clone()));
    }
    for p in doc.partlist().map_err(|e| e.to_string())? {
        snap.parts.push(PartView {
            reference: p.reference,
            kind: p.kind,
            value: p.value,
            rest: p.rest,
        });
    }
    for n in doc.netlist().map_err(|e| e.to_string())? {
        snap.nets.push(NetView {
            id: n.id,
            pins: n.pins,
        });
    }
    snap.summary
        .push(("parts".to_owned(), snap.parts.len().to_string()));
    snap.summary
        .push(("nets".to_owned(), snap.nets.len().to_string()));
    Ok(())
}

fn load_dsn(path: &Path, snap: &mut DocSnapshot) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    plib::dsn::check_magic(&data).map_err(|e| e.to_string())?;
    let wires = plib::dsn::wires(&data);
    let bus = wires.iter().filter(|w| w.bus).count();
    snap.summary
        .push(("wires".to_owned(), wires.len().to_string()));
    snap.summary.push(("bus".to_owned(), bus.to_string()));
    for w in &wires {
        snap.wires.push((
            plib::dsn::to_mm(w.x1),
            plib::dsn::to_mm(w.y1),
            plib::dsn::to_mm(w.x2),
            plib::dsn::to_mm(w.y2),
            w.bus,
        ));
    }
    let placed = plib::dsn::placements(&data);
    snap.summary
        .push(("placements".to_owned(), placed.len().to_string()));
    for p in &placed {
        snap.placements
            .push((p.text.clone(), plib::dsn::to_mm(p.x), plib::dsn::to_mm(p.y)));
    }
    Ok(())
}

fn load_idx(path: &Path, snap: &mut DocSnapshot) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let idx = plib::libdir::parse_idx(&data, plib::LibKind::Idx).map_err(|e| e.to_string())?;
    let lib_path = path.with_extension("LIB");
    let lib_data = std::fs::read(&lib_path).unwrap_or_default();
    let dirs = plib::libdir::parse_lib_dir(&lib_data);
    let by_key: HashMap<u32, &plib::libdir::DirEntry> = dirs.iter().map(|d| (d.key, d)).collect();
    for e in &idx {
        let name = by_key
            .get(&e.key)
            .map_or_else(|| format!("key:{:08x}", e.key), |d| d.name.clone());
        let mut rest = vec![format!("key={:08x}", e.key), format!("CAT={}", e.cat)];
        if !e.mfr.is_empty() {
            rest.push(format!("MFR={}", e.mfr));
        }
        if let Some(d) = by_key.get(&e.key) {
            rest.push(format!("offset={}", d.offset));
            for (k, v) in plib::libdir::device_props(&lib_data, d.offset) {
                if matches!(k.as_str(), "PREFIX" | "MODDLL" | "PROGRAM" | "PRIMITIVE") {
                    rest.push(format!("{k}={v}"));
                }
                if rest.len() >= 6 {
                    break;
                }
            }
        }
        snap.parts.push(PartView {
            reference: name,
            kind: e.cat.clone(),
            value: e.desc.clone(),
            rest,
        });
    }
    snap.summary
        .push(("entries".to_owned(), idx.len().to_string()));
    Ok(())
}

fn open_sync(path: &Path) -> Result<DocSnapshot, String> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_owned();
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    let kind = kind_of(path).to_owned();
    let mut snap = DocSnapshot {
        name,
        kind,
        ..DocSnapshot::default()
    };
    match snap.kind.as_str() {
        "pdsprj" => load_pdsprj(path, &mut snap)?,
        "lisa" => load_lisa(path, &mut snap)?,
        "dsn" => load_dsn(path, &mut snap)?,
        "idx" => load_idx(path, &mut snap)?,
        other => return Err(format!("unsupported extension: .{other}")),
    }
    snap.erc = erc_of(&snap);
    Ok(snap)
}

/// ERC over a snapshot (mirrors `NETLIST::net_erc` + `validate`).
#[must_use]
pub fn erc_of(doc: &DocSnapshot) -> Vec<(char, String)> {
    let mut out = Vec::new();
    let mut rails_of = |pins: &[(String, String, String)]| {
        let mut classes: Vec<&str> = pins
            .iter()
            .map(|(r, _, _)| match r.as_str() {
                "VCC" | "VDD" | "+5V" => "VCC/VDD",
                "GND" | "VSS" => "GND",
                "VEE" | "-5V" => "VEE",
                _ => "",
            })
            .filter(|c| !c.is_empty())
            .collect();
        classes.sort_unstable();
        classes.dedup();
        classes
    };
    let _ = &mut rails_of;
    for n in &doc.nets {
        let classes = rails_of(&n.pins);
        if classes.len() >= 2 {
            out.push((
                'E',
                format!(
                    "Power rails '{}' and '{}' interconnected in net {}",
                    classes[0], classes[1], n.id
                ),
            ));
        }
        if n.pins.len() == 1 && !["VCC", "GND", "VDD", "VSS"].contains(&n.id.as_str()) {
            let role = n.pins[0].1.as_str();
            if role == "NC" {
            } else if ["GT", "LBL", "TER"].contains(&role) {
                out.push(('W', format!("SINGULAR LABEL: {}", n.id)));
            } else {
                out.push(('W', format!("UNDRIVEN: {} (1 pin)", n.id)));
            }
        } else if n.pins.is_empty() {
            out.push(('W', format!("net {}: flotante (0 pines)", n.id)));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for p in &doc.parts {
        if !seen.insert(&p.reference) {
            out.push(('E', format!("parte {}: referencia duplicada", p.reference)));
        }
    }
    out
}

fn simulate_sync(
    name: &str,
    sdf_text: &str,
    vars: &[String],
    tstop: &str,
) -> Result<TraceView, String> {
    let doc = pmodel::parse(sdf_text).map_err(|e| e.to_string())?;
    let (errs, _warns) = asim::validate(&doc);
    if !errs.is_empty() {
        return Err(format!("validate: {}", errs.join("; ")));
    }
    let refs: Vec<&str> = vars.iter().map(String::as_str).collect();
    let (trace, subs) = asim::simulate_tran_with_subs(&doc, name, "10u", tstop, &refs)
        .map_err(|e| format!("{e:?}"))?;
    let last = trace
        .rows
        .last()
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    Ok(TraceView {
        vars: trace.vars,
        rows: trace.rows,
        last,
        subs,
    })
}

fn worker(rx: &Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        match job {
            Job::Open { path, reply } => {
                let _ = reply.send(open_sync(&path));
            }
            Job::Simulate {
                name,
                sdf_text,
                vars,
                tstop,
                reply,
            } => {
                let _ = reply.send(simulate_sync(&name, &sdf_text, &vars, &tstop));
            }
            Job::Validate { sdf_text, reply } => {
                let doc = pmodel::parse(&sdf_text).unwrap_or_else(|_| pmodel::Document {
                    format: String::new(),
                    version: String::new(),
                    preamble: HashMap::new(),
                    sections: Vec::new(),
                });
                let _ = reply.send(asim::validate(&doc));
            }
        }
    }
}

/// Render the sheet (`wires` + `placements`) as a standalone SVG string
/// (ISIS look: light paper, blue sheet border, dark wires, thick buses).
/// Pure function — the worker builds it, the UI only displays it.
#[must_use]
pub fn sheet_svg(doc: &DocSnapshot, width: u32, height: u32) -> String {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for (x1, y1, x2, y2, _) in &doc.wires {
        xs.extend_from_slice(&[*x1, *x2]);
        ys.extend_from_slice(&[*y1, *y2]);
    }
    for (_, x, y) in &doc.placements {
        xs.push(*x);
        ys.push(*y);
    }
    let (min_x, max_x) = minmax(&xs, 400.0);
    let (min_y, max_y) = minmax(&ys, 300.0);
    let pad_x = (max_x - min_x).max(1.0) * 0.06;
    let pad_y = (max_y - min_y).max(1.0) * 0.06;
    let lo_x = min_x - pad_x;
    let hi_x = max_x + pad_x;
    let lo_y = min_y - pad_y;
    let hi_y = max_y + pad_y;
    let w = f64::from(width).max(1.0);
    let h = f64::from(height).max(1.0);
    let sx = w / (hi_x - lo_x);
    let sy = h / (hi_y - lo_y);
    let sc = sx.min(sy);
    let ox = (w - (hi_x - lo_x) * sc) / 2.0;
    let oy = (h - (hi_y - lo_y) * sc) / 2.0;
    let px = |x: f64| ox + (x - lo_x) * sc;
    // SVG origin is top-left: flip Y.
    let py = |y: f64| h - (oy + (y - lo_y) * sc);
    let mut s = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}' viewBox='0 0 {width} {height}'>\n<rect x='0' y='0' width='{width}' height='{height}' fill='#f4f6fa'/>\n"
    );
    // Sheet border (ISIS blue).
    let _ = writeln!(
        s,
        "<rect x='{ox:.0}' y='{oy:.0}' width='{:.0}' height='{:.0}' fill='#ffffff' stroke='#0f4c9c' stroke-width='2'/>",
        (hi_x - lo_x) * sc,
        (hi_y - lo_y) * sc
    );
    for (x1, y1, x2, y2, bus) in &doc.wires {
        let sw = if *bus { 3 } else { 1 };
        let _ = writeln!(
            s,
            "<line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#1c2733' stroke-width='{sw}'/>",
            px(*x1),
            py(*y1),
            px(*x2),
            py(*y2)
        );
    }
    for (text, x, y) in &doc.placements {
        let esc = text.replace('&', "&amp;").replace('<', "&lt;");
        let _ = writeln!(
            s,
            "<text x='{:.1}' y='{:.1}' font-family='monospace' font-size='11' fill='#0f4c9c'>{esc}</text>",
            px(*x),
            py(*y)
        );
    }
    s.push_str("</svg>\n");
    s
}

fn minmax(v: &[f64], fallback: f64) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, fallback);
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in v {
        lo = lo.min(*x);
        hi = hi.max(*x);
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5
=====================================
Design:   RC LOWPASS

*PARTLIST,2
V1,VSRC,5
R1,RES,9k1

*NETLIST,3
IN,2
V1,PS,1
R1,PS,1
MID,2
R1,PS,2
C1,PS,1
";

    #[test]
    fn open_lisa_snapshot() {
        let dir = std::env::temp_dir().join("atenea-aengine-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rc.sdf");
        std::fs::write(&path, RC).unwrap();
        let eng = Engine::spawn();
        let snap = eng.open(&path).recv().unwrap().unwrap();
        assert_eq!(snap.kind, "lisa");
        assert_eq!(snap.parts.len(), 2);
        assert_eq!(snap.nets.len(), 2);
    }

    #[test]
    fn validate_clean_and_shorted() {
        let eng = Engine::spawn();
        let (errs, _) = eng.validate(RC).recv().unwrap();
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn svg_renders_wires_and_labels() {
        let mut snap = DocSnapshot {
            name: "t".to_owned(),
            kind: "dsn".to_owned(),
            ..DocSnapshot::default()
        };
        snap.wires.push((0.0, 0.0, 10.0, 0.0, false));
        snap.wires.push((0.0, 0.0, 0.0, 5.0, true));
        snap.placements.push(("VCC".to_owned(), 10.0, 0.0));
        let svg = sheet_svg(&snap, 800, 600);
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<line").count(), 2);
        assert!(svg.contains("stroke-width='3'"));
        assert!(svg.contains(">VCC</text>"));
        assert!(svg.contains("#0f4c9c"));
    }
}
