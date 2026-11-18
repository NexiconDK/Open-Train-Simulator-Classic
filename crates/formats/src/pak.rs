//! `Blueprints.pak` blueprint caches.
//!
//! The game writes one of these per asset product: a flat container holding a
//! copy of every blueprint `.bin` of that product, so that the whole library
//! can be indexed without touching thousands of small files. The loose files
//! (or the `.ap`) remain the source of truth; a `.pak` can be stale.
//!
//! Layout (little-endian):
//!
//! ```text
//! 0   "2ULB"
//! 4   u32 version (1)
//! 8   u32 entry count
//! 12  u32 total file size
//! 16  u32 size of the largest entry
//! 20  entry count x 24 bytes:
//!       u32 name length   u32 name offset
//!       u32 data size     u32 data offset
//!       u64 modification time (Windows FILETIME)
//! ..  names, back to back, `\` separated, no terminators
//! ..  data, back to back (each entry is a SERZ file)
//! ```

use crate::error::{Error, Result};

pub const MAGIC: &[u8; 4] = b"2ULB";

const HEADER_LEN: usize = 20;
const ENTRY_LEN: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub struct PakEntry {
    /// Path relative to the product folder, e.g. `Scenery\Alustar.bin`.
    pub name: String,
    pub offset: usize,
    pub size: usize,
    /// Windows FILETIME: 100 ns ticks since 1601-01-01 UTC.
    pub modified: u64,
}

pub struct Pak {
    data: Vec<u8>,
    entries: Vec<PakEntry>,
}

impl Pak {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() < HEADER_LEN || &data[..4] != MAGIC {
            return Err(Error::malformed(0, "missing 2ULB magic"));
        }
        let version = u32_at(&data, 4)?;
        if version != 1 {
            return Err(Error::malformed(
                4,
                format!("unsupported pak version {version}"),
            ));
        }
        let count = u32_at(&data, 8)? as usize;
        let table_end = count
            .checked_mul(ENTRY_LEN)
            .and_then(|n| n.checked_add(HEADER_LEN))
            .filter(|&end| end <= data.len())
            .ok_or_else(|| Error::malformed(8, "entry table runs past end of file"))?;

        let mut entries = Vec::with_capacity(count);
        for at in (HEADER_LEN..table_end).step_by(ENTRY_LEN) {
            let name_len = u32_at(&data, at)? as usize;
            let name_off = u32_at(&data, at + 4)? as usize;
            let size = u32_at(&data, at + 8)? as usize;
            let offset = u32_at(&data, at + 12)? as usize;
            let modified = u64::from_le_bytes(data[at + 16..at + 24].try_into().unwrap());
            let name = slice(&data, name_off, name_len)
                .ok_or_else(|| Error::malformed(at, "entry name out of range"))?;
            slice(&data, offset, size)
                .ok_or_else(|| Error::malformed(at + 8, "entry data out of range"))?;
            entries.push(PakEntry {
                name: decode_name(name),
                offset,
                size,
                modified,
            });
        }
        Ok(Pak { data, entries })
    }

    pub fn entries(&self) -> &[PakEntry] {
        &self.entries
    }

    /// The bytes of one entry.
    pub fn data(&self, entry: &PakEntry) -> &[u8] {
        &self.data[entry.offset..entry.offset + entry.size]
    }

    /// Find an entry by name, ignoring case and slash direction.
    pub fn find(&self, name: &str) -> Option<&PakEntry> {
        let wanted = crate::ap::normalize(name);
        self.entries
            .iter()
            .find(|e| crate::ap::normalize(&e.name) == wanted)
    }
}

fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    slice(data, at, 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| Error::malformed(at, "unexpected end of file"))
}

fn slice(data: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    data.get(offset..offset.checked_add(len)?)
}

fn decode_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        let names_start = HEADER_LEN + files.len() * ENTRY_LEN;
        let names_len: usize = files.iter().map(|(n, _)| n.len()).sum();
        let mut table = Vec::new();
        let mut names = Vec::new();
        let mut blobs = Vec::new();
        for (name, data) in files {
            table.extend_from_slice(&(name.len() as u32).to_le_bytes());
            table.extend_from_slice(&((names_start + names.len()) as u32).to_le_bytes());
            table.extend_from_slice(&(data.len() as u32).to_le_bytes());
            table
                .extend_from_slice(&((names_start + names_len + blobs.len()) as u32).to_le_bytes());
            table.extend_from_slice(&0x01DC_264F_671C_9828u64.to_le_bytes());
            names.extend_from_slice(name.as_bytes());
            blobs.extend_from_slice(data);
        }
        let total = names_start + names.len() + blobs.len();
        let largest = files.iter().map(|(_, d)| d.len()).max().unwrap_or(0);
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(largest as u32).to_le_bytes());
        out.extend(table);
        out.extend(names);
        out.extend(blobs);
        out
    }

    #[test]
    fn reads_entries() {
        let pak = Pak::parse(build(&[
            (r"Scenery\Alustar.bin", b"SERZ-one"),
            (r"Scenery\RibeDK.bin", b"SERZ-two!"),
        ]))
        .unwrap();
        assert_eq!(pak.entries().len(), 2);
        assert_eq!(pak.entries()[1].name, r"Scenery\RibeDK.bin");
        let entry = pak.find("scenery/alustar.BIN").unwrap();
        assert_eq!(pak.data(entry), b"SERZ-one");
    }

    #[test]
    fn rejects_out_of_range_tables() {
        let mut bytes = build(&[(r"a.bin", b"SERZ")]);
        assert!(Pak::parse(bytes[..30].to_vec()).is_err());
        bytes[8] = 0xFF; // absurd entry count
        bytes[9] = 0xFF;
        assert!(Pak::parse(bytes).is_err());
        assert!(Pak::parse(b"nope".to_vec()).is_err());
    }
}
