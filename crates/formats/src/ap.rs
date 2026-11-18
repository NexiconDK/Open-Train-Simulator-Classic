//! `.ap` asset archives.
//!
//! An `.ap` file is an ordinary ZIP archive (entries are usually stored
//! without compression) holding the files of one asset product. Entry names
//! use `/`, while paths inside blueprints use `\` and are not consistent about
//! letter case, so lookups here ignore both differences.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::error::{Error, Result};

/// Refuse to inflate a single entry beyond this size.
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct ApArchive<R: Read + Seek = BufReader<File>> {
    zip: zip::ZipArchive<R>,
    /// File names as stored, sorted.
    names: Vec<String>,
    /// Normalised entry name -> index in the archive.
    index: HashMap<String, usize>,
}

impl ApArchive<BufReader<File>> {
    /// Open an archive on disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> ApArchive<R> {
    pub fn new(reader: R) -> Result<Self> {
        let zip = zip::ZipArchive::new(reader).map_err(archive_error)?;
        let mut names = Vec::with_capacity(zip.len());
        let mut index = HashMap::with_capacity(zip.len());
        for i in 0..zip.len() {
            let Some(name) = zip.name_for_index(i) else {
                continue;
            };
            let name = name.map_err(archive_error)?;
            if name.ends_with('/') {
                continue; // directory entry
            }
            index.insert(normalize(&name), i);
            names.push(name.into_owned());
        }
        names.sort_unstable();
        Ok(ApArchive { zip, names, index })
    }

    /// Number of files (directories not counted).
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Entry names as stored in the archive, sorted.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// True if the archive holds `name` (case and slash direction ignored).
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&normalize(name))
    }

    /// Read one file completely.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let index = *self
            .index
            .get(&normalize(name))
            .ok_or_else(|| Error::NotFound(format!("'{name}' in archive")))?;
        let mut entry = self.zip.by_index(index).map_err(archive_error)?;
        read_entry(&mut entry)
    }

    /// Read `name` only if its content starts with `magic`. Lets a caller
    /// sift through an archive without loading every texture into memory.
    pub fn read_if_magic(&mut self, name: &str, magic: &[u8]) -> Result<Option<Vec<u8>>> {
        let index = *self
            .index
            .get(&normalize(name))
            .ok_or_else(|| Error::NotFound(format!("'{name}' in archive")))?;
        let mut entry = self.zip.by_index(index).map_err(archive_error)?;
        let mut head = vec![0u8; magic.len()];
        let mut filled = 0;
        while filled < head.len() {
            match entry.read(&mut head[filled..])? {
                0 => return Ok(None),
                n => filled += n,
            }
        }
        if head != magic {
            return Ok(None);
        }
        let mut rest = read_entry(&mut entry)?;
        head.append(&mut rest);
        Ok(Some(head))
    }
}

fn read_entry(entry: &mut impl Read) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let read = entry.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut data)?;
    if read as u64 > MAX_ENTRY_BYTES {
        return Err(Error::Archive("entry is larger than 2 GiB".into()));
    }
    Ok(data)
}

/// Lower-case, forward slashes, no leading slash.
pub(crate) fn normalize(name: &str) -> String {
    name.trim_start_matches(['/', '\\'])
        .replace('\\', "/")
        .to_lowercase()
}

fn archive_error(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(io) => Error::Io(io),
        other => Error::Archive(other.to_string()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A minimal stored-only ZIP writer, enough to build test archives
    /// without pulling the writing half of the zip crate into the build.
    pub(crate) fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let mut header = Vec::new();
            header.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, flags, method, time, date
            header.extend_from_slice(&crc.to_le_bytes());
            header.extend_from_slice(&(data.len() as u32).to_le_bytes());
            header.extend_from_slice(&(data.len() as u32).to_le_bytes());
            header.extend_from_slice(&(name.len() as u16).to_le_bytes());
            header.extend_from_slice(&0u16.to_le_bytes()); // extra length
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&header);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&[20, 0]); // version made by
            central.extend_from_slice(&header);
            central.extend_from_slice(&[0; 10]); // comment len, disk, attributes
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let central_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
            }
        }
        !crc
    }

    #[test]
    fn lookup_ignores_case_and_slash_direction() {
        let bytes = stored_zip(&[
            ("RailNetwork/Signals/Sig.bin", b"SERZdata"),
            ("RailNetwork/Signals/readme.txt", b"hello"),
        ]);
        let mut ap = ApArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(ap.len(), 2);
        assert!(ap.contains(r"railnetwork\signals\SIG.BIN"));
        assert_eq!(
            ap.read(r"RailNetwork\Signals\sig.bin").unwrap(),
            b"SERZdata"
        );
        assert!(matches!(ap.read("missing.bin"), Err(Error::NotFound(_))));
        assert_eq!(
            ap.read_if_magic("RailNetwork/Signals/Sig.bin", b"SERZ")
                .unwrap()
                .as_deref(),
            Some(&b"SERZdata"[..])
        );
        assert_eq!(
            ap.read_if_magic("RailNetwork/Signals/readme.txt", b"SERZ")
                .unwrap(),
            None
        );
    }
}
