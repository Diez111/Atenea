//! Atenea batch simulation: Proteus SDF netlists to ngspice and back.
//!
//! v0 scope: passive + source elements (`R`, `C`, `L`, `V`, `I`) from
//! `*PARTLIST`/`*NETLIST`, value normalization (`9k1` → `9.1k`,
//! `4M7` → `4.7MEG`, bare `M` reads as mega like Proteus), `GND` net
//! mapped to ngspice node `0`. Anything else (transistors, diodes,
//! ICs, subcircuits) returns [`SimError::UnsupportedPart`].
//!
//! Execution shells out to `ngspice -b`; no GUI, no daemon.

use std::collections::HashMap;
use std::fmt;
use std::process::Command;

/// Simulation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    /// Part kind outside the v0 set (`Q`, `D`, `U`, ...).
    UnsupportedPart(String),
    /// SDF structure problem.
    BadNetlist(String),
    /// `ngspice` binary missing or failed.
    NgspiceFailed(String),
    /// Filesystem failure (temp files).
    Io(String),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPart(p) => write!(f, "unsupported part: {p}"),
            Self::BadNetlist(e) => write!(f, "bad netlist: {e}"),
            Self::NgspiceFailed(e) => write!(f, "ngspice failed: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for SimError {}

/// ngspice run output.
#[derive(Debug, Clone)]
pub struct Output {
    /// Raw stdout of the batch run.
    pub stdout: String,
    /// Data rows reported (`No. of Data Rows`).
    pub data_rows: usize,
}

/// One transient table: time column + one column per requested variable.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// Variable names in column order.
    pub vars: Vec<String>,
    /// `(time_seconds, values)` rows.
    pub rows: Vec<(f64, Vec<f64>)>,
}

/// Normalize a Proteus value (`9k1`, `4u7`, `100n`) to SPICE text.
#[must_use]
pub fn normalize_value(value: &str) -> String {
    let v = value.trim();
    // Look for DIGITS LETTER DIGITS (e.g. 9k1, 4u7).
    if let Some(pos) = v.find(|c: char| c.is_ascii_alphabetic()) {
        let (head, tail) = v.split_at(pos);
        let mut chars = tail.chars();
        let letter = chars.next().unwrap_or(' ');
        let rest: String = chars.collect();
        if !head.is_empty()
            && head.bytes().all(|b| b.is_ascii_digit())
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
            && matches!(
                letter.to_ascii_uppercase(),
                'R' | 'K' | 'M' | 'U' | 'N' | 'P' | 'F'
            )
        {
            // ngspice reads bare `M` as milli; Proteus means mega → `MEG`.
            let scale = if letter.eq_ignore_ascii_case(&'M') {
                "MEG".to_owned()
            } else {
                letter.to_string()
            };
            return format!("{head}.{rest}{scale}");
        }
    }
    v.to_owned()
}

/// Sanitize a net id into an ngspice node name (`#00000` → `N00000`).
fn node(net: &str) -> String {
    if net.eq_ignore_ascii_case("GND") {
        return "0".to_owned();
    }
    let clean: String = net
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        format!("N{clean}")
    } else {
        clean
    }
}

/// Validate a document before simulation (mirrors `NETLIST::validate`).
///
/// Returns `(errors, warnings)` in file order. Errors: duplicate refs,
/// distinct power-rail classes shorted in one net (PWRRAILS.INI classes:
/// `VCC/VDD`, `GND/VSS`, `VEE`). Warnings: 1-pin nets (except power
/// nets and `NC` roles), empty nets, parts with no net pins.
#[must_use]
pub fn validate(doc: &pmodel::Document) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warns = Vec::new();
    let parts = doc.partlist().unwrap_or_default();
    let nets = doc.netlist().unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    for p in &parts {
        if !seen.insert(p.reference.clone()) {
            errors.push(format!("duplicate ref: {}", p.reference));
        }
    }
    let class_of = |r: &str| match r {
        "VCC" | "VDD" | "+5V" => "VCC/VDD",
        "GND" | "VSS" => "GND",
        "VEE" | "-5V" => "VEE",
        _ => "",
    };
    for n in &nets {
        let mut classes: Vec<&str> = n
            .pins
            .iter()
            .map(|(r, _, _)| class_of(r.as_str()))
            .filter(|c| !c.is_empty())
            .collect();
        classes.sort_unstable();
        classes.dedup();
        if classes.len() >= 2 {
            errors.push(format!(
                "Power rails '{}' and '{}' interconnected in net {}",
                classes[0], classes[1], n.id
            ));
        }
        if n.pins.len() == 1 && !["VCC", "GND", "VDD", "VSS"].contains(&n.id.as_str()) {
            let role = n.pins[0].1.as_str();
            if role != "NC" {
                warns.push(format!("UNDRIVEN: {} (1 pin)", n.id));
            }
        } else if n.pins.is_empty() {
            warns.push(format!("net {}: flotante (0 pines)", n.id));
        }
    }
    for p in &parts {
        let in_net = nets
            .iter()
            .any(|n| n.pins.iter().any(|(r, _, _)| r == &p.reference));
        if !in_net {
            warns.push(format!("parte {}: sin pines en netlist", p.reference));
        }
    }
    (errors, warns)
}

/// Convert SDF into ngspice lines, mapping `D`/`Q` to ngspice default
/// models (behavioral fallback).
///
/// Vendor DSIM primitives (`OA91`, `TIP31`, ...) ship as compiled model
/// DLLs with no SPICE deck; only `BC184` (`ZETEX.SML`) and `CA3140`
/// (`ANALOG.LML`) have vendor decks. Each substitution is reported in
/// the returned warnings so callers can surface it (never silent).
/// `U` (ICs) and `X`/`M` stay [`SimError::UnsupportedPart`].
///
/// # Errors
///
/// [`SimError::BadNetlist`] on missing pins; [`SimError::UnsupportedPart`]
/// for `U`/`X`/`M`/other letters.
pub fn elements_with_subs(doc: &pmodel::Document) -> Result<(Vec<String>, Vec<String>), SimError> {
    let parts = doc
        .partlist()
        .map_err(|e| SimError::BadNetlist(e.to_string()))?;
    let nets = doc
        .netlist()
        .map_err(|e| SimError::BadNetlist(e.to_string()))?;
    let mut pins: HashMap<&str, Vec<&str>> = HashMap::new();
    for net in &nets {
        for (rx, _, _) in &net.pins {
            pins.entry(rx.as_str()).or_default().push(net.id.as_str());
        }
    }
    let mut out = Vec::new();
    let mut subs = Vec::new();
    let mut models: Vec<String> = Vec::new();
    for p in &parts {
        // Vendor letter comes from the reference (`D1`, `Q1`), NOT from
        // `kind` (model name: `OA91`, `TIP31`).
        let letter = p
            .reference
            .chars()
            .next()
            .unwrap_or('?')
            .to_ascii_uppercase();
        let nets_of = pins
            .get(p.reference.as_str())
            .ok_or_else(|| SimError::BadNetlist(format!("{} has no pins", p.reference)))?;
        let num = p
            .reference
            .trim_start_matches(|c: char| c.is_ascii_alphabetic());
        if matches!(letter, 'R' | 'C' | 'L' | 'V' | 'I') {
            if nets_of.len() != 2 {
                return Err(SimError::BadNetlist(format!(
                    "{} expects 2 pins, has {}",
                    p.reference,
                    nets_of.len()
                )));
            }
            out.push(format!(
                "{}{} {} {} {}",
                letter,
                num,
                node(nets_of[0]),
                node(nets_of[1]),
                normalize_value(&p.value)
            ));
        } else if letter == 'D' {
            if nets_of.len() != 2 {
                return Err(SimError::BadNetlist(format!(
                    "{} expects 2 pins, has {}",
                    p.reference,
                    nets_of.len()
                )));
            }
            let model = format!("DMOD_{}", sanitize(&p.kind));
            out.push(format!(
                "D{num} {} {} {model}",
                node(nets_of[0]),
                node(nets_of[1])
            ));
            if !models.contains(&model) {
                models.push(model.clone());
                out.push(format!(".MODEL {model} D"));
                subs.push(format!(
                    "{}: sin deck SPICE vendor, modelo D default (no OA91 germanio)",
                    p.reference
                ));
            }
        } else if letter == 'Q' {
            if nets_of.len() != 3 {
                return Err(SimError::BadNetlist(format!(
                    "{} expects 3 pins, has {}",
                    p.reference,
                    nets_of.len()
                )));
            }
            let model = format!("QMOD_{}", sanitize(&p.kind));
            out.push(format!(
                "Q{num} {} {} {} {model}",
                node(nets_of[0]),
                node(nets_of[1]),
                node(nets_of[2])
            ));
            if !models.contains(&model) {
                models.push(model.clone());
                out.push(format!(".MODEL {model} NPN"));
                subs.push(format!(
                    "{}: sin deck SPICE vendor, modelo NPN default",
                    p.reference
                ));
            }
        } else {
            return Err(SimError::UnsupportedPart(format!(
                "{}(kind {})",
                p.reference, p.kind
            )));
        }
    }
    Ok((out, subs))
}

/// Identifier-safe model suffix (`TIP31` -> `TIP31`, `2N 2222` -> `2N_2222`).
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Convert an SDF `*PARTLIST`/`*NETLIST` document into ngspice element
/// lines (no title, no analysis, no `.end` — see [`simulate`]).
///
/// # Errors
///
/// [`SimError::UnsupportedPart`] for non R/C/L/V/I parts,
/// [`SimError::BadNetlist`] on missing pins.
pub fn elements(doc: &pmodel::Document) -> Result<Vec<String>, SimError> {
    let parts = doc
        .partlist()
        .map_err(|e| SimError::BadNetlist(e.to_string()))?;
    let nets = doc
        .netlist()
        .map_err(|e| SimError::BadNetlist(e.to_string()))?;
    // ref -> [(net, pin)] in netlist order.
    let mut pins: HashMap<&str, Vec<&str>> = HashMap::new();
    for net in &nets {
        for (rx, _, _) in &net.pins {
            pins.entry(rx.as_str()).or_default().push(net.id.as_str());
        }
    }
    let mut out = Vec::new();
    for p in &parts {
        // Vendor letter comes from the reference (`D1`, `Q1`), NOT from
        // `kind` (which is the model name: `OA91`, `TIP31`, `BC184`).
        let letter = p
            .reference
            .chars()
            .next()
            .unwrap_or('?')
            .to_ascii_uppercase();
        if !matches!(letter, 'R' | 'C' | 'L' | 'V' | 'I') {
            return Err(SimError::UnsupportedPart(format!(
                "{}(kind {})",
                p.reference, p.kind
            )));
        }
        let nets_of = pins
            .get(p.reference.as_str())
            .ok_or_else(|| SimError::BadNetlist(format!("{} has no pins", p.reference)))?;
        if nets_of.len() != 2 {
            return Err(SimError::BadNetlist(format!(
                "{} expects 2 pins, has {}",
                p.reference,
                nets_of.len()
            )));
        }
        out.push(format!(
            "{}{} {} {} {}",
            letter,
            p.reference
                .trim_start_matches(|c: char| c.is_ascii_alphabetic()),
            node(nets_of[0]),
            node(nets_of[1]),
            normalize_value(&p.value)
        ));
    }
    Ok(out)
}

/// Build a transient `.cir` netlist from SDF elements (pure, no I/O).
///
/// # Errors
///
/// Propagates [`SimError`] from [`elements`].
pub fn to_cir(
    doc: &pmodel::Document,
    title: &str,
    dt: &str,
    t_end: &str,
    vars: &[&str],
) -> Result<String, SimError> {
    let mut lines = vec![format!("* {title}")];
    lines.extend(elements(doc)?);
    lines.extend(vars.iter().map(|v| format!(".print tran {v}")));
    lines.push(format!(".tran {dt} {t_end} uic"));
    lines.push(".end".to_owned());
    Ok(lines.join("\n") + "\n")
}

/// Build a transient `.cir` with D/Q default-model substitution.
///
/// Returns the netlist plus substitution warnings (see
/// [`elements_with_subs`]).
///
/// # Errors
///
/// Propagates [`SimError`] from [`elements_with_subs`].
pub fn to_cir_with_subs(
    doc: &pmodel::Document,
    title: &str,
    dt: &str,
    t_end: &str,
    vars: &[&str],
) -> Result<(String, Vec<String>), SimError> {
    let mut lines = vec![format!("* {title}")];
    let (els, subs) = elements_with_subs(doc)?;
    lines.extend(els);
    lines.extend(vars.iter().map(|v| format!(".print tran {v}")));
    lines.push(format!(".tran {dt} {t_end} uic"));
    lines.push(".end".to_owned());
    Ok((lines.join("\n") + "\n", subs))
}

/// Write a `.cir` netlist to disk.
///
/// # Errors
///
/// [`SimError::Io`] on filesystem failure.
pub fn write_cir(path: &std::path::Path, cir: &str) -> Result<(), SimError> {
    std::fs::write(path, cir).map_err(|e| SimError::Io(e.to_string()))
}

/// Quote one CSV field per RFC 4180.
fn rfc4180(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// `*PARTLIST` as CSV (`reference,kind,value,extra`) for BOM/debug.
///
/// # Errors
///
/// [`SimError::BadNetlist`] when the part list is malformed.
pub fn parts_csv(doc: &pmodel::Document) -> Result<String, SimError> {
    let mut out = String::from("reference,kind,value,extra\n");
    for p in doc
        .partlist()
        .map_err(|e| SimError::BadNetlist(e.to_string()))?
    {
        out.push_str(&rfc4180(&p.reference));
        out.push(',');
        out.push_str(&rfc4180(&p.kind));
        out.push(',');
        out.push_str(&rfc4180(&p.value));
        out.push(',');
        out.push_str(&rfc4180(&p.rest.join(",")));
        out.push('\n');
    }
    Ok(out)
}

/// Write a parts CSV to disk.
///
/// # Errors
///
/// [`SimError::Io`] on filesystem failure.
pub fn write_parts_csv(path: &std::path::Path, csv: &str) -> Result<(), SimError> {
    std::fs::write(path, csv).map_err(|e| SimError::Io(e.to_string()))
}

/// Run a full netlist text through `ngspice -b`, returning stdout.
///
/// # Errors
///
/// [`SimError::Io`] on temp-file failure,
/// [`SimError::NgspiceFailed`] on nonzero exit or missing binary.
pub fn run(netlist: &str) -> Result<Output, SimError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tag = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("atenea-asim-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| SimError::Io(e.to_string()))?;
    let cir = dir.join("tb.cir");
    let log = dir.join("out.log");
    std::fs::write(&cir, netlist).map_err(|e| SimError::Io(e.to_string()))?;
    let status = Command::new("ngspice")
        .arg("-b")
        .arg("-o")
        .arg(&log)
        .arg(&cir)
        .status()
        .map_err(|e| SimError::NgspiceFailed(format!("spawn: {e}")))?;
    if !status.success() {
        return Err(SimError::NgspiceFailed(format!(
            "exit {status}, see {}",
            log.display()
        )));
    }
    let stdout = std::fs::read_to_string(&log).map_err(|e| SimError::Io(e.to_string()))?;
    let data_rows = stdout
        .lines()
        .filter_map(|l| {
            l.strip_prefix("No. of Data Rows : ")
                .and_then(|n| n.trim().parse::<usize>().ok())
        })
        .sum();
    Ok(Output { stdout, data_rows })
}

/// Last value of the `var_index`-th `print` variable in batch stdout.
/// Column 0 is the sweep variable (time); variables follow it. Only real
/// data rows count (first field parses as integer), so headers and the
/// trailing `Stack = ...` footer are ignored. Unlike [`trace`], this
/// accepts rows with any column count.
#[must_use]
pub fn last_printed(output: &Output, var_index: usize) -> Option<f64> {
    output
        .stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.first()?.parse::<usize>().ok()?;
            fields.get(1)?.parse::<f64>().ok()?;
            fields.get(var_index + 2)?.parse::<f64>().ok()
        })
        .next_back()
}

/// Parse `print tran` tables with exactly `nvars` variables.
///
/// Data rows look like `index time v1 ... vn` (all numeric, index
/// integer). ngspice may repeat the header between chunks; all matching
/// rows are collected, headers and footers never match.
#[must_use]
pub fn trace(output: &Output, nvars: usize) -> Option<Trace> {
    let mut rows: Vec<(f64, Vec<f64>)> = Vec::new();
    for line in output.stdout.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() == nvars + 2
            && fields[0].parse::<usize>().is_ok()
            && fields[1].parse::<f64>().is_ok()
            && fields[2..].iter().all(|f| f.parse::<f64>().is_ok())
        {
            let mut vals = Vec::with_capacity(nvars);
            for f in &fields[2..] {
                vals.push(f.parse::<f64>().unwrap_or(f64::NAN));
            }
            rows.push((fields[1].parse().unwrap_or(f64::NAN), vals));
        }
    }
    if rows.is_empty() {
        return None;
    }
    Some(Trace {
        vars: (0..nvars).map(|i| format!("v{i}")).collect(),
        rows,
    })
}

/// Build a transient netlist from SDF elements and run it, returning the
/// parsed trace of `vars` (e.g. `["v(OUT)"]`).
///
/// # Errors
///
/// Propagates [`SimError`] from [`elements`] and [`run`], plus
/// [`SimError::BadNetlist`] when no trace table is found.
pub fn simulate_tran(
    doc: &pmodel::Document,
    title: &str,
    dt: &str,
    t_end: &str,
    vars: &[&str],
) -> Result<Trace, SimError> {
    let (errs, _warns) = validate(doc);
    if !errs.is_empty() {
        return Err(SimError::BadNetlist(format!(
            "validate: {}",
            errs.join("; ")
        )));
    }
    let netlist = to_cir(doc, title, dt, t_end, vars)?;
    let out = run(&netlist)?;
    trace(&out, vars.len()).ok_or_else(|| SimError::BadNetlist("no trace table".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_clean_rc_and_shorted_rails() {
        let doc = pmodel::parse(RC_SDF).unwrap();
        let (errs, _warns) = validate(&doc);
        assert!(errs.is_empty(), "{errs:?}");
        let bad = pmodel::parse(
            "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n====\n*PARTLIST,2\nV1,VSRC,5\nR1,RES,9k1\n*NETLIST,2\nSHORT,2\nVCC,PS,1\nGND,PS,1\nOUT,2\nV1,PS,2\nR1,PS,1\n",
        )
        .unwrap();
        let (errs, _) = validate(&bad);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("interconnected"), "{}", errs[0]);
        // validate() gates simulation with a vendor-style message.
        assert!(
            simulate_tran(&bad, "t", "10u", "5m", &["v(OUT)"])
                .unwrap_err()
                .to_string()
                .contains("validate:")
        );
    }

    const RC_SDF: &str = "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5
=====================================
Design:   RC LOWPASS

*PARTLIST,3
V1,VSRC,5
R1,RES,9k1
C1,CAP,100n

*NETLIST,2
IN,2
V1,PS,1
R1,PS,1
OUT,2
R1,PS,2
C1,PS,1
GND,2
V1,PS,2
C1,PS,2
";

    const GOLDEN4: &str = "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5
=====================================
Design:   GOLDEN4

*PARTLIST,4
V1,VSRC,5
R1,RES,4M7
C1,CAP,100n
L1,IND,10u

*NETLIST,4
IN,2
V1,PS,1
R1,PS,1
MID,2
R1,PS,2
C1,PS,1
OUT,2
C1,PS,2
L1,PS,1
GND,2
V1,PS,2
L1,PS,2
";

    #[test]
    fn golden_four_parts_exact() {
        let doc = pmodel::parse(GOLDEN4).unwrap();
        let els = elements(&doc).unwrap();
        assert_eq!(
            els,
            vec![
                "V1 IN 0 5".to_owned(),
                "R1 IN MID 4.7MEG".to_owned(),
                "C1 MID OUT 100n".to_owned(),
                "L1 OUT 0 10u".to_owned(),
            ]
        );
    }

    #[test]
    fn unsupported_part_names_part() {
        let bad = GOLDEN4.replace("L1,IND,10u", "Q1,NPN,BC184");
        let doc = pmodel::parse(&bad).unwrap();
        let err = elements(&doc).unwrap_err();
        assert_eq!(err, SimError::UnsupportedPart("Q1(kind NPN)".to_owned()));
        assert_eq!(err.to_string(), "unsupported part: Q1(kind NPN)");
    }

    #[test]
    fn to_cir_is_golden() {
        let doc = pmodel::parse(GOLDEN4).unwrap();
        let cir = to_cir(&doc, "G4", "10u", "5m", &["v(OUT)", "v(MID)"]).unwrap();
        let expected = "* G4\nV1 IN 0 5\nR1 IN MID 4.7MEG\nC1 MID OUT 100n\nL1 OUT 0 10u\n.print tran v(OUT)\n.print tran v(MID)\n.tran 10u 5m uic\n.end\n";
        assert_eq!(cir, expected);
    }

    #[test]
    fn parts_csv_quotes_commas() {
        let doc = pmodel::parse(
            "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n====\n*PARTLIST,2\nR1,RES,9k1,EID=6,PACKAGE=RES40,PINSWAP=\"1,2\"\nU1,CA3140,CA3140\n*NETLIST,0\n",
        )
        .unwrap();
        let csv = parts_csv(&doc).unwrap();
        assert_eq!(
            csv,
            "reference,kind,value,extra\nR1,RES,9k1,\"EID=6,PACKAGE=RES40,PINSWAP=\"\"1,2\"\"\"\nU1,CA3140,CA3140,\n"
        );
    }

    #[test]
    fn trace_parses_fabricated_stdout() {
        let out = Output {
            stdout: "bla bla\n0 1e-05 4.9 0.1\n1 2e-05 5.0 0.2\nStack = 0 bytes.\n".to_owned(),
            data_rows: 2,
        };
        let t = trace(&out, 2).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(last_printed(&out, 0), Some(5.0));
        assert_eq!(last_printed(&out, 1), Some(0.2));
    }

    #[test]
    fn values_normalize() {
        assert_eq!(normalize_value("9k1"), "9.1k");
        assert_eq!(normalize_value("4u7"), "4.7u");
        assert_eq!(normalize_value("4M7"), "4.7MEG");
        assert_eq!(normalize_value("100n"), "100n");
        assert_eq!(normalize_value("5"), "5");
    }

    #[test]
    fn elements_map() {
        let doc = pmodel::parse(RC_SDF).unwrap();
        let els = elements(&doc).unwrap();
        assert_eq!(els.len(), 3);
        assert!(els.iter().any(|e| e.starts_with("V1 ")));
        assert!(els.iter().any(|e| e == "R1 IN OUT 9.1k"));
        assert!(els.iter().any(|e| e == "C1 OUT 0 100n"));
    }

    #[test]
    fn unsupported_part_errors() {
        let doc = pmodel::parse(RC_SDF.replace("C1,CAP,100n", "Q1,NPN,BC184").as_str()).unwrap();
        let err = elements(&doc).unwrap_err();
        assert!(matches!(err, SimError::UnsupportedPart(_)));
    }

    #[test]
    fn dq_substitution_simulates() {
        if std::process::Command::new("ngspice")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        // Diode clipper + NPN switch with default models.
        let sdf = "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n=====\n*PARTLIST,4\nV1,VSRC,5\nR1,RES,1k\nD1,OA91,OA91\nQ1,BC184,BC184\n*NETLIST,4\nIN,2\nV1,PS,1\nR1,PS,1\nMID,3\nR1,PS,2\nD1,PS,K\nQ1,PS,1\nOUT,2\nD1,PS,A\nQ1,PS,2\nGND,2\nV1,PS,2\nQ1,PS,3\n";
        let doc = pmodel::parse(sdf).unwrap();
        let (cir, subs) = to_cir_with_subs(&doc, "dq", "10u", "1m", &["v(OUT)"]).expect("dq cir");
        assert_eq!(subs.len(), 2, "{subs:?}");
        assert!(cir.contains(".MODEL DMOD_OA91 D"), "{cir}");
        assert!(cir.contains(".MODEL QMOD_BC184 NPN"), "{cir}");
        let out = run(&cir).expect("ngspice dq");
        assert!(!out.stdout.is_empty(), "vacio");
    }

    #[test]
    fn rc_transient_charges() {
        // Needs the ngspice binary; passes vacuously without it so a
        // clean checkout stays green (CI without ngspice).
        if std::process::Command::new("ngspice")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        // `uic`: start the capacitor discharged, watch it charge to 5 V.
        let doc = pmodel::parse(RC_SDF).unwrap();
        let trace =
            simulate_tran(&doc, "Atenea RC lowpass", "10u", "5m", &["v(OUT)"]).expect("simulate");
        assert!(trace.rows.len() > 100);
        let (_, last) = trace.rows.last().unwrap();
        assert!((last[0] - 5.0).abs() < 0.25, "v(out)={}", last[0]);
        // Monotonic charge (allow tiny numerical wiggles).
        let mut prev = 0.0;
        for (_, v) in &trace.rows {
            assert!(v[0] + 1e-6 >= prev, "non-monotonic: {} after {prev}", v[0]);
            prev = v[0];
        }
    }
}
