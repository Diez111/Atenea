//! Canonical LISA writer: `Document` back to text.
//!
//! Mirrors `NETLIST::net_gen` dispatch (`NETLIST.dll` 0x1000fa00): a
//! 12-byte format table `{u32 key, u32 subkey, writer fn}` keyed by
//! 3-letter codes (`SDF`, `NET`, `CPT`, `CMP`, `CIR`, `MDF`). Only the
//! LISA family (`SDF`/`MDF`) is implemented; the rest is an explicit
//! [`EmitError::UnsupportedFormat`], never silent output.
//!
//! The writer recomputes every `*NAME,count` from the actual body lines
//! (mirroring `NETLIST::write`, which emits a placeholder count and
//! backpatches it). Roundtrip is defined at the typed level:
//! `parse(emit(parse(x)))` yields an equal `Document` with recomputed
//! counts. `Net` markers (`*ID,GT`) and sub-header `*` forms are not
//! preserved — canonical output uses plain `#ID,count` / `ID,count` form.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Document, ModelDef, Net, Part};

/// Quote a CSV field when it contains `,`, `"` or whitespace edges.
fn quote_csv_field(field: &str) -> String {
    if field.contains([',', '"']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// `*NAME,count` header plus body lines (count always recomputed).
fn section_to_text(name: &str, lines: &[String]) -> String {
    let mut out = format!("*{name},{}\n", lines.len());
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One `*PARTLIST` row: `REF,TYPE,VALUE,extra...` (quotes restored).
#[must_use]
pub fn part_to_line(part: &Part) -> String {
    let mut fields = vec![
        part.reference.clone(),
        part.kind.clone(),
        part.value.clone(),
    ];
    fields.extend(part.rest.iter().cloned());
    fields
        .iter()
        .map(|f| quote_csv_field(f))
        .collect::<Vec<_>>()
        .join(",")
}

/// One `*MODELDEFS` row in canonical `NAME : params` form.
#[must_use]
pub fn modeldef_to_line(def: &ModelDef) -> String {
    format!("{} : {}", def.name, def.params)
}

/// One net: header `ID,count` plus one line per pin.
/// Pins with empty extra use the 2-field `REF,KIND` form.
#[must_use]
pub fn net_to_lines(net: &Net) -> Vec<String> {
    let mut out = vec![format!("{},{}", net.id, net.pins.len())];
    for (reference, pin, extra) in &net.pins {
        if extra.is_empty() {
            out.push(format!("{reference},{pin}"));
        } else {
            out.push(format!("{reference},{pin},{extra}"));
        }
    }
    out
}

/// Whole document to canonical LISA text.
///
/// Section bodies round-trip verbatim; only the `*NAME,count` headers
/// are recomputed from the actual body lines (mirroring how
/// `NETLIST::write` backpatches its placeholder count). Preamble order
/// is normalized (sorted keys) for determinism.
/// Format codes from the `NETLIST::net_gen` dispatch table.
pub const NETLIST_FORMATS: &[&str] = &["SDF", "NET", "CPT", "CMP", "CIR", "MDF"];

/// Writer error: only LISA (`SDF`/`MDF`) is implemented natively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// Format code from [`NETLIST_FORMATS`] without a native writer.
    UnsupportedFormat(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFormat(code) => write!(f, "unsupported netlist format: {code}"),
        }
    }
}

impl std::error::Error for EmitError {}

/// Emit `doc` in the given `NETLIST::net_gen` format code.
///
/// # Errors
///
/// [`EmitError::UnsupportedFormat`] for any code but `SDF`/`MDF`.
pub fn emit_format(doc: &Document, code: &str) -> Result<String, EmitError> {
    match code {
        "SDF" | "MDF" => Ok(emit(doc)),
        other => Err(EmitError::UnsupportedFormat(other.to_owned())),
    }
}

#[must_use]
pub fn emit(doc: &Document) -> String {
    let mut out = String::new();
    if !doc.format.is_empty() {
        let _ = writeln!(out, "{} FORMAT {}", doc.format, doc.version);
        out.push_str("=====================================\n");
    }
    let preamble: BTreeMap<&String, &String> = doc.preamble.iter().collect();
    for (key, value) in preamble {
        let _ = writeln!(out, "{key}: {value}");
    }
    for section in &doc.sections {
        out.push_str(&section_to_text(&section.name, &section.lines));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse;

    const SAMPLE: &str =
        "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n=====\nDesign: D\n*PROPERTIES,1\nTRACE=1\n";

    #[test]
    fn roundtrip_preamble_only() {
        let d1 = parse(SAMPLE).unwrap();
        let d2 = parse(&super::emit(&d1)).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn registry_lists_vendor_codes() {
        assert_eq!(super::NETLIST_FORMATS.len(), 6);
        assert!(super::NETLIST_FORMATS.contains(&"MDF"));
        assert!(super::NETLIST_FORMATS.contains(&"SDF"));
        let d = parse("ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n====\nDesign: D\n").unwrap();
        assert!(super::emit_format(&d, "SDF").is_ok());
        assert_eq!(
            super::emit_format(&d, "CIR"),
            Err(super::EmitError::UnsupportedFormat("CIR".into()))
        );
    }

    #[test]
    fn counts_recomputed() {
        let mut d = parse(SAMPLE).unwrap();
        d.sections[0].lines.push("X=1".to_owned());
        let text = super::emit(&d);
        assert!(text.contains("*PROPERTIES,2\n"));
    }

    #[test]
    fn net_roundtrip_through_emit() {
        let doc = parse(
            "LISA MODEL DESCRIPTION FORMAT 6.1\n====\n*NETLIST,2\nVDD,2\nVDD,GT\nCPU,PP,VDD\n",
        )
        .unwrap();
        let nets = doc.netlist().unwrap();
        assert_eq!(nets.len(), 1);
        let lines: Vec<String> = nets.iter().flat_map(super::net_to_lines).collect();
        assert_eq!(lines, vec!["VDD,2", "VDD,GT", "CPU,PP,VDD"]);
    }

    #[test]
    fn quote_roundtrip() {
        let p = super::Part {
            reference: "R1".into(),
            kind: "RES".into(),
            value: "9k1".into(),
            rest: vec!["PINSWAP=\"1,2\"".into()],
        };
        assert_eq!(
            super::part_to_line(&p),
            "R1,RES,9k1,\"PINSWAP=\"\"1,2\"\"\""
        );
    }
}
