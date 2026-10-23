//! Parsers for the Proteus LISA-family text formats.
//!
//! Observed facts (Proteus 8.6 SP2, shipped samples):
//! - `MODELS/12ce51x.MDF` starts `LISA MODEL DESCRIPTION FORMAT 6.1`.
//! - `SAMPLES/Tutorials/Ppsu.SDF` starts `ISIS SCHEMATIC DESCRIPTION FORMAT 4.5`.
//! - Both share the layout: title line, `===` rule, `Key: value` preamble,
//!   then `*SECTION,count` blocks (`*PROPERTIES`, `*MODELDEFS`,
//!   `*PARTLIST`, `*NETLIST`, `*GATES`).
//! - `*PARTLIST` rows: `REF,VALUE,MODEL,k=v,...` (e.g.
//!   `R1,RES,9k1,EID=6,PACKAGE=RES40,PINSWAP="1,2"`).
//! - `*NETLIST` rows: `#netid,npins` followed by `REF,PIN,n` pin lines.
//!
//! v0 scope: preamble + section split + typed `PARTLIST`/`NETLIST`.
//! Library containers (`.SML` SPICE decks, `.LML` model descriptions)
//! split in [`sml`] + [`lml`] over the shared [`container`] layout;
//! `.SUBCKT`/`.MODEL`/element grammar in [`spice`].

use std::collections::HashMap;
use std::fmt;

/// Shared `.SML`/`.LML` library container splitter.
pub mod container;
pub mod emit;
/// Model library (`.LML`) container splitter.
pub mod lml;
pub mod sml;
/// SPICE deck parser (`.SUBCKT`/`.MODEL`/elements with scales).
pub mod spice;

/// LISA parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// First line is not a `<NAME> FORMAT <VERSION>` title.
    BadTitle,
    /// A `*SECTION,count` line has no comma or a non-numeric count.
    BadSection(String),
    /// A `*PARTLIST` row has fewer than 2 fields.
    BadPart(String),
    /// A `*NETLIST` pin line has fewer than 3 fields.
    BadPin(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadTitle => write!(f, "bad LISA title line"),
            Self::BadSection(l) => write!(f, "bad section header: {l}"),
            Self::BadPart(l) => write!(f, "bad partlist row: {l}"),
            Self::BadPin(l) => write!(f, "bad netlist pin: {l}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Whole parsed document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// e.g. `LISA MODEL DESCRIPTION` or `ISIS SCHEMATIC DESCRIPTION`.
    pub format: String,
    /// e.g. `6.1` or `4.5`.
    pub version: String,
    /// Preamble fields (`Design`, `Author`, ...).
    pub preamble: HashMap<String, String>,
    /// Sections in file order.
    pub sections: Vec<Section>,
}

/// One `*NAME,count` block with its raw body lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Section name without `*` (e.g. `PARTLIST`).
    pub name: String,
    /// Declared row count (advisory; not enforced).
    pub count: usize,
    /// Tokens after the count (e.g. `MAPPINGS` map-table name from
    /// `MAP ON COMPARATOR`, `PARTLIST` part kind). May be empty.
    pub extra: String,
    /// Body lines, trimmed, empties dropped.
    pub lines: Vec<String>,
}

/// One `*PARTLIST` entry: `REF,TYPE,VALUE,extra...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Reference designator (`R1`, `U1`, `CPU`).
    pub reference: String,
    /// Component type (`RES`, `CAP`, `PIC12C51X`).
    pub kind: String,
    /// Value (`9k1`, `100n`, `<CLOCK>`).
    pub value: String,
    /// Remaining comma fields (`MODEL`, `EID=6`, `PINSWAP="1,2"`, ...).
    pub rest: Vec<String>,
}

/// One `*MODELDEFS` entry (`NAME : params` or `NAME,params...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDef {
    /// Definition name (`TTL`, `CMOS`, ...).
    pub name: String,
    /// Raw parameter text after `:` or `,`.
    pub params: String,
}

/// One `*NETLIST` net.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    /// Net id as written (`#00000`).
    pub id: String,
    /// Declared pin count (advisory).
    pub pins_declared: usize,
    /// `(reference, pin, extra)` triples.
    pub pins: Vec<(String, String, String)>,
}

/// Top-level section names. Anything else starting with `*` while inside
/// `*NETLIST` is a per-net sub-header (`*VCC,5`, `*VCC,GT`) kept verbatim
/// in the section body for [`Document::netlist`] to interpret.
const TOP_SECTIONS: &[&str] = &[
    "PROPERTIES",
    "MODELDEFS",
    "PARTLIST",
    "NETLIST",
    "GATES",
    "MAPPINGS",
    "SCRIPT",
];

/// Parse a full LISA-family document.
///
/// # Errors
///
/// [`ParseError::BadTitle`] when the first line is not a title,
/// [`ParseError::BadSection`] on a malformed section header.
pub fn parse(text: &str) -> Result<Document, ParseError> {
    let mut lines = text.lines().map(str::trim_end);
    let first = lines.next().ok_or(ParseError::BadTitle)?.trim();
    // SPICELNK variants start directly with `Design:` (no title line).
    let (format, version, preamble_first) = match split_title(first) {
        Some((f, v)) => (f, v, None),
        None if first.contains(':') => (String::new(), String::new(), Some(first)),
        None => return Err(ParseError::BadTitle),
    };

    let mut preamble = HashMap::new();
    let mut sections = Vec::new();
    let mut current: Option<Section> = None;

    let mut first_line = preamble_first;
    loop {
        let line = match first_line.take() {
            Some(l) => l,
            None => match lines.next() {
                Some(l) => l.trim(),
                None => break,
            },
        };
        if line.is_empty() || line.starts_with('=') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('*') {
            let (name, _) = rest.split_once(',').unwrap_or((rest, ""));
            let name = name.trim();
            // `*ENDSCRIPT` closes a `*SCRIPT` block (vendor `NETLIST::write`).
            if name == "ENDSCRIPT" {
                if let Some(sec) = current.take() {
                    sections.push(sec);
                }
                continue;
            }
            let in_netlist = current.as_ref().is_some_and(|s| s.name == "NETLIST");
            if !TOP_SECTIONS.contains(&name) {
                if in_netlist {
                    // Per-net sub-header (`*VCC,5`, `*VCC,GT`): body content.
                    if let Some(sec) = current.as_mut() {
                        sec.lines.push(line.to_owned());
                    }
                    continue;
                }
                return Err(ParseError::BadSection(line.to_owned()));
            }
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            // Header: `NAME[,count[,extra...]]` — count is the first token,
            // the rest (e.g. MAPPINGS map name) is kept verbatim.
            let (name, after) = match rest.split_once(',') {
                Some((n, a)) => (n, a),
                None => (rest, ""),
            };
            let mut toks = after.split(',');
            let count_str = toks.next().unwrap_or("").trim();
            let count = if count_str.is_empty() || count_str == "XXXXX" {
                0
            } else {
                count_str
                    .parse()
                    .map_err(|_| ParseError::BadSection(line.to_owned()))?
            };
            let extra: Vec<&str> = toks.map(str::trim).collect();
            current = Some(Section {
                name: name.trim().to_owned(),
                count,
                extra: extra.join(","),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(sec) = current.as_mut() {
            if !line.is_empty() {
                sec.lines.push(line.to_owned());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            preamble.insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    if let Some(sec) = current {
        sections.push(sec);
    }

    Ok(Document {
        format,
        version,
        preamble,
        sections,
    })
}

fn split_title(title: &str) -> Option<(String, String)> {
    let (name, ver) = title.rsplit_once("FORMAT")?;
    let (name, ver) = (name.trim(), ver.trim());
    if name.is_empty() || ver.is_empty() {
        return None;
    }
    Some((name.to_owned(), ver.to_owned()))
}

/// Split a row on commas, ignoring commas inside `"..."`.
fn split_csv(row: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in row.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ',' if !quoted => {
                fields.push(cur.trim().to_owned());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    fields.push(cur.trim().to_owned());
    fields
}

impl Document {
    /// Find a section by name.
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Parse `*PROPERTIES` as `KEY=value` pairs.
    #[must_use]
    pub fn properties(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        if let Some(sec) = self.section("PROPERTIES") {
            for line in &sec.lines {
                if let Some((k, v)) = line.split_once('=') {
                    out.insert(k.trim().to_owned(), v.trim().to_owned());
                }
            }
        }
        out
    }

    /// Parse `*PARTLIST` rows (`REF,TYPE,VALUE,extra...`).
    ///
    /// # Errors
    ///
    /// [`ParseError::BadPart`] on rows with fewer than 3 fields.
    /// Parse `*PARTLIST` rows (`REF,TYPE,VALUE,extra...`).
    ///
    /// Vendor `read_parts` never fails a row: short rows get empty
    /// fields (`VALUE` defaulted when absent). We mirror that tolerance.
    pub fn partlist(&self) -> Result<Vec<Part>, ParseError> {
        let mut out = Vec::new();
        if let Some(sec) = self.section("PARTLIST") {
            for line in &sec.lines {
                let fields = split_csv(line);
                if fields.is_empty() {
                    return Err(ParseError::BadPart(line.clone()));
                }
                let get = |i: usize| fields.get(i).cloned().unwrap_or_default();
                out.push(Part {
                    reference: get(0),
                    kind: get(1),
                    value: get(2),
                    rest: fields.get(3..).unwrap_or(&[]).to_vec(),
                });
            }
        }
        Ok(out)
    }

    /// Parse `*MODELDEFS` rows (`NAME : params` or `NAME,params...`).
    #[must_use]
    pub fn modeldefs(&self) -> Vec<ModelDef> {
        let mut out = Vec::new();
        if let Some(sec) = self.section("MODELDEFS") {
            for line in &sec.lines {
                if let Some((name, params)) = line.split_once(':') {
                    out.push(ModelDef {
                        name: name.trim().to_owned(),
                        params: params.trim().to_owned(),
                    });
                } else {
                    let fields = split_csv(line);
                    if let Some((name, rest)) = fields.split_first() {
                        out.push(ModelDef {
                            name: name.clone(),
                            params: rest.join(","),
                        });
                    }
                }
            }
        }
        out
    }

    /// Parse `*NETLIST` nets.
    ///
    /// Net headers are `ID,count` lines (`#00000,2`, named `UNREG,3`, or
    /// `*VCC,5` sub-headers); pin lines are `REF,KIND` or `REF,KIND,PIN...`
    /// (`GP5/OSC1,GT` is a pin, `VDD,2` a header). A `*ID,GT` marker that
    /// repeats the current net id is skipped, not treated as a new net.
    ///
    /// # Errors
    ///
    /// [`ParseError::BadPin`] on lines with fewer than 2 fields.
    pub fn netlist(&self) -> Result<Vec<Net>, ParseError> {
        let mut out = Vec::new();
        if let Some(sec) = self.section("NETLIST") {
            let mut current: Option<Net> = None;
            for line in &sec.lines {
                // Per-net sub-headers keep their leading `*` (`*VCC,5`).
                // `*ID,GT` markers carry no pin data and are skipped.
                let starred = line.starts_with('*');
                let body = line.strip_prefix('*').unwrap_or(line);
                let fields = split_csv(body);
                if fields.len() == 2 && fields[1].parse::<usize>().is_ok() {
                    if let Some(net) = current.take() {
                        out.push(net);
                    }
                    current = Some(Net {
                        id: fields[0].clone(),
                        pins_declared: fields[1].parse().unwrap_or(0),
                        pins: Vec::new(),
                    });
                } else if !starred {
                    if fields.len() >= 2 {
                        if let Some(net) = current.as_mut() {
                            net.pins.push((
                                fields[0].clone(),
                                fields[1].clone(),
                                fields.get(2..).unwrap_or(&[]).join(","),
                            ));
                        }
                    } else {
                        return Err(ParseError::BadPin(line.clone()));
                    }
                }
                // Starred non-header lines (`*ID,GT`) carry no pins.
            }
            if let Some(net) = current {
                out.push(net);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn vendor_write_quirks_mappings_and_backpatch() {
        // `*MAPPINGS,` (no count) and `*MODELDEFS,XXXXX` (un-backpatched).
        let doc = parse(
            "LISA MODEL DESCRIPTION FORMAT 6.1\n====\n\
             *PROPERTIES,1\nTRACE=1\n\
             *MAPPINGS,\nA=1\n\
             *MODELDEFS,XXXXX\nTTL : RHI=50\n\
             *SCRIPT\nLINE1\n*ENDSCRIPT\n\
             *PARTLIST,0\n*NETLIST,0\n",
        )
        .unwrap();
        assert_eq!(doc.section("MAPPINGS").unwrap().count, 0);
        assert_eq!(doc.section("MODELDEFS").unwrap().count, 0);
        assert!(doc.section("SCRIPT").is_some());
    }

    use super::*;

    const SAMPLE: &str = "ISIS SCHEMATIC DESCRIPTION FORMAT 4.5
=====================================
Design:   PPSU DESIGN
Author:   Labcenter

*PROPERTIES,0

*PARTLIST,2
R1,RES,9k1,EID=6,PACKAGE=RES40,PINSWAP=\"1,2\"
U1,CA3140,CA3140,EID=1,PACKAGE=DIL08

*NETLIST,3
#00000,2
Q1,PS,1
U1,OP,6
";

    #[test]
    fn title_and_preamble() {
        let doc = parse(SAMPLE).unwrap();
        assert_eq!(doc.format, "ISIS SCHEMATIC DESCRIPTION");
        assert_eq!(doc.version, "4.5");
        assert_eq!(doc.preamble["Design"], "PPSU DESIGN");
        assert_eq!(doc.preamble["Author"], "Labcenter");
    }

    #[test]
    fn partlist_short_rows_tolerated_like_vendor() {
        // read_parts never fails a row: missing fields default to empty.
        let doc =
            parse("LISA MODEL DESCRIPTION FORMAT 6.1\n====\n*PARTLIST,2\nR1,RES\nU1\n").unwrap();
        let p = doc.partlist().unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].value, "");
        assert_eq!(p[1].kind, "");
    }

    #[test]
    fn partlist_rows() {
        let parts = parse(SAMPLE).unwrap().partlist().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].reference, "R1");
        assert_eq!(parts[0].kind, "RES");
        assert_eq!(parts[0].value, "9k1");
        assert!(parts[0].rest.contains(&"PACKAGE=RES40".to_owned()));
        // Quoted comma stays inside one field.
        assert!(parts[0].rest.contains(&"PINSWAP=\"1,2\"".to_owned()));
        assert_eq!(parts[0].rest.len(), 3);
    }

    #[test]
    fn modeldefs_variants() {
        let doc = parse("LISA MODEL DESCRIPTION FORMAT 6.1\n====\n*MODELDEFS,2\nTTL    : RHI=50, RLO=5\nCMOS,RHI=1M\n").unwrap();
        let defs = doc.modeldefs();
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "TTL");
        assert!(defs[0].params.contains("RHI=50"));
        assert_eq!(defs[1].name, "CMOS");
    }

    #[test]
    fn netlist_nets() {
        let nets = parse(SAMPLE).unwrap().netlist().unwrap();
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].id, "#00000");
        assert_eq!(nets[0].pins.len(), 2);
        assert_eq!(nets[0].pins[0].0, "Q1");
    }

    #[test]
    fn netlist_named_nets_and_short_pins() {
        let doc = parse("LISA MODEL DESCRIPTION FORMAT 6.1\n====\n*NETLIST,2\nVDD,2\nVDD,GT\nCPU,PP,VDD\nUNREG,1\nQ1,PS,2\n").unwrap();
        let nets = doc.netlist().unwrap();
        assert_eq!(nets.len(), 2);
        assert_eq!(nets[0].id, "VDD");
        assert_eq!(
            nets[0].pins[0],
            ("VDD".to_owned(), "GT".to_owned(), String::new())
        );
        assert_eq!(nets[1].id, "UNREG");
        assert_eq!(nets[1].pins.len(), 1);
    }

    #[test]
    fn mappings_and_starred_net_subheaders() {
        let doc = parse("LISA MODEL DESCRIPTION FORMAT 6.1\n====\n*MODELDEFS,1\nTTL : RHI=50, RLO=5\n*MAPPINGS,2,COMPARATOR\nA=1\nB=2\n*NETLIST,2\n*VCC,1\n*VCC,GT\n*GND,1\nGND,GT\n").unwrap();
        assert_eq!(doc.section("MAPPINGS").unwrap().lines, vec!["A=1", "B=2"]);
        // Map-table name from `MAP ON COMPARATOR` survives the header.
        assert_eq!(doc.section("MAPPINGS").unwrap().extra, "COMPARATOR");
        assert_eq!(doc.modeldefs()[0].name, "TTL");
        let nets = doc.netlist().unwrap();
        assert_eq!(nets.len(), 2);
        assert_eq!(nets[0].id, "VCC");
        assert_eq!(nets[0].pins, Vec::new());
        assert_eq!(nets[1].id, "GND");
    }

    #[test]
    fn bad_title_rejected() {
        assert_eq!(parse("garbage\n").unwrap_err(), ParseError::BadTitle);
        assert_eq!(parse("FORMAT\n").unwrap_err(), ParseError::BadTitle);
        assert_eq!(parse("X FORMAT \n").unwrap_err(), ParseError::BadTitle);
        assert_eq!(parse("X FORMAT\n").unwrap_err(), ParseError::BadTitle);
    }

    #[test]
    fn mdf_title_variant() {
        let doc = parse("LISA MODEL DESCRIPTION FORMAT 6.1\n====\n").unwrap();
        assert_eq!(doc.format, "LISA MODEL DESCRIPTION");
        assert_eq!(doc.version, "6.1");
    }

    #[test]
    fn reg_preamble_doc_no_y_secciones_vacias_con_espacios() {
        let doc = parse("ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n=====================================\nDesign:   PPSU DESIGN\nDoc. no.: <NONE>\nRevision: 3\nAuthor:   Labcenter\nCreated:  Fri Jun  1 1990\nModified: 20/11/98\n*PROPERTIES,0    \n*MODELDEFS,0    \n*GATES,0    \n").unwrap();
        assert_eq!(doc.preamble["Doc. no."], "<NONE>");
        assert_eq!(doc.preamble["Created"], "Fri Jun  1 1990");
        assert_eq!(doc.preamble["Modified"], "20/11/98");
        assert_eq!(
            doc.section("PROPERTIES").unwrap().lines,
            Vec::<String>::new()
        );
        assert_eq!(doc.modeldefs(), Vec::new());
        assert_eq!(doc.section("GATES").unwrap().lines, Vec::<String>::new());
    }

    #[test]
    fn reg_partlist_tipo_igual_valor_y_pinswap_con_comillas() {
        let doc = parse("ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n====\n*PARTLIST,2\nD1,OA91,OA91,EID=8,PACKAGE=DIODE30\nC1,CAP,100n,EID=2,PACKAGE=CAP10,PINSWAP=\"1,2\"\n").unwrap();
        let p = doc.partlist().unwrap();
        assert_eq!(p[0].reference, "D1");
        assert_eq!(p[0].kind, "OA91");
        assert_eq!(p[0].value, "OA91");
        assert_eq!(p[1].value, "100n");
        assert!(p[1].rest.contains(&"PINSWAP=\"1,2\"".to_owned()));
    }

    #[test]
    fn reg_netlist_nombrada_con_marcador_y_pin_letra() {
        let doc = parse("ISIS SCHEMATIC DESCRIPTION FORMAT 4.5\n====\n*NETLIST,2\nVOUT,4\nVOUT,OT\nQ1,PS,3\nR1,PS,1\nD1,PS,A\nUNREG,3\nUNREG,GT\nQ1,PS,2\nU1,PP,7\n").unwrap();
        let n = doc.netlist().unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].id, "VOUT");
        assert_eq!(n[0].pins_declared, 4);
        assert_eq!(
            n[0].pins[0],
            ("VOUT".to_owned(), "OT".to_owned(), String::new())
        );
        assert_eq!(
            n[0].pins[3],
            ("D1".to_owned(), "PS".to_owned(), "A".to_owned())
        );
        assert_eq!(
            n[1].pins[0],
            ("UNREG".to_owned(), "GT".to_owned(), String::new())
        );
    }
}
