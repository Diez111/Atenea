//! Readers for Proteus `.pdsprj` project containers.
//!
//! Observed facts (`SAMPLES/Tutorials/Isistut.pdsprj`, 20 KB):
//! - Standard ZIP (deflated) with exactly `PROJECT.XML`, `ROOT.CDB`, `ROOT.DSN`.
//! - `PROJECT.XML`: `<?xml ...?><PROJECT TITLE="...">` + `<TIMESTAMP
//!   RELEASE / FILEVER / MODIFIED / CUSTID / CREATED/>` + `<SCRIPTS/>`.
//! - `ROOT.DSN`: regular schematic binary (`plib::dsn` validates it).
//! - `ROOT.CDB`: little-endian binary config DB, kept opaque in v0.
//!
//! v0 scope: open by member name, parse project title/timestamp,
//! validate the embedded DSN. CDB decoding is pending deeper RE.

use std::fmt;
use std::io::{Read, Seek};

pub mod cdb;

/// Opened project container.
///
/// Two flavors exist in the wild: schematic projects (`<PROJECT TITLE>`,
/// `ROOT.CDB` + `ROOT.DSN`) and firmware workspaces (no `TITLE`,
/// `FIRMWARE.XML` + `FIRMWARE/...` tree). Absent members stay `None`.
#[derive(Debug, Clone)]
pub struct Project {
    /// `TITLE` attribute of `<PROJECT>` (absent in firmware workspaces).
    pub title: Option<String>,
    /// Attributes of `<TIMESTAMP>`.
    pub timestamp: Timestamp,
    /// Raw `ROOT.DSN` bytes (schematic projects).
    pub dsn: Option<Vec<u8>>,
    /// Raw `ROOT.CDB` bytes (opaque in v0).
    pub cdb: Option<Vec<u8>>,
    /// Raw `FIRMWARE.XML` bytes (firmware workspaces).
    pub firmware_xml: Option<Vec<u8>>,
    /// All member names in archive order.
    pub members: Vec<String>,
}

/// `<TIMESTAMP>` attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timestamp {
    /// `RELEASE` attribute.
    pub release: String,
    /// `FILEVER` attribute.
    pub filever: String,
    /// `MODIFIED` attribute (unix epoch as string).
    pub modified: String,
    /// `CUSTID` attribute.
    pub custid: String,
    /// `CREATED` attribute (unix epoch as string).
    pub created: String,
}

/// Open error.
#[derive(Debug)]
pub enum OpenError {
    /// Underlying ZIP/container I/O failure.
    Zip(String),
    /// A required member is missing.
    Missing(&'static str),
    /// `PROJECT.XML` is not parseable.
    Xml(String),
    /// Embedded DSN failed validation.
    BadDsn(plib::ParseError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(e) => write!(f, "zip error: {e}"),
            Self::Missing(m) => write!(f, "missing member: {m}"),
            Self::Xml(e) => write!(f, "bad PROJECT.XML: {e}"),
            Self::BadDsn(e) => write!(f, "bad ROOT.DSN: {e}"),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BadDsn(e) => Some(e),
            Self::Zip(_) | Self::Missing(_) | Self::Xml(_) => None,
        }
    }
}

/// Cap for a single decompressed member (zip-bomb guard).
const MAX_MEMBER_BYTES: u64 = 256 * 1024 * 1024;

/// Open a `.pdsprj` container from any readable + seekable source.
///
/// `PROJECT.XML` is required; `ROOT.CDB`, `ROOT.DSN` and `FIRMWARE.XML`
/// are optional (schematic vs firmware-workspace flavors).
///
/// # Errors
///
/// [`OpenError::Missing`] when `PROJECT.XML` is absent,
/// [`OpenError::Xml`] on malformed `PROJECT.XML`,
/// [`OpenError::BadDsn`] when an embedded DSN header is invalid.
pub fn open<R: Read + Seek>(reader: R) -> Result<Project, OpenError> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| OpenError::Zip(e.to_string()))?;
    let members: Vec<String> = zip.file_names().map(str::to_owned).collect();

    let xml = read_member(&mut zip, "PROJECT.XML")?;
    let cdb = read_member_opt(&mut zip, "ROOT.CDB")?;
    let dsn = read_member_opt(&mut zip, "ROOT.DSN")?;
    let firmware_xml = read_member_opt(&mut zip, "FIRMWARE.XML")?;

    let (title, timestamp) = parse_project_xml(&xml)?;
    if let Some(ref dsn) = dsn {
        plib::dsn::check_magic(dsn).map_err(OpenError::BadDsn)?;
    }

    Ok(Project {
        title,
        timestamp,
        dsn,
        cdb,
        firmware_xml,
        members,
    })
}

/// Required member reader.
fn read_member<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, OpenError> {
    let file = zip.by_name(name).map_err(|_| {
        OpenError::Missing(match name {
            "PROJECT.XML" => "PROJECT.XML",
            "ROOT.CDB" => "ROOT.CDB",
            "ROOT.DSN" => "ROOT.DSN",
            _ => "unknown",
        })
    })?;
    let mut buf = Vec::new();
    file.take(MAX_MEMBER_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| OpenError::Zip(e.to_string()))?;
    if buf.len() as u64 > MAX_MEMBER_BYTES {
        return Err(OpenError::Zip(format!("{name} exceeds size cap")));
    }
    Ok(buf)
}

fn read_member_opt<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>, OpenError> {
    match read_member(zip, name) {
        Ok(buf) => Ok(Some(buf)),
        Err(OpenError::Missing(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn parse_project_xml(xml: &[u8]) -> Result<(Option<String>, Timestamp), OpenError> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut title = None;
    let mut timestamp = Timestamp::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e) | Event::Start(ref e)) => {
                let attrs: Vec<(String, String)> = e
                    .attributes()
                    .filter_map(Result::ok)
                    .map(|a| {
                        let value = a.unescape_value().map_or_else(
                            |_| String::from_utf8_lossy(a.value.as_ref()).into_owned(),
                            std::borrow::Cow::into_owned,
                        );
                        (String::from_utf8_lossy(a.key.as_ref()).into_owned(), value)
                    })
                    .collect();
                let get = |k: &str| attrs.iter().find(|(ak, _)| ak == k).map(|(_, v)| v.clone());
                match e.name().as_ref() {
                    b"PROJECT" => title = get("TITLE"),
                    b"TIMESTAMP" => {
                        timestamp = Timestamp {
                            release: get("RELEASE").unwrap_or_default(),
                            filever: get("FILEVER").unwrap_or_default(),
                            modified: get("MODIFIED").unwrap_or_default(),
                            custid: get("CUSTID").unwrap_or_default(),
                            created: get("CREATED").unwrap_or_default(),
                        };
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OpenError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok((title, timestamp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Write as _;

    const XML: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes'?>
<PROJECT TITLE="ISIS Tutorial Circuit">
 <TIMESTAMP RELEASE="805" FILEVER="806" MODIFIED="1455289047" CUSTID="1210869955" CREATED="1455289045"/>
 <SCRIPTS/>
</PROJECT>"#;

    /// Build a synthetic `.pdsprj` in memory (mirrors the observed layout).
    fn sample_pdsprj() -> Vec<u8> {
        let mut dsn = Vec::from(b"ISIS SCHEMATIC FILE\x1aTest\0".as_slice());
        dsn.extend_from_slice(b"{*DEVICE} PREFIX=U\0");
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in [
                ("PROJECT.XML", XML.as_bytes()),
                ("ROOT.CDB", b"\x04\x00\x00\x00ROOT".as_slice()),
                ("ROOT.DSN", dsn.as_slice()),
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn opens_sample() {
        let raw = sample_pdsprj();
        let proj = open(Cursor::new(raw)).unwrap();
        assert_eq!(proj.title.as_deref(), Some("ISIS Tutorial Circuit"));
        assert_eq!(proj.timestamp.release, "805");
        assert_eq!(proj.timestamp.filever, "806");
        assert_eq!(proj.timestamp.modified, "1455289047");
        assert_eq!(proj.timestamp.custid, "1210869955");
        assert_eq!(proj.timestamp.created, "1455289045");
        assert_eq!(proj.members.len(), 3);
        let (dsn, cdb) = (proj.dsn.unwrap(), proj.cdb.unwrap());
        assert!(!dsn.is_empty() && !cdb.is_empty());
        // Embedded DSN parses through plib too.
        let blocks = plib::dsn::blocks(&dsn);
        assert_eq!(blocks[0].head, "*DEVICE");
    }

    #[test]
    fn firmware_workspace_has_no_title() {
        let xml = br#"<?xml version='1.0' encoding='UTF-8'?>
<PROJECT>
 <TIMESTAMP RELEASE="802" MODIFIED="1415714307" FILEVER="805" CREATED="1371464270"/>
 <SCRIPTS/>
</PROJECT>"#;
        let (title, ts) = parse_project_xml(xml).unwrap();
        assert_eq!(title, None);
        assert_eq!(ts.release, "802");
    }

    #[test]
    fn missing_project_xml_is_error() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("ROOT.DSN", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"ISIS SCHEMATIC FILE\x1aX").unwrap();
            zip.finish().unwrap();
        }
        let err = open(Cursor::new(buf.into_inner())).unwrap_err();
        assert!(matches!(err, OpenError::Missing("PROJECT.XML")));
    }

    #[test]
    fn optional_members_default_to_none() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("PROJECT.XML", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(XML.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let proj = open(Cursor::new(buf.into_inner())).unwrap();
        assert!(proj.dsn.is_none() && proj.cdb.is_none());
    }

    #[test]
    fn bad_xml_is_error() {
        let mut raw = sample_pdsprj();
        // Corrupt the XML member beyond repair is hard in a zip;
        // instead assert the parser rejects a non-XML doc directly.
        let _ = &mut raw;
        assert!(parse_project_xml(b"<broken").is_err());
    }
}
