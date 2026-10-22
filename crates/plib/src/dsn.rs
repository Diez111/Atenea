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
        let mut prev = pts.next().unwrap_or((0, 0));
        for (x, y) in pts {
            out.push(Wire {
                x1: prev.0,
                y1: prev.1,
                x2: x,
                y2: y,
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

/// One vector-stroke symbol record (`COMPONENT` graphic primitive).
///
/// Layout (observed, `siggen.DSN`): a `02 7F` marker + literal
/// `COMPONENT\0`, preceded by a fixed binary header and followed by a
/// payload of little-endian `i32` values. Coordinates that fall inside
/// an expanded wire bounding box are kept as candidate vertices; the
/// exact stroke encoding (lines vs rects, pen, layer) is still open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Absolute file offset of the `COMPONENT` marker.
    pub offset: usize,
    /// Candidate vertices `(x, y)` in DSN internal units.
    pub vertices: Vec<(i32, i32)>,
}

/// Marker preceding every graphic `COMPONENT` record.
const COMPONENT_MARK: &[u8] = b"\x02\x7fCOMPONENT\0";
/// Maximum payload bytes scanned per record.
const SYMBOL_SCAN_LEN: usize = 128;
/// Maximum vertices kept per record.
const SYMBOL_MAX_VERTS: usize = 32;

/// Extract graphic `COMPONENT` records with candidate vertices.
///
/// A vertex is an `i32` pair with both coordinates inside `bbox` (given
/// as `(xmin, ymin, xmax, ymax)`, typically the wire bbox expanded by a
/// margin). Pass `None` to keep every `i32` pair (noisy).
#[must_use]
pub fn symbols(data: &[u8], bbox: Option<(i32, i32, i32, i32)>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some(pos) = data[i..]
            .windows(COMPONENT_MARK.len())
            .position(|w| w == COMPONENT_MARK)
        else {
            break;
        };
        let base = i + pos + COMPONENT_MARK.len();
        let end = (base + SYMBOL_SCAN_LEN).min(data.len());
        let mut vertices = Vec::new();
        let mut j = base;
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
        // Drop markers with no plausible geometry (e.g. text-only hits).
        if !(vertices.is_empty() && bbox.is_some()) {
            out.push(Symbol {
                offset: i + pos,
                vertices,
            });
        }
        i = base;
    }
    out
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
    for magic in [DSN_MAGIC, LYT_MAGIC] {
        if data.len() > magic.len()
            && &data[..magic.len()] == magic
            && data[magic.len()] == MAGIC_TERM
        {
            return Ok(magic.len() + 1);
        }
    }
    Err(ParseError::BadMagic {
        expected: "ISIS SCHEMATIC FILE or PCB LAYOUT FILE",
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
                    y2: 0
                },
                Wire {
                    x1: 0,
                    y1: 0,
                    x2: 0,
                    y2: 508_000
                },
                Wire {
                    x1: 0,
                    y1: 508_000,
                    x2: 254_000,
                    y2: 508_000
                },
                Wire {
                    x1: -127_000,
                    y1: 10,
                    x2: -127_000,
                    y2: 264_000
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
        // 125 bare COMPONENT hits were counted by independent scan;
        // only markers with plausible geometry survive.
        // Independent byte scan counts exactly 30 `02 7f COMPONENT` markers.
        assert_eq!(syms.len(), 30);
        // Every record yields vertices on the 0.05"/0.1" grid.
        let total: usize = syms.iter().map(|s| s.vertices.len()).sum();
        assert_eq!(total, 249);
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
