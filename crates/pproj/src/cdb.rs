//! Reader for Proteus `ROOT.CDB` component databases (v0).
//!
//! Observed facts (`SAMPLES/Tutorials/Isistut.pdsprj`, `ROOT.CDB`, 5371 B,
//! corroborated by the public `KiCad` importer write-up):
//! - Header of four little-endian `u32` (first observed `4`, second `1`).
//! - Component records: `u32` length, `u8`-prefixed name, `u32` pin count,
//!   then per pin a `(name, number)` pair of `u8`-prefixed ASCII strings
//!   (number may be empty), e.g. `U1` with
//!   `("POS IP","3") ("NEG IP","2") ("OP","6") ...`.
//! - Record section as `{FIELD=value}` text lines (like
//!   `{UE,CAPACITOR}`, `{CODE=WX51F}`, `{PACKAGE=CAP20}`).
//! - Sheet names appear as length-prefixed strings near the head (`ROOT`).
//!
//! v0 scope: header + heuristic component scan + brace-record extraction.
//! Pin coordinates and record separators are pending deeper RE.

/// One length-prefixed string with its absolute file offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PString {
    /// Offset of the length byte.
    pub offset: usize,
    /// Decoded text.
    pub text: String,
}

/// One component entry: sequence number + refdes + pin table.
///
/// Verified on real `ROOT.CDB` (`Isistut.pdsprj`, 5371 B): records are
/// `u32 seq | PString name | u32 pin-count | 2N PStrings | footer` where
/// `seq` increments per record (`0x28, 0x29, 0x2a, …` — a sequence
/// number, NOT a VFILE length), pin tables are plain `(name, number)`
/// pairs (e.g. 7-pin `U1`: `POS IP=3 NEG IP=2 OP=6 V+=7 V-=4 ON1=1
/// ON2=5`), and footers hold `00`-padding + flags + `ffffffff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// Record sequence number (`u32` immediately before the name).
    pub seq: u32,
    /// Reference designator (`U1`, `R3`).
    pub name: String,
    /// `(pin name, pin number)` pairs; number may be empty.
    pub pins: Vec<(String, String)>,
}

/// Parsed database (best effort).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cdb {
    /// First four header `u32` values, little-endian.
    pub header: [u32; 4],
    /// Components found by structural scan.
    pub components: Vec<Component>,
    /// `{...}` text records.
    pub records: Vec<String>,
}

/// Read a little-endian `u32`.
fn u32le(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a `u8`-length-prefixed ASCII string at `at`.
///
/// Empty strings are rejected unless `allow_empty` (pin numbers use a
/// bare `0x00` byte for "no number").
fn pstring(data: &[u8], at: usize, allow_empty: bool) -> Option<(PString, usize)> {
    let len = *data.get(at)? as usize;
    if len == 0 && !allow_empty {
        return None;
    }
    if len > 64 {
        return None;
    }
    let bytes = data.get(at + 1..at + 1 + len)?;
    if !bytes
        .iter()
        .all(|b| *b == 0x09 || (0x20..=0x7E).contains(b))
    {
        return None;
    }
    let text = String::from_utf8_lossy(bytes).into_owned();
    Some((PString { offset: at, text }, at + 1 + len))
}

/// Heuristic component probe at `at`: `PString + u32 n<=128 + 2N PStrings`.
/// Pushes on success. Returns the offset just past the pins.
fn scan_component(data: &[u8], at: usize, out: &mut Vec<Component>) -> Option<usize> {
    let (name, after_name) = pstring(data, at, false)?;
    let n = u32le(data, after_name)?;
    if n > 128 {
        return None;
    }
    let mut pins = Vec::new();
    let mut j = after_name + 4;
    for _ in 0..n {
        let (pn, j1) = pstring(data, j, false)?;
        let (num, j2) = pstring(data, j1, true)?;
        pins.push((pn.text, num.text));
        j = j2;
    }
    if pins.is_empty() {
        return None;
    }
    out.push(Component {
        seq: u32le(data, at.saturating_sub(4)).unwrap_or(0),
        name: name.text,
        pins,
    });
    Some(j)
}

/// Parse a database blob. Never fails outright; unknown regions are skipped.
#[must_use]
pub fn parse(data: &[u8]) -> Cdb {
    let mut header = [0u32; 4];
    for (i, h) in header.iter_mut().enumerate() {
        *h = u32le(data, i * 4).unwrap_or(0);
    }

    let mut components = Vec::new();
    let mut i = 16usize;
    while i < data.len() {
        // Records: `u32 seq | PString name | u32 pin-count | 2N PStrings |
        // footer`. The scanner validates structure; `seq` is captured
        // from the 4 bytes before the name.
        if let Some(past) = scan_component(data, i, &mut components) {
            i = past;
        } else {
            i += 1;
        }
    }

    // Brace records, same shape as DSN blocks.
    let mut records = Vec::new();
    let mut k = 0usize;
    while k < data.len() {
        if data[k] != b'{' {
            k += 1;
            continue;
        }
        if let Some(end) = data[k..].iter().position(|&b| b == b'}') {
            let head = String::from_utf8_lossy(&data[k + 1..k + end]).into_owned();
            if !head.is_empty()
                && head.len() <= 128
                && head.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(
                            b,
                            b'*' | b'$' | b'_' | b'-' | b'=' | b' ' | b'/' | b'.' | b','
                        )
                })
            {
                records.push(head);
            }
            k += end + 1;
        } else {
            break;
        }
    }

    Cdb {
        header,
        components,
        records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic record mirroring the observed `U1` layout.
    fn sample() -> Vec<u8> {
        let mut v = vec![4u8, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&[0x13, 0, 0, 0]); // seq (record sequence number)
        v.push(2);
        v.extend_from_slice(b"U1");
        v.extend_from_slice(&[2u8, 0, 0, 0]); // 2 pins
        v.push(6);
        v.extend_from_slice(b"POS IP");
        v.push(1);
        v.extend_from_slice(b"3");
        v.push(1);
        v.extend_from_slice(b"1");
        v.push(0); // empty number
        v.extend_from_slice(b"{UE,OPAMP}\n");
        v
    }

    #[test]
    fn header_and_component() {
        let cdb = parse(&sample());
        assert_eq!(cdb.header, [4, 1, 1, 0]);
        assert_eq!(cdb.components.len(), 1);
        let c = &cdb.components[0];
        assert_eq!(c.name, "U1");
        assert_eq!(
            c.pins,
            vec![
                ("POS IP".to_owned(), "3".to_owned()),
                ("1".to_owned(), String::new()),
            ]
        );
        assert_eq!(cdb.records, vec!["UE,OPAMP".to_owned()]);
    }

    #[test]
    fn empty_is_empty() {
        let cdb = parse(&[]);
        assert_eq!(cdb.components.len(), 0);
        assert_eq!(cdb.records.len(), 0);
        assert_eq!(cdb.header, [0, 0, 0, 0]);
    }

    #[test]
    fn two_components_and_records() {
        let mut v = vec![4u8, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&[0x0f, 0, 0, 0]); // seq (record sequence number)
        v.push(2);
        v.extend_from_slice(b"R1");
        v.extend_from_slice(&[2u32.to_le_bytes()].concat());
        for (n, p) in [("A", "1"), ("B", "2")] {
            v.push(u8::try_from(n.len()).unwrap());
            v.extend_from_slice(n.as_bytes());
            v.push(u8::try_from(p.len()).unwrap());
            v.extend_from_slice(p.as_bytes());
        }
        v.extend_from_slice(b"{PACKAGE=RES40}{CODE=WX51F}");
        let cdb = parse(&v);
        assert_eq!(cdb.components.len(), 1);
        assert_eq!(cdb.components[0].name, "R1");
        assert_eq!(
            cdb.components[0].pins,
            vec![
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(
            cdb.records,
            vec!["PACKAGE=RES40".to_owned(), "CODE=WX51F".to_owned()]
        );
    }
}
