//! Reader for Proteus `.SML` SPICE library containers.
//!
//! Observed facts (`MODELS/ANALOG.SML`, 12711 bytes):
//! - Magic `SPICE LIBRARY\x1a`, then `u32` data-size-ish fields,
//!   `u32` deck count at `0x18`, 32-byte TOC entries from `0x20`:
//!   `name[16]` NUL-padded + `u32` zero + `u32` chunk offset +
//!   `u32` timestamp + `u32` unknown (ignored).
//! - Each chunk: `u32` little-endian total length (including these
//!   4 bytes) + ASCII SPICE text (`.SUBCKT ...` ... `.ENDS`), chunks
//!   packed contiguously.
//!
//! The container layout is shared with `.LML` files; the split logic
//! lives in [`crate::container`]. SPICE grammar (`.SUBCKT`/`.MODEL`/
//! elements) lives in [`crate::spice`].

pub use crate::container::{Deck, ParseError};

/// Container magic (without trailing `0x1A`).
pub const SML_MAGIC: &[u8] = b"SPICE LIBRARY";

/// Split a `.SML` container into named decks.
///
/// # Errors
///
/// [`ParseError::BadHeader`] on bad magic/TOC,
/// [`ParseError::TruncatedChunk`] when a chunk overruns the file.
pub fn split(data: &[u8]) -> Result<Vec<Deck>, ParseError> {
    crate::container::split(data, SML_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = Vec::from(b"SPICE LIBRARY\x1a\0\0".as_slice());
        v.extend_from_slice(&[0u8; 8]); // pad to 0x18
        v.extend_from_slice(&[2u8, 0, 0, 0]); // count
        v.extend_from_slice(&[0u8; 4]); // pad to 0x20
        // entry 0 @0x20: name + zero + offset + ts + ?
        let mut e0 = [0u8; 32];
        e0[..5].copy_from_slice(b"LM123");
        e0[20..24].copy_from_slice(&128u32.to_le_bytes());
        v.extend_from_slice(&e0);
        let mut e1 = [0u8; 32];
        e1[..6].copy_from_slice(b"LM338A");
        e1[20..24].copy_from_slice(&160u32.to_le_bytes());
        v.extend_from_slice(&e1);
        // pad to offset 128, then two chunks
        v.resize(128, 0);
        let d0 = b".SUBCKT LM123\nR1 1 2 1K\n.ENDS\n";
        v.extend_from_slice(&u32::try_from(d0.len() + 4).unwrap().to_le_bytes());
        v.extend_from_slice(d0);
        v.resize(160, 0);
        let d1 = b".SUBCKT LM338A\n.ENDS\n";
        v.extend_from_slice(&u32::try_from(d1.len() + 4).unwrap().to_le_bytes());
        v.extend_from_slice(d1);
        v
    }

    #[test]
    fn splits_sample() {
        let decks = split(&sample()).unwrap();
        assert_eq!(decks.len(), 2);
        assert_eq!(decks[0].name, "LM123");
        assert!(decks[0].text.contains(".SUBCKT LM123"));
        assert_eq!(decks[1].name, "LM338A");
    }

    #[test]
    fn bad_magic_rejected() {
        assert_eq!(split(b"garbage").unwrap_err(), ParseError::BadHeader);
    }
}
