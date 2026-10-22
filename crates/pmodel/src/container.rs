//! Shared splitter for Proteus library containers (`.SML` and `.LML`).
//!
//! Observed facts (`MODELS/ANALOG.SML`, `MODELS/ACTIVE.LML`):
//! - Magic `<KIND> LIBRARY\x1a` (`SPICE LIBRARY`, `MODEL LIBRARY`), then
//!   `u32` data-size-ish fields, `u32` deck count at `0x18`, 32-byte TOC
//!   entries from `0x20`: `name[16]` NUL-padded + `u32` zero +
//!   `u32` chunk offset + `u32` timestamp + `u32` unknown (ignored).
//! - Each chunk: `u32` little-endian total length (including these
//!   4 bytes) + payload text, chunks packed contiguously. Payload is
//!   SPICE (`.SUBCKT ... .ENDS`) in `.SML`, LISA model descriptions in
//!   `.LML`.
//!
//! v0 scope: container split into named decks. Payload grammars live in
//! [`crate::spice`] (SPICE) and the crate root ([`crate::parse`], LISA).

use std::fmt;

/// One library deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deck {
    /// TOC name (`LM123`, `ACIMETER`).
    pub name: String,
    /// Raw deck text.
    pub text: String,
}

/// Split error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Bad magic or truncated header/TOC.
    BadHeader,
    /// A chunk runs past the end of the file.
    TruncatedChunk {
        /// Deck index.
        index: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHeader => write!(f, "bad library header"),
            Self::TruncatedChunk { index } => {
                write!(f, "truncated chunk {index}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

const TOC_BASE: usize = 0x20;
const TOC_ENTRY: usize = 32;

fn u32le(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Split a library container into named decks.
///
/// `magic` is the expected container magic without the trailing `0x1A`
/// (e.g. [`crate::sml::SML_MAGIC`], [`crate::lml::LML_MAGIC`]).
///
/// # Errors
///
/// [`ParseError::BadHeader`] on bad magic/TOC,
/// [`ParseError::TruncatedChunk`] when a chunk overruns the file.
pub fn split(data: &[u8], magic: &[u8]) -> Result<Vec<Deck>, ParseError> {
    if data.len() < TOC_BASE
        || data.get(..magic.len()) != Some(magic)
        || data.get(magic.len()) != Some(&0x1A)
    {
        return Err(ParseError::BadHeader);
    }
    let count = u32le(data, 0x18).ok_or(ParseError::BadHeader)? as usize;
    if count > 4096 {
        return Err(ParseError::BadHeader);
    }
    let mut decks = Vec::with_capacity(count.min(256));
    for i in 0..count {
        let base = TOC_BASE + i * TOC_ENTRY;
        let raw = data
            .get(base..base + TOC_ENTRY)
            .ok_or(ParseError::BadHeader)?;
        let name = String::from_utf8_lossy(&raw[..16])
            .trim_end_matches('\0')
            .to_owned();
        let offset = u32le(data, base + 20).ok_or(ParseError::BadHeader)? as usize;
        let total = u32le(data, offset).ok_or(ParseError::TruncatedChunk { index: i })? as usize;
        if total < 4 {
            return Err(ParseError::TruncatedChunk { index: i });
        }
        let text = data
            .get(offset + 4..offset + total)
            .ok_or(ParseError::TruncatedChunk { index: i })?;
        decks.push(Deck {
            name,
            text: String::from_utf8_lossy(text).into_owned(),
        });
    }
    Ok(decks)
}
