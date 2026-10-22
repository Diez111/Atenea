//! `.IDX` index and `.LIB` directory readers.
//!
//! Observed facts (`ArduinoTEP.IDX`, 443 bytes; `ArduinoTEP.LIB`, 289430
//! bytes, Proteus 8.6 / TEP pack):
//!
//! - IDX: magic `DEVICE LIBRARY INDEX\x1A`, padding, `u32` count at
//!   offset `36` (7 here), then records split by the `01 00 00 00`
//!   terminator: `key: u32` + NUL-separated `CAT`, `MFR` (may be empty),
//!   `DESC` strings. File ends with the `12 34 56 78` sentinel.
//! - LIB: magic `DEVICE LIBRARY\x1A` + 13 header bytes, then a directory
//!   from offset `32` with stride 80: `name[64]` NUL-padded + `seq: u32`
//!   + `key: u32` (matches the IDX key) + `body_offset: u32` (absolute,
//!     points at the device's `COMPONENT` area) + `flags: u32`.
//! - 6 of 7 offsets verified live; `ARDUINO NANO` → `6460` (zeros):
//!   stale slot or external reference, kept as-is, not hidden.
//!
//! v0 scope: index + directory parsing with IDX↔LIB key join. Record
//! internals (geometry blobs) stay out.

use super::ParseError;

/// One `.IDX` entry: key plus catalog strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdxEntry {
    /// Join key shared with [`DirEntry::key`].
    pub key: u32,
    /// Category (`Arduino`, `Analog ICs`, ...).
    pub cat: String,
    /// Manufacturer (may be empty).
    pub mfr: String,
    /// Description (`Arduino MEGA 2560`, ...).
    pub desc: String,
}

/// One `.LIB` directory entry (80 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Device name (`ARDUINO MEGA 2560`).
    pub name: String,
    /// Sequence number (0-based file order guess).
    pub seq: u32,
    /// Join key shared with [`IdxEntry::key`].
    pub key: u32,
    /// Absolute file offset of the device body.
    pub offset: u32,
    /// Flags (observed 0).
    pub flags: u32,
}

/// File-level preface between magic and records: zero padding, a `u32`
/// tag (observed `102`), more padding, then the `u32` record count.
/// Returns `(record_count, records_start)`.
fn idx_preface(data: &[u8], header: usize) -> Option<(u32, usize)> {
    let mut i = header;
    while data.get(i) == Some(&0) {
        i += 1;
    }
    i += 4; // tag
    while data.get(i) == Some(&0) {
        i += 1;
    }
    let count = u32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?);
    Some((count, i + 4))
}
/// `.IDX` record terminator.
const IDX_TERM: &[u8; 4] = b"\x01\x00\x00\x00";
/// `.IDX` end-of-index sentinel.
const _IDX_SENTINEL: u32 = 0x1234_5678;
/// `.LIB` directory base offset and stride.
const LIB_DIR_BASE: usize = 32;
const LIB_DIR_STRIDE: usize = 80;
/// `.LIB` directory name field width.
const LIB_NAME_LEN: usize = 64;

/// Parse an `.IDX` blob into its entries.
///
/// # Errors
///
/// [`ParseError::BadMagic`] on magic/terminator mismatch,
/// [`ParseError::Truncated`] when too short.
pub fn parse_idx(data: &[u8], kind: super::LibKind) -> Result<Vec<IdxEntry>, ParseError> {
    let header = super::check_magic(data, kind)?;
    let Some((_count, start)) = idx_preface(data, header) else {
        return Err(ParseError::Truncated);
    };
    let mut rest = &data[start..];
    let mut out = Vec::new();
    // Split records on the terminator; the tail holds the sentinel.
    while let Some(pos) = rest.windows(IDX_TERM.len()).position(|w| w == IDX_TERM) {
        let (rec, after) = rest.split_at(pos);
        rest = &after[IDX_TERM.len()..];
        if rec.len() < 5 || rec.iter().all(|&b| b == 0) {
            continue;
        }
        let key = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
        // Layout: CAT \0 [MFR \0] DESC \0 — empty MFR is omitted,
        // never an empty field (verified on ArduinoTEP.IDX).
        let fields: Vec<String> = rec[4..]
            .split(|&b| b == 0)
            .filter_map(|s| {
                if s.is_empty() {
                    None
                } else {
                    Some(String::from_utf8_lossy(s).into_owned())
                }
            })
            .collect();
        let (cat, mfr, desc) = match fields.as_slice() {
            [c, m, d, ..] => (c.clone(), m.clone(), d.clone()),
            [c, d] => (c.clone(), String::new(), d.clone()),
            _ => continue,
        };
        out.push(IdxEntry {
            key,
            cat,
            mfr,
            desc,
        });
    }
    Ok(out)
}

/// Parse a `.LIB` directory into entries.
///
/// Stops at the first all-zero slot or implausible name; caps at 4096.
#[must_use]
pub fn parse_lib_dir(data: &[u8]) -> Vec<DirEntry> {
    let mut out = Vec::new();
    let mut base = LIB_DIR_BASE;
    while out.len() < 4096 {
        let Some(slot) = data.get(base..base + LIB_DIR_STRIDE) else {
            break;
        };
        if slot.iter().all(|&b| b == 0) {
            break;
        }
        let name_raw = &slot[..LIB_NAME_LEN];
        if name_raw
            .iter()
            .any(|&b| b != 0 && !(0x20..=0x7E).contains(&b))
        {
            break;
        }
        let name = String::from_utf8_lossy(name_raw);
        let name = name.trim_end_matches('\0').to_owned();
        let tail = &slot[LIB_NAME_LEN..];
        let u = |o: usize| u32::from_le_bytes([tail[o], tail[o + 1], tail[o + 2], tail[o + 3]]);
        out.push(DirEntry {
            name,
            seq: u(0),
            key: u(4),
            offset: u(8),
            flags: u(12),
        });
        base += LIB_DIR_STRIDE;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::LibKind;

    const IDX_MAGIC: &[u8] = b"DEVICE LIBRARY INDEX";

    fn sample_idx() -> Vec<u8> {
        let mut v = Vec::from(IDX_MAGIC);
        v.push(super::super::MAGIC_TERM);
        v.extend_from_slice(&[0u8; 11]); // pad
        v.extend_from_slice(&102u32.to_le_bytes()); // tag
        v.extend_from_slice(&2u32.to_le_bytes()); // count
        for (key, cat, mfr, desc) in [
            (11u32, "Arduino", "www.x", "Zoom 1"),
            (22u32, "Analog ICs", "", "Zoom 2"),
        ] {
            v.extend_from_slice(&key.to_le_bytes());
            v.extend_from_slice(cat.as_bytes());
            v.push(0);
            if !mfr.is_empty() {
                // Real files omit the MFR field entirely when empty:
                // `CAT\0DESC\0`, not `CAT\0\0DESC\0`.
                v.extend_from_slice(mfr.as_bytes());
                v.push(0);
            }
            v.extend_from_slice(desc.as_bytes());
            v.push(0);
            v.extend_from_slice(super::IDX_TERM);
        }
        v.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        v
    }

    #[test]
    fn idx_roundtrip() {
        let entries = super::parse_idx(&sample_idx(), LibKind::Idx).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, 11);
        assert_eq!(entries[0].cat, "Arduino");
        assert_eq!(entries[0].mfr, "www.x");
        assert_eq!(entries[0].desc, "Zoom 1");
        assert_eq!(entries[1].mfr, "");
    }

    #[test]
    fn dir_entry_layout() {
        let mut lib = vec![0u8; 32 + 80];
        lib[32..32 + 7].copy_from_slice(b"ZOOMDEV");
        lib[32 + 64..32 + 68].copy_from_slice(&3u32.to_le_bytes());
        lib[32 + 68..32 + 72].copy_from_slice(&11u32.to_le_bytes());
        lib[32 + 72..32 + 76].copy_from_slice(&4096u32.to_le_bytes());
        let dirs = super::parse_lib_dir(&lib);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "ZOOMDEV");
        assert_eq!(dirs[0].seq, 3);
        assert_eq!(dirs[0].key, 11);
        assert_eq!(dirs[0].offset, 4096);
    }

    #[test]
    fn real_tep_join() {
        let idx = std::fs::read(
            "/home/diez/Descargas/Proteus-install/Proteus-8.6-SP2-Pro-español/Librerias Arduino/ArduinoTEP/ArduinoTEP.IDX",
        );
        let lib = std::fs::read(
            "/home/diez/Descargas/Proteus-install/Proteus-8.6-SP2-Pro-español/Librerias Arduino/ArduinoTEP/ArduinoTEP.LIB",
        );
        let (Ok(idx), Ok(lib)) = (idx, lib) else {
            return;
        };
        let entries = super::parse_idx(&idx, LibKind::Idx).unwrap();
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].desc, "Arduino MEGA 2560");
        let dirs = super::parse_lib_dir(&lib);
        assert_eq!(dirs.len(), 7);
        assert_eq!(dirs[0].name, "ARDUINO MEGA 2560");
        // 6 of 7 IDX keys join to directory entries; the MEGA2560 Rev3
        // variant has no dedicated slot (verified absence). ARDUINO UNO
        // appears twice (alias slot, same key and offset).
        for e in &entries {
            let n = dirs.iter().filter(|d| d.key == e.key).count();
            if e.desc == "Arduino MEGA2560 Rev3" {
                assert_eq!(n, 0, "key {:08x}", e.key);
            } else {
                assert!((1..=2).contains(&n), "key {:08x}", e.key);
            }
        }
        // 6 of 7 offsets land in live body regions (NANO→zeros, documented).
        let live = dirs
            .iter()
            .filter(|d| {
                lib.get(d.offset as usize..d.offset as usize + 4)
                    .is_some_and(|b| b.iter().any(|&x| x != 0))
            })
            .count();
        assert!(live >= 6, "live={live}");
    }
}
