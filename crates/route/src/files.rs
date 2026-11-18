//! Access to the files of one route, wherever they are stored.
//!
//! A route folder holds its files loose, inside `.ap` archives
//! (`MainContent.ap`), or both. Loose files win, the same way they do for
//! assets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use opentsc_formats::ap::ApArchive;
use opentsc_formats::install::resolve;

use crate::{Error, Result};

pub struct RouteFiles {
    dir: PathBuf,
    archives: Vec<Archive>,
}

struct Archive {
    names: Vec<String>,
    reader: Mutex<ApArchive>,
}

impl RouteFiles {
    /// Open the route stored in `dir`. Nothing is read yet apart from the
    /// directories of the archives found there.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ap")))
            .collect();
        paths.sort();
        let mut archives = Vec::with_capacity(paths.len());
        for path in paths {
            let ap = ApArchive::open(&path)?;
            archives.push(Archive {
                names: ap.names().to_vec(),
                reader: Mutex::new(ap),
            });
        }
        Ok(RouteFiles { dir, archives })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// True if any of the route's files come from an archive.
    pub fn is_packed(&self) -> bool {
        !self.archives.is_empty()
    }

    /// Read one file by its path inside the route, e.g.
    /// `Networks/Tracks.bin`. Letter case and slash direction do not matter.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        if let Some(loose) = resolve(&self.dir, path).filter(|p| p.is_file()) {
            return Ok(fs::read(loose)?);
        }
        for archive in &self.archives {
            let mut reader = archive.reader.lock().unwrap_or_else(|p| p.into_inner());
            if reader.contains(path) {
                return Ok(reader.read(path)?);
            }
        }
        Err(Error::Missing(path.to_string()))
    }

    /// Names of the files directly inside `folder` (not in subfolders),
    /// sorted. An absent folder gives an empty list.
    pub fn list(&self, folder: &str) -> Result<Vec<String>> {
        // Keyed by lower-case name so a loose file hides its archived twin.
        let mut found: BTreeMap<String, String> = BTreeMap::new();
        let prefix = normalize(folder);
        for archive in &self.archives {
            for name in &archive.names {
                let normal = normalize(name);
                let Some(rest) = normal
                    .strip_prefix(&prefix)
                    .and_then(|r| r.strip_prefix('/'))
                else {
                    continue;
                };
                if !rest.is_empty() && !rest.contains('/') {
                    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
                    found.insert(rest.to_string(), file.to_string());
                }
            }
        }
        if let Some(loose) = resolve(&self.dir, folder).filter(|p| p.is_dir()) {
            for entry in fs::read_dir(loose)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    found.insert(name.to_lowercase(), name);
                }
            }
        }
        Ok(found.into_values().collect())
    }
}

fn normalize(path: &str) -> String {
    path.trim_matches(['/', '\\'])
        .replace('\\', "/")
        .to_lowercase()
}
