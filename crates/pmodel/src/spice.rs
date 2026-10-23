//! SPICE deck parser for `.SML` subcircuits.
//!
//! Observed facts (`MODELS/ANALOG.SML` → `LM123`/`LM323A`/`LM323K`/`LM338`,
//! `MODELS/TEXAS.SML` → `LM2940T_5`/`LM2940T_10`, via [`crate::sml::split`]):
//! decks are `.SUBCKT <name> <nodes...>` … `.ENDS` with element lines
//! (`R`/`C`/`D`/`E`/`F`/`G`/`H`/`I`/`V`), `.MODEL` lines and `*` comments.
//!
//! v0 scope: single-`.SUBCKT` decks only; transmission lines, `J`/`K`/`L`/
//! `M`/`Q`/`T` devices and multi-`.SUBCKT` files return an error.
//!
//! # SPICE scales ([`parse_value`])
//!
//! Suffixes are case-insensitive: `T` = 1e12, `G` = 1e9, `MEG` = 1e6,
//! `K` = 1e3, `M` = 1e-3, `U` = 1e-6, `N` = 1e-9, `P` = 1e-12,
//! `F` = 1e-15, `MIL` = 25.4e-6. A plain exponent (`1E8`, `1E+06`)
//! needs no suffix; trailing unit text (`ohm`, `V`, `A`, `F`) is ignored.
//!
//! `M` vs `MEG` ambiguity (documented choice): bare `M`/`m` is **milli**
//! (1e-3) per SPICE3, mega requires the explicit `MEG`/`meg` form.
//! So `1M` = 0.001 while `1MEG` = 1_000_000. `MIL`/`mil` (25.4 µm) is
//! checked before `M` so `10MIL` is not read as 10 milli.

use std::fmt;

/// Element kind, from the first letter of the designator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Resistor.
    R,
    /// Capacitor.
    C,
    /// Diode (model only, no numeric value).
    D,
    /// Voltage-controlled voltage source (4 nodes + gain).
    E,
    /// Current-controlled current source (2 nodes + sense source + gain).
    F,
    /// Voltage-controlled current source (4 nodes + transconductance).
    G,
    /// Current-controlled voltage source (2 nodes + sense source + gain).
    H,
    /// Current source (2 nodes + value).
    I,
    /// Voltage source (2 nodes + value).
    V,
}

impl Kind {
    /// Classify by the first letter of `designator` (case-insensitive).
    #[must_use]
    pub fn from_designator(designator: &str) -> Option<Self> {
        match designator.chars().next()?.to_ascii_uppercase() {
            'R' => Some(Self::R),
            'C' => Some(Self::C),
            'D' => Some(Self::D),
            'E' => Some(Self::E),
            'F' => Some(Self::F),
            'G' => Some(Self::G),
            'H' => Some(Self::H),
            'I' => Some(Self::I),
            'V' => Some(Self::V),
            _ => None,
        }
    }
}

/// One SPICE element line.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Reference as written (`R1`, `R_YY`, `ECCX`, `CSTEP`).
    pub designator: String,
    /// Classified from the first letter.
    pub kind: Kind,
    /// Electrical nodes only (control sources stay in [`Self::params`]).
    pub nodes: Vec<String>,
    /// Numeric value when present: `R`/`C` value, `E`/`F`/`G`/`H` gain,
    /// `I`/`V` source value. `D` lines carry a model instead → [`None`].
    pub value: Option<f64>,
    /// Remaining tokens: model names (`DMOD`, `RQ`), control sources
    /// (`V3` for `F`/`H`), keywords (`DC`) and `K=V` pairs (`IC=0`).
    pub params: Vec<String>,
}

/// One `.MODEL name type params...` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Model name (`DMOD`, `RQ`).
    pub name: String,
    /// Model type as written (`D`, `RES`, `R`).
    pub kind: String,
    /// `K=V` / bare tokens after the type.
    pub params: Vec<String>,
}

/// One parsed `.SUBCKT` deck.
#[derive(Debug, Clone, PartialEq)]
pub struct Subckt {
    /// Subcircuit name from `.SUBCKT`.
    pub name: String,
    /// External nodes from `.SUBCKT`.
    pub nodes: Vec<String>,
    /// Element lines in file order.
    pub elements: Vec<Element>,
    /// `.MODEL` lines in file order.
    pub models: Vec<Model>,
}

impl Subckt {
    /// Find an element by designator (case-insensitive).
    #[must_use]
    pub fn find(&self, designator: &str) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.designator.eq_ignore_ascii_case(designator))
    }

    /// Count elements of one [`Kind`].
    #[must_use]
    pub fn count(&self, kind: Kind) -> usize {
        self.elements.iter().filter(|e| e.kind == kind).count()
    }
}

/// Deck parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No `.SUBCKT` header found.
    MissingSubckt,
    /// Malformed `.SUBCKT` line (payload is the line).
    BadSubckt(String),
    /// Malformed `.MODEL` line (payload is the line).
    BadModel(String),
    /// Malformed element line (payload is the line).
    BadElement(String),
    /// First letter outside `R`/`C`/`D`/`E`/`F`/`G`/`H`/`I`/`V`
    /// (payload is the designator).
    Unsupported(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSubckt => write!(f, "missing .SUBCKT header"),
            Self::BadSubckt(l) => write!(f, "bad .SUBCKT line: {l}"),
            Self::BadModel(l) => write!(f, "bad .MODEL line: {l}"),
            Self::BadElement(l) => write!(f, "bad element line: {l}"),
            Self::Unsupported(d) => write!(f, "unsupported element: {d}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a SPICE numeric literal with scale suffixes.
///
/// Accepts an optional sign, decimal digits, an optional `e` exponent and
/// one scale suffix (`T`/`G`/`MEG`/`K`/`M`/`U`/`N`/`P`/`F`/`MIL`,
/// case-insensitive); anything after the suffix (units) is ignored.
/// Returns [`None`] when the text does not start with a number
/// (e.g. model names like `RQ`, keywords like `DC`).
///
/// # Examples
///
/// ```
/// # use pmodel::spice::parse_value;
/// assert_eq!(parse_value("1E8"), Some(1e8));
/// assert_eq!(parse_value("10n"), Some(10e-9));
/// assert_eq!(parse_value("1u"), Some(1e-6));
/// assert_eq!(parse_value("1MEG"), Some(1e6));
/// assert_eq!(parse_value("1M"), Some(1e-3)); // milli, not mega
/// assert_eq!(parse_value("RQ"), None);
/// ```
#[must_use]
pub fn parse_value(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut pos = 0_usize;
    if bytes[pos] == b'+' || bytes[pos] == b'-' {
        pos += 1;
    }
    let mut digits = 0_usize;
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
        digits += 1;
    }
    if pos < bytes.len() && bytes[pos] == b'.' {
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if pos < bytes.len() && (bytes[pos] == b'e' || bytes[pos] == b'E') {
        let mut exp_end = pos + 1;
        if exp_end < bytes.len() && (bytes[exp_end] == b'+' || bytes[exp_end] == b'-') {
            exp_end += 1;
        }
        let exp_digits = exp_end;
        while exp_end < bytes.len() && bytes[exp_end].is_ascii_digit() {
            exp_end += 1;
        }
        if exp_end > exp_digits {
            pos = exp_end;
        }
    }
    let (num_str, suffix) = trimmed.split_at(pos);
    // Infix form `9k1`/`4u7`: scale letter between digit runs means
    // `9.1k`/`4.7u`. Without this the trailing digits silently vanish
    // (`9k1` -> 9000 instead of 9100). Bare `M` after digits is Proteus
    // mega (`4M7`); lowercase `m` stays SPICE milli (`4m7`).
    let mut suffix_chars = suffix.chars();
    if let Some(letter) = suffix_chars.next() {
        let rest: String = suffix_chars.collect();
        if letter.is_ascii_alphabetic()
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
        {
            if letter == 'M' {
                return parse_value(&format!("{num_str}.{rest}MEG"));
            }
            let dotted = format!("{num_str}.{rest}{letter}");
            return parse_value(&dotted);
        }
    }
    let num: f64 = num_str.parse().ok()?;
    let suffix = suffix.trim_start().to_ascii_lowercase();
    let mult = if suffix.starts_with("meg") {
        1e6
    } else if suffix.starts_with("mil") {
        25.4e-6
    } else if suffix.starts_with('t') {
        1e12
    } else if suffix.starts_with('g') {
        1e9
    } else if suffix.starts_with('k') {
        1e3
    } else if suffix.starts_with('m') {
        1e-3
    } else if suffix.starts_with('u') {
        1e-6
    } else if suffix.starts_with('n') {
        1e-9
    } else if suffix.starts_with('p') {
        1e-12
    } else if suffix.starts_with('f') {
        1e-15
    } else {
        1.0
    };
    Some(num * mult)
}

/// Join `+` continuation lines and drop `*` comments/blank lines.
///
/// A line whose first non-blank char is `+` is appended to the previous
/// logical line; `*` comment lines and empty (or NUL-only) lines are
/// dropped without breaking a pending continuation.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let cleaned: String = raw.chars().filter(|c| *c != '\0').collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        let left = cleaned.trim_start();
        if left.starts_with('*') {
            continue;
        }
        if let Some(cont) = left.strip_prefix('+') {
            if let Some(prev) = out.last_mut() {
                prev.push(' ');
                prev.push_str(cont.trim());
            }
            continue;
        }
        out.push(trimmed.to_owned());
    }
    out
}

/// Merge `KEY=` + `value` pairs split by whitespace (`TC1=` `0.5`).
fn merge_eq_tokens(tokens: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0_usize;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok.ends_with('=') && i + 1 < tokens.len() && !tokens[i + 1].contains('=') {
            out.push(format!("{tok}{}", tokens[i + 1]));
            i += 2;
        } else {
            out.push(tok.to_owned());
            i += 1;
        }
    }
    out
}

fn split_value_model(rest: &[&str]) -> (Option<f64>, Vec<String>) {
    if rest.is_empty() {
        return (None, Vec::new());
    }
    if let Some(v) = parse_value(rest[0]) {
        let params = rest[1..].iter().map(|s| (*s).to_owned()).collect();
        return (Some(v), params);
    }
    for (idx, tok) in rest.iter().enumerate().skip(1) {
        if let Some(v) = parse_value(tok) {
            let mut params: Vec<String> = rest[..idx].iter().map(|s| (*s).to_owned()).collect();
            params.extend(rest[idx + 1..].iter().map(|s| (*s).to_owned()));
            return (Some(v), params);
        }
    }
    (None, rest.iter().map(|s| (*s).to_owned()).collect())
}

fn parse_element(line: &str) -> Result<Element, ParseError> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let first = tokens
        .first()
        .ok_or_else(|| ParseError::BadElement(line.to_owned()))?;
    let designator = (*first).to_owned();
    let kind = Kind::from_designator(&designator)
        .ok_or_else(|| ParseError::Unsupported(designator.clone()))?;
    let node_count = match kind {
        Kind::R | Kind::C | Kind::D | Kind::I | Kind::V | Kind::F | Kind::H => 2,
        Kind::E | Kind::G => 4,
    };
    if tokens.len() < 1 + node_count {
        return Err(ParseError::BadElement(line.to_owned()));
    }
    let nodes: Vec<String> = tokens[1..=node_count]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let rest: &[&str] = &tokens[1 + node_count..];
    let (value, params) = match kind {
        Kind::R | Kind::C => split_value_model(rest),
        Kind::D => {
            if rest.is_empty() {
                return Err(ParseError::BadElement(line.to_owned()));
            }
            (None, rest.iter().map(|s| (*s).to_owned()).collect())
        }
        Kind::E | Kind::G => {
            let gain = rest
                .first()
                .and_then(|t| parse_value(t))
                .ok_or_else(|| ParseError::BadElement(line.to_owned()))?;
            let params = rest[1..].iter().map(|s| (*s).to_owned()).collect();
            (Some(gain), params)
        }
        Kind::F | Kind::H => {
            if rest.len() < 2 {
                return Err(ParseError::BadElement(line.to_owned()));
            }
            let gain =
                parse_value(rest[1]).ok_or_else(|| ParseError::BadElement(line.to_owned()))?;
            let mut params = vec![rest[0].to_owned()];
            params.extend(rest[2..].iter().map(|s| (*s).to_owned()));
            (Some(gain), params)
        }
        Kind::I | Kind::V => {
            let mut value: Option<f64> = None;
            let mut params: Vec<String> = Vec::new();
            for tok in rest {
                if value.is_none()
                    && let Some(v) = parse_value(tok)
                {
                    value = Some(v);
                } else {
                    params.push((*tok).to_owned());
                }
            }
            (value, params)
        }
    };
    Ok(Element {
        designator,
        kind,
        nodes,
        value,
        params,
    })
}

fn parse_model(line: &str) -> Result<Model, ParseError> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return Err(ParseError::BadModel(line.to_owned()));
    }
    Ok(Model {
        name: tokens[1].to_owned(),
        kind: tokens[2].to_owned(),
        params: merge_eq_tokens(&tokens[3..]),
    })
}

/// Parse one `.SUBCKT` deck into elements and models.
///
/// Comment (`*`) and blank lines are ignored, `+` continuations are
/// joined, unknown `.` directives are skipped, parsing stops at `.ENDS`.
///
/// # Errors
///
/// [`ParseError::MissingSubckt`] when no `.SUBCKT` header is present,
/// [`ParseError::BadSubckt`] on a malformed header or a second header,
/// [`ParseError::BadModel`]/[`ParseError::BadElement`] on malformed
/// lines, [`ParseError::Unsupported`] on unknown element letters.
pub fn parse(text: &str) -> Result<Subckt, ParseError> {
    let mut name: Option<String> = None;
    let mut nodes: Vec<String> = Vec::new();
    let mut elements: Vec<Element> = Vec::new();
    let mut models: Vec<Model> = Vec::new();

    for line in logical_lines(text) {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with(".SUBCKT") {
            if name.is_some() {
                return Err(ParseError::BadSubckt(line));
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 2 {
                return Err(ParseError::BadSubckt(line));
            }
            name = Some(tokens[1].to_owned());
            nodes = tokens[2..].iter().map(|s| (*s).to_owned()).collect();
        } else if upper.starts_with(".MODEL") {
            models.push(parse_model(&line)?);
        } else if upper.starts_with(".ENDS") || upper == ".END" {
            break;
        } else if !upper.starts_with('.') {
            elements.push(parse_element(&line)?);
        }
    }

    let Some(name) = name else {
        return Err(ParseError::MissingSubckt);
    };
    Ok(Subckt {
        name,
        nodes,
        elements,
        models,
    })
}

/// Parse every `.SUBCKT` deck in `text` (LML decks concatenate many).
///
/// Same grammar as [`parse`], but a second `.SUBCKT` header starts a new
/// deck instead of erroring. Enabler for `X` instances (match by deck name).
///
/// # Errors
///
/// [`ParseError::MissingSubckt`] when no header is present at all;
/// per-deck errors mirror [`parse`].
#[must_use]
pub fn parse_many(text: &str) -> Vec<Result<Subckt, ParseError>> {
    let mut out = Vec::new();
    let mut current = Vec::new();
    let mut seen = false;
    for line in logical_lines(text) {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with(".SUBCKT") && seen {
            out.push(parse(&current.join("\n")));
            current.clear();
            seen = false;
        }
        if upper.starts_with(".SUBCKT") {
            seen = true;
        }
        current.push(line);
    }
    if seen {
        out.push(parse(&current.join("\n")));
    } else if out.is_empty() {
        out.push(Err(ParseError::MissingSubckt));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC: &str = "\
.SUBCKT DEMO 1 2
* comment line
R1 1 2 1E8
R2 1 2 10n
R3 1 2 RQ 833.333
C1 1 2 1u
C2 1 2 0.00001 IC=0
D1 1 2 DMOD
E1 3 4 5 6 2.5
G1 3 4 5 6 0.1
F1 1 2 V3 1.5
H1 1 2 VSENS 2.22222
I1 1 2 DC 1E-3
V1 1 2 DC 0
V2 1 2 1.56
.MODEL DMOD D
.MODEL RQ RES TC1=0.0008
.ENDS DEMO
";

    #[test]
    fn synthetic_deck_kinds_and_values() {
        let sub = parse(SYNTHETIC).unwrap();
        assert_eq!(sub.name, "DEMO");
        assert_eq!(sub.nodes, vec!["1".to_owned(), "2".to_owned()]);
        assert_eq!(sub.elements.len(), 13);
        assert_eq!(sub.models.len(), 2);

        let r1 = sub.find("R1").unwrap();
        assert_eq!(r1.kind, Kind::R);
        assert_eq!(r1.nodes, vec!["1".to_owned(), "2".to_owned()]);
        assert!((r1.value.unwrap() - 1e8).abs() < 1.0);

        let r2 = sub.find("R2").unwrap();
        assert!((r2.value.unwrap() - 10e-9).abs() < 1e-18);

        // Model + value split.
        let r3 = sub.find("R3").unwrap();
        assert!((r3.value.unwrap() - 833.333).abs() < 1e-9);
        assert_eq!(r3.params, vec!["RQ".to_owned()]);

        let c1 = sub.find("C1").unwrap();
        assert!((c1.value.unwrap() - 1e-6).abs() < 1e-15);

        let c2 = sub.find("C2").unwrap();
        assert!((c2.value.unwrap() - 1e-5).abs() < 1e-12);
        assert_eq!(c2.params, vec!["IC=0".to_owned()]);

        let d1 = sub.find("D1").unwrap();
        assert_eq!(d1.value, None);
        assert_eq!(d1.params, vec!["DMOD".to_owned()]);

        let e1 = sub.find("E1").unwrap();
        assert_eq!(e1.nodes.len(), 4);
        assert!((e1.value.unwrap() - 2.5).abs() < 1e-12);

        let f1 = sub.find("F1").unwrap();
        assert_eq!(f1.nodes.len(), 2);
        assert!((f1.value.unwrap() - 1.5).abs() < 1e-12);
        assert_eq!(f1.params, vec!["V3".to_owned()]);

        let h1 = sub.find("H1").unwrap();
        assert!((h1.value.unwrap() - 2.222_22).abs() < 1e-9);
        assert_eq!(h1.params, vec!["VSENS".to_owned()]);

        let i1 = sub.find("I1").unwrap();
        assert!((i1.value.unwrap() - 1e-3).abs() < 1e-12);
        assert_eq!(i1.params, vec!["DC".to_owned()]);

        let v2 = sub.find("V2").unwrap();
        assert!((v2.value.unwrap() - 1.56).abs() < 1e-12);

        let m = sub.models.iter().find(|m| m.name == "RQ").unwrap();
        assert_eq!(m.kind, "RES");
        assert!(m.params.contains(&"TC1=0.0008".to_owned()));
    }

    #[test]
    fn infix_values_not_silently_truncated() {
        // Regression: infix `9k1` must be 9100, not 9000.
        assert_eq!(parse_value("9k1"), Some(9100.0));
        assert_eq!(parse_value("4u7"), Some(4.7e-6));
        assert_eq!(parse_value("2K2"), Some(2200.0));
        assert_eq!(parse_value("4M7"), Some(4.7e6));
        assert!((parse_value("4m7").unwrap() - 4.7e-3).abs() < 1e-12);
    }

    #[test]
    fn scales_meg_vs_milli_documented() {
        assert_eq!(parse_value("1MEG"), Some(1e6));
        assert_eq!(parse_value("1meg"), Some(1e6));
        assert_eq!(parse_value("1M"), Some(1e-3));
        assert_eq!(parse_value("1m"), Some(1e-3));
        assert_eq!(parse_value("1K"), Some(1e3));
        assert_eq!(parse_value("1k"), Some(1e3));
        assert_eq!(parse_value("1G"), Some(1e9));
        assert_eq!(parse_value("1T"), Some(1e12));
        assert_eq!(parse_value("1U"), Some(1e-6));
        assert_eq!(parse_value("1N"), Some(1e-9));
        assert_eq!(parse_value("1P"), Some(1e-12));
        assert_eq!(parse_value("1F"), Some(1e-15));
        assert_eq!(parse_value("1E8"), Some(1e8));
        assert_eq!(parse_value("7.94228E+06"), Some(7.942_28e6));
        assert_eq!(parse_value("DC"), None);
        assert_eq!(parse_value("RQ"), None);
        assert_eq!(parse_value(""), None);
    }

    #[test]
    fn parse_many_splits_decks() {
        let text = ".SUBCKT A 1 2\nR1 1 2 1K\n.ENDS\n.SUBCKT B 1\nC1 1 0 1u\n.ENDS\n";
        let decks = parse_many(text);
        assert_eq!(decks.len(), 2);
        assert_eq!(decks[0].as_ref().unwrap().name, "A");
        assert_eq!(decks[1].as_ref().unwrap().name, "B");
        assert!(parse_many("R1 1 2 10\n")[0].is_err());
    }

    #[test]
    fn rejects_missing_subckt_and_unknown_kind() {
        assert_eq!(parse("R1 1 2 10\n").unwrap_err(), ParseError::MissingSubckt);
        let bad = parse(".SUBCKT X 1 2\nQ1 1 2 3 BOGUS\n.ENDS\n").unwrap_err();
        assert_eq!(bad, ParseError::Unsupported("Q1".to_owned()));
        assert!(parse(".SUBCKT X 1 2\nR1 1\n.ENDS\n").is_err());
    }

    #[test]
    fn continuation_and_comments() {
        let sub = parse(".SUBCKT X 1 2\n* hello\nR1 1 2 1K\n+ TC=1\n.ENDS\n").unwrap();
        assert_eq!(sub.elements.len(), 1);
        let r1 = &sub.elements[0];
        assert!((r1.value.unwrap() - 1e3).abs() < 1e-9);
        assert_eq!(r1.params, vec!["TC=1".to_owned()]);
    }

    const ANALOG_SML: &str = "/home/diez/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/MODELS/ANALOG.SML";
    const TEXAS_SML: &str = "/home/diez/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/MODELS/TEXAS.SML";

    fn parse_all(path: &str) -> Option<Vec<Subckt>> {
        let Ok(data) = std::fs::read(path) else {
            return None;
        };
        let Ok(decks) = crate::sml::split(&data) else {
            return None;
        };
        let mut out = Vec::new();
        for d in &decks {
            out.push(parse(&d.text).ok()?);
        }
        Some(out)
    }

    /// Shipped samples. Skipped (not failed) when absent so the suite
    /// stays green without the wine prefix.
    #[test]
    fn real_six_decks_element_counts() {
        let Some(analog) = parse_all(ANALOG_SML) else {
            return;
        };
        let Some(texas) = parse_all(TEXAS_SML) else {
            return;
        };
        assert_eq!(analog.len(), 4);
        assert_eq!(texas.len(), 2);

        // LM123 family: 62 elements each (13E/9V/8F/2H/15R/4C/2I/9D).
        for sub in &analog[..3] {
            assert!(sub.elements.len() > 5, "{}", sub.name);
            assert_eq!(sub.elements.len(), 62, "{}", sub.name);
            assert_eq!(sub.models.len(), 4, "{}", sub.name);
            for kind in [
                Kind::R,
                Kind::C,
                Kind::D,
                Kind::E,
                Kind::F,
                Kind::H,
                Kind::I,
                Kind::V,
            ] {
                assert!(sub.count(kind) > 0, "{} {kind:?}", sub.name);
            }
            assert_eq!(sub.count(Kind::E), 13, "{}", sub.name);
            assert_eq!(sub.count(Kind::V), 9, "{}", sub.name);
            assert_eq!(sub.count(Kind::F), 8, "{}", sub.name);
            assert_eq!(sub.count(Kind::H), 2, "{}", sub.name);
            assert_eq!(sub.count(Kind::R), 15, "{}", sub.name);
            assert_eq!(sub.count(Kind::C), 4, "{}", sub.name);
            assert_eq!(sub.count(Kind::I), 2, "{}", sub.name);
            assert_eq!(sub.count(Kind::D), 9, "{}", sub.name);
        }
        let lm123 = &analog[0];
        assert_eq!(lm123.name, "LM123");
        assert_eq!(
            lm123.nodes,
            vec!["3".to_owned(), "1".to_owned(), "2".to_owned()]
        );
        for d in ["E1", "V1", "F1", "R1", "C1", "I1", "D1"] {
            assert!(lm123.find(d).is_some(), "LM123 missing {d}");
        }
        let r1 = lm123.find("R1").unwrap();
        assert!((r1.value.unwrap() - 1e8).abs() < 1.0);

        // LM338 + LM2940 pair: 66 elements each (13E/9V/8F/2H/19R/4C/2I/9D).
        let rest: Vec<&Subckt> = analog[3..].iter().chain(texas.iter()).collect();
        assert_eq!(rest.len(), 3);
        for sub in &rest {
            assert!(sub.elements.len() > 5, "{}", sub.name);
            assert_eq!(sub.elements.len(), 66, "{}", sub.name);
            assert_eq!(sub.models.len(), 4, "{}", sub.name);
            assert_eq!(sub.count(Kind::R), 19, "{}", sub.name);
            assert_eq!(sub.count(Kind::C), 4, "{}", sub.name);
        }
        assert_eq!(rest[0].name, "LM338");
        assert_eq!(rest[1].name, "LM2940T_5");
        assert_eq!(rest[2].name, "LM2940T_10");
    }
}
