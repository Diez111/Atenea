//! Token extractor for Proteus `.DSN` / `.LYT` binaries.
//!
//! Observed facts (`SAMPLES/.../siggen.DSN`, 126 KB):
//! - Magic `ISIS SCHEMATIC FILE\x1A` (layout: `PCB LAYOUT FILE\x1A`).
//! - Text blocks `{*DEVICE}`, `{*COMPONENT}`, `{*PROPDEFS}`, `{*USER}`...
//!   embedded in binary coordinate data.
//! - Wire records: `WIRE 00 00 00 <n> 00` + `n` points of
//!   `(x, y)` little-endian `i32` (`BUS WIRE` shares the layout).
//!   All segments are Manhattan; lengths are multiples of `25_400` units
//!   (0.01 inch), modal segment `254_000` units = 0.1 inch.
//! - Scale: `2_540_000` units per inch (1 unit = 10 nm). The older
//!   "2540 → 1 unit = 10 um" guess is refuted: it would make a
//!   0.1-inch wire 2.54 m long and the `siggen` sheet ~350 m wide.
//!
//! v0 scope: magic check + `{...}` head/body extraction + wire geometry.
//! Coordinate decoding beyond wires (components, labels) is pending.

use super::{MAGIC_TERM, ParseError};

/// Magic of `.DSN` schematics (without trailing `0x1A`).
pub const DSN_MAGIC: &[u8] = b"ISIS SCHEMATIC FILE";
/// Magic of `.LYT` layouts (without trailing `0x1A`).
pub const LYT_MAGIC: &[u8] = b"PCB LAYOUT FILE";
/// ISIS IV circuit magic handled by `DSNCVT40` (same framing family).
pub const CCT_MAGIC: &[u8] = b"ISIS CIRCUIT FILE";

/// Internal coordinate units per inch (1 unit = 10 nm).
///
/// Verified on 5 shipped samples (`siggen`, `Dbell`, `Cpu`, `Epe`,
/// `Features`): every wire length is a multiple of `25_400` units
/// (0.01 inch) and the modal segment is `254_000` units = 0.1 inch.
pub const UNITS_PER_INCH: u32 = 2_540_000;

/// Millimetres per internal unit (`25.4 / 2_540_000`).
pub const MM_PER_UNIT: f64 = 0.000_01;

/// One straight schematic wire segment, in DSN internal units.
///
/// Multi-point `WIRE` polylines are expanded into one `Wire` per segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wire {
    /// Start x in internal units (1 unit = 10 nm).
    pub x1: i32,
    /// Start y in internal units.
    pub y1: i32,
    /// End x in internal units.
    pub x2: i32,
    /// End y in internal units.
    pub y2: i32,
    /// True for `BUS WIRE` records (rendered thick).
    pub bus: bool,
}

/// Convert internal units to millimetres.
///
/// `to_mm(254_000)` is ~2.54 (one 0.1-inch grid step).
#[must_use]
pub fn to_mm(units: i32) -> f64 {
    f64::from(units) * MM_PER_UNIT
}

/// Tag opening a wire record.
const WIRE_TAG: &[u8] = b"WIRE";
/// Header bytes after [`WIRE_TAG`]: `00 00 00 <npoints> 00`.
const WIRE_HDR_LEN: usize = 5;
/// Bytes per polyline point: `(x, y)` little-endian `i32`.
const POINT_LEN: usize = 8;
/// Max polyline points accepted per record (wild max observed: 6).
const MAX_POINTS: usize = 64;

/// Extract straight wire segments from `WIRE` / `BUS WIRE` geometry records.
///
/// Record layout (verified on shipped `siggen.DSN`, `128_192` bytes):
/// `WIRE 00 00 00 <n> 00` followed by `n` points of `(x, y)`
/// little-endian `i32` in internal units ([`UNITS_PER_INCH`]). `BUS WIRE`
/// records share the layout and are included. Non-geometry records
/// (`WIRE LABEL`, `WIRE DOT`, `WIRE 00 01 00 ff ...` annotations) are
/// skipped, as are truncated tails.
///
/// Segments keep file order; an `n`-point polyline yields `n - 1` segments.
#[must_use]
pub fn wires(data: &[u8]) -> Vec<Wire> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if !data[i..].starts_with(WIRE_TAG) {
            i += 1;
            continue;
        }
        let Some(hdr) = data.get(i + WIRE_TAG.len()..i + WIRE_TAG.len() + WIRE_HDR_LEN) else {
            break;
        };
        // Geometry only: skips ` LABEL` / ` DOT` (hdr[0] == b' ')
        // and `00 01 00 ff ...` annotations (hdr[1] != 0).
        let n = usize::from(hdr[3]);
        if hdr[0] != 0
            || hdr[1] != 0
            || hdr[2] != 0
            || hdr[4] != 0
            || !(2..=MAX_POINTS).contains(&n)
        {
            i += 1;
            continue;
        }
        let base = i + WIRE_TAG.len() + WIRE_HDR_LEN;
        let Some(body) = data.get(base..base + n * POINT_LEN) else {
            i += 1;
            continue;
        };
        let mut pts = body.chunks_exact(POINT_LEN).map(|c| {
            (
                i32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                i32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            )
        });
        // `n >= 2`, so `prev` is always `Some`.
        let bus = i >= 4 && data[i - 4..i] == *b"BUS ";
        let mut prev = pts.next().unwrap_or((0, 0));
        for (x, y) in pts {
            out.push(Wire {
                x1: prev.0,
                y1: prev.1,
                x2: x,
                y2: y,
                bus,
            });
            prev = (x, y);
        }
        i += 1;
    }
    out
}

/// A placed text object: component, value, terminal or label.
///
/// Layout, verified on `siggen.DSN` (100+ instances with coordinates
/// inside the wire bounding box): `0xFF`, length byte, ASCII text,
/// then `x`, `y` as little-endian `i32` in [`UNITS_PER_INCH`] units.
/// Components (`U1`), values (`390`, `RES`), terminals (`VCC`),
/// sub-designators (`U5:A`) and labels are all placed objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Placed text (`U1`, `9k1`, `VCC`, ...).
    pub text: String,
    /// X in internal units (1 unit = 10 nm).
    pub x: i32,
    /// Y in internal units.
    pub y: i32,
    /// Absolute file offset of the `0xFF` marker.
    pub offset: usize,
}

/// Longest refdes/value label expected (bounds the scan).
const MAX_PLACED_LEN: usize = 24;

/// Extract every placed text object.
///
/// Positions are raw: callers filter by bounding box (graph labels and
/// annotation texts also match the pattern with degenerate coordinates).
#[must_use]
pub fn placements(data: &[u8]) -> Vec<Placed> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let Some(&len) = data.get(i + 1) else {
            break;
        };
        let len = len as usize;
        let end = i + 2 + len + 8;
        let Some(name) = data.get(i + 2..i + 2 + len) else {
            i += 1;
            continue;
        };
        let Some(coords) = data.get(i + 2 + len..end) else {
            i += 1;
            continue;
        };
        if (1..=MAX_PLACED_LEN).contains(&len)
            && name.iter().all(|b| *b == 0x20 || (0x21..=0x7E).contains(b))
        {
            out.push(Placed {
                text: String::from_utf8_lossy(name).into_owned(),
                x: i32::from_le_bytes([coords[0], coords[1], coords[2], coords[3]]),
                y: i32::from_le_bytes([coords[4], coords[5], coords[6], coords[7]]),
                offset: i,
            });
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

/// One PCB track segment in internal units (1 unit = 10 nm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    /// Start x.
    pub x1: i32,
    /// Start y (`0` in the samples seen — axis TBD).
    pub y1: i32,
    /// End x.
    pub x2: i32,
    /// End y.
    pub y2: i32,
    /// 3-byte group id shared by chained vertices (`cc ff 00` in Cpur).
    pub group: [u8; 3],
}

/// Track-vertex marker (`02 60 <group:3B> <x:i32> <y:i32> ...`).
const TRACK_MARK: &[u8] = b"\x02\x60";

/// Extract PCB track segments from `.LYT` (ARES side).
///
/// Consecutive `02 60` vertices sharing the 3-byte group id form one
/// polyline; each link becomes a [`Track`]. Verified on `Cpur.LYT`
/// (2 vertices → 1 segment, `$WR8`); `02 40` = via + `02 44` pin ref
/// (framing TBD). Y reads `0` in the samples — axis encoding TBD.
#[must_use]
pub fn lyt_tracks(data: &[u8]) -> Vec<Track> {
    let mut verts: Vec<([u8; 3], i32, i32)> = Vec::new();
    let mut i = 0;
    while i + 13 <= data.len() {
        if data[i] == TRACK_MARK[0] && data[i + 1] == TRACK_MARK[1] {
            let group = [data[i + 2], data[i + 3], data[i + 4]];
            let x = i32::from_le_bytes([data[i + 5], data[i + 6], data[i + 7], data[i + 8]]);
            let y = i32::from_le_bytes([data[i + 9], data[i + 10], data[i + 11], data[i + 12]]);
            // Sanity: board-scale coords, group id non-zero.
            if x.abs() < 100_000_000 && y.abs() < 100_000_000 && group != [0, 0, 0] {
                verts.push((group, x, y));
            }
            i += 13;
            continue;
        }
        i += 1;
    }
    let mut out = Vec::new();
    for w in verts.windows(2) {
        if w[0].0 == w[1].0 {
            out.push(Track {
                x1: w[0].1,
                y1: w[0].2,
                x2: w[1].1,
                y2: w[1].2,
                group: w[0].0,
            });
        }
    }
    out
}

/// One graphic symbol record: template name + candidate vertices.
///
/// Template directory form (verified `siggen.DSN`): `02 7F 00 LEN NAME
/// 01 19` enumerates `COMPONENT/PIN/PORT/MARKER/ACTUATOR/INDICATOR/
/// VPROBE/IPROBE/TAPE/GENERATOR/TERMINAL/SUBCIRCUIT/2D GRAPHIC/WIRE
/// DOT/WIRE/BUS WIRE/Panel/SUBCKT`. Records carry `02 7F <NAME>\0`
/// (template ref) or `02 7B/67/7D/7E <NAME>\0` (placed instance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Template name (`COMPONENT`, `GENERATOR`, `TERMINAL`, ...).
    pub template: String,
    /// Absolute file offset of the record marker.
    pub offset: usize,
    /// Candidate vertices `(x, y)` in DSN internal units.
    pub vertices: Vec<(i32, i32)>,
    /// Style id from the pen block (`34 00 00 00 00 02 <STYLE>`, e.g.
    /// `0x21` TERMINAL, `0x22` COMPONENT). `0` when absent.
    pub style: u32,
}

/// Marker prefix preceding every graphic template record.
const SYMBOL_MARK: &[u8] = b"\x02\x7f";
/// Maximum payload bytes scanned per record.
const SYMBOL_SCAN_LEN: usize = 128;
/// Maximum vertices kept per record.
const SYMBOL_MAX_VERTS: usize = 32;

/// Extract graphic template records (`02 7F <NAME>\0`) with candidate
/// vertices.
///
/// A vertex is an `i32` pair with both coordinates inside `bbox` (given
/// as `(xmin, ymin, xmax, ymax)`, typically the wire bbox expanded by a
/// margin). Pass `None` to keep every `i32` pair (noisy).
#[must_use]
pub fn symbols(data: &[u8], bbox: Option<(i32, i32, i32, i32)>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < data.len() {
        let Some(pos) = data[i..]
            .windows(SYMBOL_MARK.len())
            .position(|w| w == SYMBOL_MARK)
        else {
            break;
        };
        let at = i + pos + SYMBOL_MARK.len();
        // Template name: ASCII up to NUL, 1..=24 chars.
        let name_end = (at..(at + 25).min(data.len()))
            .find(|&k| data[k] == 0)
            .unwrap_or(at);
        let name = &data[at..name_end];
        if name.is_empty()
            || name.len() > 24
            || !name.iter().all(|b| *b == 0x20 || (0x21..=0x7E).contains(b))
        {
            i = at;
            continue;
        }
        let template = String::from_utf8_lossy(name).into_owned();
        let base = name_end + 1;
        let end = (base + SYMBOL_SCAN_LEN).min(data.len());
        let mut vertices = Vec::new();
        // Count-first: `u32 count` + pairs (observed `04 00 00 00` +
        // 4×`i32` in COMPONENT records). Fall back to blind scan when
        // the count is implausible (bbox-header records start with
        // coordinates, e.g. `70 5f f4 ff ...`).
        let mut j = base;
        if base + 4 <= data.len() {
            let count =
                u32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]]);
            if count > 0
                && u64::from(count) <= SYMBOL_MAX_VERTS as u64 * 2
                && base + 4 + count as usize * 8 <= data.len()
            {
                let mut ok = true;
                let mut cur = base + 4;
                let mut counted = Vec::new();
                for _ in 0..count {
                    let vex = i32::from_le_bytes([
                        data[cur],
                        data[cur + 1],
                        data[cur + 2],
                        data[cur + 3],
                    ]);
                    let vey = i32::from_le_bytes([
                        data[cur + 4],
                        data[cur + 5],
                        data[cur + 6],
                        data[cur + 7],
                    ]);
                    if vex.abs() > 100_000_000 || vey.abs() > 100_000_000 {
                        ok = false;
                        break;
                    }
                    counted.push((vex, vey));
                    cur += 8;
                }
                if ok {
                    j = base + 4 + count as usize * 8;
                    let keep = |vx: i32, vy: i32| match bbox {
                        Some((x0, y0, x1, y1)) => vx >= x0 && vx <= x1 && vy >= y0 && vy <= y1,
                        None => true,
                    };
                    vertices.extend(counted.into_iter().filter(|&(vx, vy)| keep(vx, vy)));
                }
            }
        }
        while j + 8 <= end && vertices.len() < SYMBOL_MAX_VERTS {
            let x = i32::from_le_bytes([data[j], data[j + 1], data[j + 2], data[j + 3]]);
            let y = i32::from_le_bytes([data[j + 4], data[j + 5], data[j + 6], data[j + 7]]);
            let keep = match bbox {
                Some((x0, y0, x1, y1)) => x >= x0 && x <= x1 && y >= y0 && y <= y1,
                None => true,
            };
            if keep {
                vertices.push((x, y));
            }
            j += 8;
        }
        out.push(Symbol {
            template,
            offset: i + pos,
            vertices,
            style: style_of(data, base),
        });
        i = base;
    }
    out
}
/// Extract the pen-block style id following a record payload:
/// `u32 len` + `34 00 00 00 00 02 <STYLE> ...` (observed `0x21` for
/// TERMINAL, `0x22` for COMPONENT in `siggen.DSN`).
fn style_of(data: &[u8], base: usize) -> u32 {
    let end = (base + SYMBOL_SCAN_LEN + 64).min(data.len());
    let win = &data[base.min(data.len())..end];
    for w in win.windows(8) {
        if w[0] == 0x34 && w[1] == 0 && w[2] == 0 && w[3] == 0 && w[4] == 0 && w[5] == 2 {
            return u32::from(w[6]);
        }
    }
    0
}
/// One `{...}` block: head plus the printable body that follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Absolute file offset of `{`.
    pub offset: usize,
    /// Head without braces (e.g. `*DEVICE`, `$PINSHORT`).
    pub head: String,
    /// Printable ASCII after `}` (trimmed, capped).
    pub body: String,
}

/// Max body chars kept per block.
const MAX_BODY: usize = 512;

/// Validate a `.DSN` or `.LYT` header.
///
/// # Errors
///
/// [`ParseError::Truncated`] when too short,
/// [`ParseError::BadMagic`] on magic/terminator mismatch.
pub fn check_magic(data: &[u8]) -> Result<usize, ParseError> {
    // Shortest known magic is `PCB LAYOUT FILE` (15) + terminator.
    if data.len() < LYT_MAGIC.len() + 1 {
        return Err(ParseError::Truncated);
    }
    for magic in [DSN_MAGIC, LYT_MAGIC, CCT_MAGIC] {
        if data.len() > magic.len()
            && &data[..magic.len()] == magic
            && data[magic.len()] == MAGIC_TERM
        {
            return Ok(magic.len() + 1);
        }
    }
    Err(ParseError::BadMagic {
        expected: "ISIS SCHEMATIC FILE, PCB LAYOUT FILE or ISIS CIRCUIT FILE",
    })
}

/// Extract `{...}` blocks with their printable bodies.
#[must_use]
pub fn blocks(data: &[u8]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] != b'{' {
            i += 1;
            continue;
        }
        let Some(end) = data[i..].iter().position(|&b| b == b'}') else {
            break;
        };
        let head = String::from_utf8_lossy(&data[i + 1..i + end]).into_owned();
        // Drop binary garbage: real heads are short ASCII tokens.
        let head_ok = !head.is_empty()
            && head.len() <= 48
            && head.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'*' | b'$' | b'_' | b'-' | b'=' | b' ' | b'/' | b'.')
            });
        if !head_ok {
            i = i + end + 1;
            continue;
        }
        // Body: printable run after `}`, capped.
        let mut body = String::new();
        let mut j = i + end + 1;
        while j < data.len() && body.len() < MAX_BODY {
            let b = data[j];
            if b == b'{' {
                break;
            }
            if b == 0x09 || (0x20..=0x7E).contains(&b) {
                body.push(b as char);
            } else if body.is_empty() {
                // Skip leading binary padding, keep scanning a bit.
                if j - (i + end + 1) > 64 {
                    break;
                }
            } else {
                break;
            }
            j += 1;
        }
        out.push(Block {
            offset: i,
            head,
            body: body.trim().to_owned(),
        });
        i = i + end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = Vec::from(b"ISIS SCHEMATIC FILE\x1aSignal Generator\0\0".as_slice());
        v.extend_from_slice(
            b"\x01\x02{*DEVICE} PREFIX=U PINOUT=7404\0\xff{*COMPONENT} {MODFILE=74INV.MDF}\0",
        );
        v
    }

    #[test]
    fn lyt_tracks_chain() {
        // Two chained `02 60` vertices (Cpur.LYT shape) -> 1 segment.
        let mut v = Vec::new();
        for x in [14_239_744i32, 10_233_560i32] {
            v.extend_from_slice(b"\x02\x60\xcc\xff\x00");
            v.extend_from_slice(&x.to_le_bytes());
            v.extend_from_slice(&0i32.to_le_bytes());
        }
        let t = super::lyt_tracks(&v);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].x1, 14_239_744);
        assert_eq!(t[0].x2, 10_233_560);
        assert_eq!(t[0].group, [0xcc, 0xff, 0x00]);
        // Different groups do not chain.
        v[2] = 0x01;
        assert!(super::lyt_tracks(&v).is_empty(), "groups");
    }

    #[test]
    fn symbols_pen_style() {
        // `34 00 00 00 00 02 <STYLE>` pen block (TERMINAL=0x21).
        let mut v = Vec::from(b"\x02\x7fTERMINAL\x00".as_slice());
        v.extend_from_slice(&[0u8; 16]);
        v.extend_from_slice(&[0x34, 0, 0, 0, 0, 2, 0x21, 0]);
        let syms = symbols(&v, None);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].style, 0x21);
        // No pen block -> style 0.
        let w = Vec::from(b"\x02\x7fWIRE\x00".as_slice());
        assert_eq!(symbols(&w, None)[0].style, 0);
    }

    #[test]
    fn symbols_count_first() {
        // Observed COMPONENT record shape: marker + `u32 count` + pairs.
        let mut v = Vec::from(b"\x02\x7fCOMPONENT\x00".as_slice());
        v.extend_from_slice(&2u32.to_le_bytes());
        for (x, y) in [(1_000_000i32, 2_000_000i32), (3_000_000, 4_000_000)] {
            v.extend_from_slice(&x.to_le_bytes());
            v.extend_from_slice(&y.to_le_bytes());
        }
        let syms = symbols(&v, None);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].template, "COMPONENT");
        assert_eq!(syms[0].vertices.len(), 2);
        // Implausible count (bbox header) falls back to blind scan.
        let mut w = Vec::from(b"\x02\x7fWIRE\x00".as_slice());
        w.extend_from_slice(&0xfff4_5f70u32.to_le_bytes());
        w.extend_from_slice(&[0u8; 120]);
        assert_eq!(symbols(&w, None).len(), 1);
    }

    #[test]
    fn magic_ok() {
        assert!(check_magic(&sample()).is_ok());
        assert!(check_magic(b"short").is_err());
        assert!(check_magic(b"NOT A SCHEMATIC FILE!!!!!").is_err());
    }

    #[test]
    fn blocks_found() {
        let b = blocks(&sample());
        // `{MODFILE=...}` nested in the COMPONENT body is its own block.
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].head, "*DEVICE");
        assert!(b[0].body.contains("PREFIX=U"));
        assert_eq!(b[1].head, "*COMPONENT");
        assert_eq!(b[2].head, "MODFILE=74INV.MDF");
    }

    #[test]
    fn real_dsn_head_validates() {
        assert!(check_magic(b"ISIS SCHEMATIC FILE\x1aSignal").is_ok());
        // DSNCVT40 identity: ISIS IV circuit magic shares the framing.
        assert!(check_magic(b"ISIS CIRCUIT FILE\x1aCCT001").is_ok());
        assert!(check_magic(b"PCB LAYOUT FILE\x1a technolog").is_ok());
        // 16 bytes (LYT-sized) of garbage is BadMagic, not Truncated.
        assert!(matches!(
            check_magic(b"0123456789ABCDEF"),
            Err(ParseError::BadMagic { .. })
        ));
        assert!(matches!(check_magic(b"short"), Err(ParseError::Truncated)));
    }

    fn push_pt(buf: &mut Vec<u8>, x: i32, y: i32) {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
    }

    fn push_wire(buf: &mut Vec<u8>, pts: &[(i32, i32)]) {
        buf.extend_from_slice(b"WIRE\x00\x00\x00");
        buf.push(u8::try_from(pts.len()).unwrap_or(u8::MAX));
        buf.push(0);
        for &(x, y) in pts {
            push_pt(buf, x, y);
        }
    }

    #[test]
    fn wires_decode_polylines_and_bus() {
        let mut v = Vec::from(b"ISIS SCHEMATIC FILE\x1a\x00".as_slice());
        // N=2 horizontal segment.
        push_wire(&mut v, &[(0, 0), (254_000, 0)]);
        // N=3 L-shape -> 2 segments.
        push_wire(&mut v, &[(0, 0), (0, 508_000), (254_000, 508_000)]);
        // BUS WIRE shares the geometry layout.
        v.extend_from_slice(b"BUS ");
        push_wire(&mut v, &[(-127_000, 10), (-127_000, 264_000)]);
        // Decoys: annotation, label and dot records hold no geometry.
        v.extend_from_slice(b"WIRE\x00\x01\x00\xff\x0bSTRAT=POWER");
        v.extend_from_slice(b"WIRE LABEL\x00\x00\x00\x00\x00");
        v.extend_from_slice(b"WIRE DOT\x01");
        // Truncated tail must not panic nor yield a phantom wire.
        v.extend_from_slice(b"WIRE\x00\x00\x00\x02\x00\x01\x02");

        assert_eq!(
            wires(&v),
            vec![
                Wire {
                    x1: 0,
                    y1: 0,
                    x2: 254_000,
                    y2: 0,
                    bus: false,
                },
                Wire {
                    x1: 0,
                    y1: 0,
                    x2: 0,
                    y2: 508_000,
                    bus: false,
                },
                Wire {
                    x1: 0,
                    y1: 508_000,
                    x2: 254_000,
                    y2: 508_000,
                    bus: false,
                },
                Wire {
                    x1: -127_000,
                    y1: 10,
                    x2: -127_000,
                    y2: 264_000,
                    bus: true,
                },
            ]
        );
    }

    #[test]
    fn scale_matches_tenth_inch_grid() {
        // Modal wild segment: 254_000 units == 0.1 inch == 2.54 mm.
        let got = to_mm(254_000);
        assert!((got - 2.54).abs() < 1e-9, "to_mm(254_000) = {got}");
        let inch = to_mm(i32::try_from(UNITS_PER_INCH).unwrap_or(i32::MAX));
        assert!((inch - 25.4).abs() < 1e-9, "to_mm(UNITS_PER_INCH) = {inch}");
        assert_eq!(to_mm(0), 0.0);
    }

    /// Shipped sample used for RE verification. Skipped (not failed)
    /// when absent so the suite stays green without the wine prefix.
    const SIGGEN_DSN: &str = "/home/diez/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/SAMPLES/Schematic & PCB Layout/siggen.DSN";

    #[test]
    fn placements_decode_positions() {
        // ff | len=2 "U1" | x=5461000 | y=3937000 (siggen layout).
        let mut v = vec![0xffu8, 2, b'U', b'1'];
        v.extend_from_slice(&5_461_000i32.to_le_bytes());
        v.extend_from_slice(&3_937_000i32.to_le_bytes());
        let ps = placements(&v);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "U1");
        assert_eq!((ps[0].x, ps[0].y), (5_461_000, 3_937_000));
    }

    #[test]
    fn real_siggen_placements() {
        let Ok(data) = std::fs::read(SIGGEN_DSN) else {
            return;
        };
        let ps = placements(&data);
        // Spot-checks from independent hex inspection.
        for (name, x, y) in [
            ("U1", 5_461_000, 3_937_000),
            ("R1", 2_667_000, 1_498_600),
            ("C1", 4_826_000, 127_000),
        ] {
            assert!(
                ps.iter().any(|p| p.text == name && p.x == x && p.y == y),
                "{name} at ({x},{y})"
            );
        }
        // Most placements fall inside the wire bounding box; the
        // remainder are graph/sheet annotations with degenerate coords.
        let inside = ps
            .iter()
            .filter(|p| {
                (-18_415_000..=16_637_000).contains(&p.x) && (-8_255_000..=6_477_000).contains(&p.y)
            })
            .count();
        assert!(inside > 100, "inside={inside} of {}", ps.len());
    }

    #[test]
    fn real_siggen_symbols_have_geometry() {
        let Ok(data) = std::fs::read(SIGGEN_DSN) else {
            return;
        };
        let bbox = (-18_415_000, -8_255_000, 16_637_000, 6_477_000);
        let syms = symbols(&data, Some(bbox));
        // Full template directory, not just COMPONENT: 298 records in
        // 11 template kinds (independent byte scan agrees).
        let mut kinds = std::collections::BTreeMap::new();
        for s in &syms {
            *kinds.entry(s.template.clone()).or_insert(0) += 1;
        }
        assert_eq!(syms.len(), 298);
        assert_eq!(kinds.get("COMPONENT"), Some(&30));
        assert_eq!(kinds.get("WIRE"), Some(&120));
        assert_eq!(kinds.len(), 11);
        let total: usize = syms.iter().map(|s| s.vertices.len()).sum();
        assert_eq!(total, 1627);
        // Deterministic: same input, same output.
        assert_eq!(syms, symbols(&data, Some(bbox)));
    }

    #[test]
    fn real_siggen_wire_count_and_bbox() {
        let Ok(data) = std::fs::read(SIGGEN_DSN) else {
            return;
        };
        assert!(check_magic(&data).is_ok());
        let ws = wires(&data);
        // 101 plain + 17 bus records expanded to segments.
        assert_eq!(ws.len(), 153);
        // Every wild segment is axis-aligned.
        assert!(ws.iter().all(|w| w.x1 == w.x2 || w.y1 == w.y2));
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for w in &ws {
            for (x, y) in [(w.x1, w.y1), (w.x2, w.y2)] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        assert_eq!(
            (x0, y0, x1, y1),
            (-18_415_000, -8_255_000, 16_637_000, 6_477_000)
        );
        // Bbox is ~350.5 x 147.3 mm: fits an A3 sheet (420 x 297 mm)
        // at 10 nm/unit, refuting the 10 um/unit guess (~350 m wide).
        assert!(((to_mm(x1 - x0) - 350.52).abs() < 0.01));
        assert!(((to_mm(y1 - y0) - 147.32).abs() < 0.01));
    }
}
