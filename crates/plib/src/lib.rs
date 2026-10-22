//! Parsers for Labcenter Proteus library container formats.
//!
//! Observed facts (Proteus 8.6 SP2, from shipped `ArduinoTEP` samples):
//! - `.IDX` files start with magic `DEVICE LIBRARY INDEX` + `0x1A`.
//! - `.LIB` files start with magic `DEVICE LIBRARY` + `0x1A`.
//! - Both embed NUL-padded printable ASCII (part names, categories, URLs).
//!
//! v0 scope: magic validation + string-table extraction. Record-structure
//! decoding is pending deeper RE (see `docs/reverse-engineering.md`).

use std::fmt;

pub mod dsn;
pub mod libdir;

/// Magic prefix of `.IDX` index files (without the trailing `0x1A`).
pub const IDX_MAGIC: &[u8] = b"DEVICE LIBRARY INDEX";
/// Magic prefix of `.LIB` device library files (without trailing `0x1A`).
pub const LIB_MAGIC: &[u8] = b"DEVICE LIBRARY";
/// Magic prefix of package/footprint `.LIB` files (e.g. `CONNECTORS.LIB`).
pub const PACKAGE_MAGIC: &[u8] = b"PACKAGE LIBRARY";
/// Magic prefix of `.LML` model library files (e.g. `MODELS/ACTIVE.LML`).
pub const MODEL_MAGIC: &[u8] = b"MODEL LIBRARY";
/// Magic prefix of `SYSTEM.LIB` symbol libraries.
pub const SYMBOL_MAGIC: &[u8] = b"SYMBOL LIBRARY";
/// Magic prefix of `SYSTEM.IDX` symbol index files.
pub const SYMBOL_IDX_MAGIC: &[u8] = b"SYMBOL LIBRARY INDEX";
/// Terminator byte observed after both magics.
pub const MAGIC_TERM: u8 = 0x1A;

const MIN_STRING_LEN: usize = 3;

/// Format discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibKind {
    /// `DEVICE LIBRARY INDEX` (`.IDX`).
    Idx,
    /// `DEVICE LIBRARY` (`.LIB` device libraries).
    Lib,
    /// `PACKAGE LIBRARY` (`.LIB` footprint libraries).
    Package,
    /// `MODEL LIBRARY` (`.LML` model libraries).
    Model,
    /// `SYMBOL LIBRARY` (`SYSTEM.LIB` symbol libraries).
    Symbol,
    /// `SYMBOL LIBRARY INDEX` (`SYSTEM.IDX` symbol index).
    SymbolIdx,
}

/// Validation / parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Magic prefix does not match `expected`.
    BadMagic { expected: &'static str },
    /// File ends before the magic terminator.
    Truncated,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic { expected } => {
                write!(f, "bad magic: expected {expected}")
            }
            Self::Truncated => write!(f, "truncated file"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Validate the header of an `.IDX`, `.LIB` or `.LML` blob.
///
/// Returns the byte length of the validated header.
///
/// # Errors
///
/// - [`ParseError::Truncated`] if the blob is shorter than magic + terminator.
/// - [`ParseError::BadMagic`] if magic or terminator mismatch.
pub fn check_magic(data: &[u8], kind: LibKind) -> Result<usize, ParseError> {
    let magic = match kind {
        LibKind::Idx => IDX_MAGIC,
        LibKind::Lib => LIB_MAGIC,
        LibKind::Package => PACKAGE_MAGIC,
        LibKind::Model => MODEL_MAGIC,
        LibKind::Symbol => SYMBOL_MAGIC,
        LibKind::SymbolIdx => SYMBOL_IDX_MAGIC,
    };
    if data.len() < magic.len() + 1 {
        return Err(ParseError::Truncated);
    }
    if &data[..magic.len()] != magic {
        return Err(ParseError::BadMagic {
            expected: match kind {
                LibKind::Idx => "DEVICE LIBRARY INDEX",
                LibKind::Lib => "DEVICE LIBRARY",
                LibKind::Package => "PACKAGE LIBRARY",
                LibKind::Model => "MODEL LIBRARY",
                LibKind::Symbol => "SYMBOL LIBRARY",
                LibKind::SymbolIdx => "SYMBOL LIBRARY INDEX",
            },
        });
    }
    if data[magic.len()] != MAGIC_TERM {
        return Err(ParseError::BadMagic {
            expected: "0x1A terminator",
        });
    }
    Ok(magic.len() + 1)
}

/// Extract printable-ASCII runs (`len >= 3`) after the header.
///
/// This recovers part names, categories and vendor strings without
/// knowing the record layout yet. Offsets are absolute file offsets.
///
/// # Errors
///
/// Propagates header errors from [`check_magic`].
pub fn string_table(data: &[u8], kind: LibKind) -> Result<Vec<StringEntry>, ParseError> {
    let header_len = check_magic(data, kind)?;
    let mut out = Vec::new();
    let mut start: Option<usize> = None;

    let flush = |out: &mut Vec<StringEntry>, end: usize, start: usize| {
        if end - start >= MIN_STRING_LEN
            && let Ok(s) = std::str::from_utf8(&data[start..end])
        {
            out.push(StringEntry {
                offset: start,
                text: s.to_owned(),
            });
        }
    };

    for (i, &b) in data.iter().enumerate().skip(header_len) {
        // Printable ASCII plus tab; strings are NUL-padded in the wild.
        let printable = b == 0x09 || (0x20..=0x7E).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                flush(&mut out, i, s);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        flush(&mut out, data.len(), s);
    }
    Ok(out)
}

/// One printable run inside the blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringEntry {
    /// Absolute file offset where the run starts.
    pub offset: usize,
    /// The decoded text.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic blob mirroring the observed `.IDX` layout:
    /// magic + 0x1A, padding, then NUL-separated ASCII fields.
    fn sample_idx() -> Vec<u8> {
        let mut v = Vec::from(b"DEVICE LIBRARY INDEX\x1a\0\0\0".as_slice());
        v.extend_from_slice(b"Arduino\x00www.TheEngineeringProjects.com\x00Arduino MEGA 2560\x00");
        v
    }

    #[test]
    fn idx_magic_ok() {
        assert_eq!(check_magic(&sample_idx(), LibKind::Idx), Ok(21));
    }

    #[test]
    fn lib_magic_rejected_for_idx_blob() {
        // "DEVICE LIBRARY" matches the prefix, but the next byte is ' '
        // (0x20), not 0x1A -> BadMagic. This pins the current strict rule.
        assert!(check_magic(&sample_idx(), LibKind::Lib).is_err());
    }

    #[test]
    fn truncated_is_error() {
        assert_eq!(
            check_magic(b"DEVICE LIB", LibKind::Lib),
            Err(ParseError::Truncated)
        );
    }

    #[test]
    fn string_table_finds_fields() {
        let t = string_table(&sample_idx(), LibKind::Idx).unwrap();
        let texts: Vec<&str> = t.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"Arduino"));
        assert!(texts.contains(&"www.TheEngineeringProjects.com"));
        assert!(texts.contains(&"Arduino MEGA 2560"));
    }

    #[test]
    fn real_idx_sample_head_validates() {
        // First 21 bytes of the shipped ArduinoTEP.IDX (443 bytes).
        let head = b"DEVICE LIBRARY INDEX\x1a\0\0";
        assert_eq!(check_magic(head, LibKind::Idx), Ok(21));
    }
}
