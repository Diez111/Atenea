//! Reader for Proteus `.LML` model library containers.
//!
//! Observed facts (`MODELS/ACTIVE.LML`, 49632 bytes, 41 decks):
//! - Same container layout as `.SML` ([`crate::sml`]): `u32` deck count
//!   at `0x18`, 32-byte TOC entries from `0x20` (`name[16]` NUL-padded +
//!   `u32` zero + `u32` chunk offset + `u32` timestamp + `u32` unknown),
//!   chunks are `u32` total length including the 4 bytes + payload text.
//!   Only the magic differs: `MODEL LIBRARY\x1a`.
//! - Payload is LISA model text (`LISA MODEL DESCRIPTION FORMAT 8.0`,
//!   parseable with [`crate::parse`]). Quirk: 6 title-less
//!   SPICELNK-converted decks in `ANALOG.LML` (`7806`, `7808`–`7810`,
//!   `7818`, `7824`) start at `Design:` with no `FORMAT` title line.
//!
//! v0 scope: container split into named decks (see [`crate::container`]).

pub use crate::container::{Deck, ParseError};

/// Container magic (without trailing `0x1A`).
pub const LML_MAGIC: &[u8] = b"MODEL LIBRARY";

/// Split a `.LML` container into named decks.
///
/// # Errors
///
/// [`ParseError::BadHeader`] on bad magic/TOC,
/// [`ParseError::TruncatedChunk`] when a chunk overruns the file.
pub fn split(data: &[u8]) -> Result<Vec<Deck>, ParseError> {
    crate::container::split(data, LML_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = Vec::from(b"MODEL LIBRARY\x1a\0\0".as_slice());
        v.extend_from_slice(&[0u8; 8]); // pad to 0x18
        v.extend_from_slice(&[2u8, 0, 0, 0]); // count
        v.extend_from_slice(&[0u8; 4]); // pad to 0x20
        // entry 0 @0x20: name + zero + offset + ts + ?
        let mut e0 = [0u8; 32];
        e0[..8].copy_from_slice(b"ACIMETER");
        e0[20..24].copy_from_slice(&128u32.to_le_bytes());
        v.extend_from_slice(&e0);
        let mut e1 = [0u8; 32];
        e1[..6].copy_from_slice(b"ACLAMP");
        e1[20..24].copy_from_slice(&192u32.to_le_bytes());
        v.extend_from_slice(&e1);
        // pad to offset 128, then two chunks
        v.resize(128, 0);
        let d0 = b"LISA MODEL DESCRIPTION FORMAT 8.0\n====\n*PARTLIST,0\n";
        v.extend_from_slice(&u32::try_from(d0.len() + 4).unwrap().to_le_bytes());
        v.extend_from_slice(d0);
        v.resize(192, 0);
        let d1 = b"LISA MODEL DESCRIPTION FORMAT 8.0\n====\n*PARTLIST,0\n";
        v.extend_from_slice(&u32::try_from(d1.len() + 4).unwrap().to_le_bytes());
        v.extend_from_slice(d1);
        v
    }

    #[test]
    fn splits_sample() {
        let decks = split(&sample()).unwrap();
        assert_eq!(decks.len(), 2);
        assert_eq!(decks[0].name, "ACIMETER");
        assert!(decks[0].text.contains("LISA MODEL DESCRIPTION FORMAT"));
        assert_eq!(decks[1].name, "ACLAMP");
    }

    #[test]
    fn bad_magic_rejected() {
        assert_eq!(split(b"garbage").unwrap_err(), ParseError::BadHeader);
        // The sibling container magic is not accepted either.
        assert_eq!(
            split(b"SPICE LIBRARY\x1a\0\0garbage-padded-to-32-bytes.....").unwrap_err(),
            ParseError::BadHeader
        );
    }

    const MODELS: &str = "/home/diez/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/MODELS";
    const ACTIVE_LML: &str = "/home/diez/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/MODELS/ACTIVE.LML";

    /// Shipped sample. Skipped (not failed) when absent so the suite
    /// stays green without the wine prefix.
    #[test]
    fn real_active_lml_deck_count() {
        let Ok(data) = std::fs::read(ACTIVE_LML) else {
            return;
        };
        let decks = split(&data).unwrap();
        assert_eq!(decks.len(), 41);
        assert_eq!(decks[0].name, "ACIMETER");
        assert_eq!(decks[40].name, "WMETER");
        for d in &decks {
            assert!(
                d.text.starts_with("LISA MODEL DESCRIPTION FORMAT"),
                "{}",
                d.name
            );
            // Every deck parses as a LISA document.
            let doc = crate::parse(&d.text.replace('\0', "")).unwrap();
            assert_eq!(doc.format, "LISA MODEL DESCRIPTION");
        }
    }

    /// Conformance sweep: every `.LML` in `MODELS/` splits cleanly.
    /// Skipped (not failed) when the wine prefix is absent.
    #[test]
    fn real_all_lml_split() {
        let Ok(entries) = std::fs::read_dir(MODELS) else {
            return;
        };
        let mut files = 0;
        let mut decks_total = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("LML") {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let decks = split(&data).unwrap();
            assert!(!decks.is_empty(), "{}", path.display());
            decks_total += decks.len();
            files += 1;
        }
        assert_eq!(files, 33);
        assert_eq!(decks_total, 1097);
    }
}
