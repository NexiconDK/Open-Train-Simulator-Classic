//! Finding things inside a RailWorks installation folder.
//!
//! ```text
//! RailWorks/
//!   Assets/<Provider>/<Product>/...      loose files and/or *.ap archives
//!   Content/Routes/<route guid>/...      RouteProperties.xml, tiles, scenarios
//! ```
//!
//! Content refers to assets by a *(provider, product, path)* triple, where the
//! path uses `\` and names the blueprint's XML source even though only the
//! compiled `.bin` is installed. Nothing here ever writes to the folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::ap::ApArchive;
use crate::error::{Error, Result};
use crate::node::{Document, Element};

/// Parse a file that may be either `SERZ` binary or XML.
pub fn parse_document(data: &[u8]) -> Result<Document> {
    if crate::serz::is_serz(data) {
        crate::serz::parse(data)
    } else {
        crate::xml::parse_bytes(data)
    }
}

pub struct Install {
    root: PathBuf,
    /// Archives opened so far. Reading the directory of a multi-gigabyte
    /// `.ap` is slow enough that it should happen once.
    archives: Mutex<HashMap<PathBuf, ApArchive>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteInfo {
    /// Folder name under `Content/Routes`, normally a GUID.
    pub id: String,
    /// Display name, if the route's properties could be read.
    pub name: Option<String>,
    /// True if the route's files live inside an `.ap` in its folder.
    pub packed: bool,
    pub path: PathBuf,
}

impl Install {
    /// Open an installation. `root` is the folder that contains `Assets` and
    /// `Content` (the one with `RailWorks.exe` in it).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for required in ["Assets", "Content"] {
            if resolve(&root, required).is_none() {
                return Err(Error::NotFound(format!(
                    "'{}' has no {required} folder; is it a RailWorks installation?",
                    root.display()
                )));
            }
        }
        Ok(Install {
            root,
            archives: Mutex::new(HashMap::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every folder under `Content/Routes` that holds a route, sorted by name
    /// then id. Folders with scenarios but no route (left behind by add-ons
    /// for routes that are not installed) are skipped.
    pub fn routes(&self) -> Result<Vec<RouteInfo>> {
        let Some(routes_dir) = resolve(&self.root, "Content/Routes") else {
            return Ok(Vec::new());
        };
        let mut routes = Vec::new();
        for entry in fs::read_dir(routes_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let Some((properties, packed)) = self.route_properties(&path)? else {
                continue;
            };
            let name = crate::xml::parse_bytes(&properties)
                .ok()
                .and_then(|doc| doc.root().and_then(|root| localised(root, "DisplayName")));
            routes.push(RouteInfo {
                id: entry.file_name().to_string_lossy().into_owned(),
                name,
                packed,
                path,
            });
        }
        routes.sort_by(|a, b| (a.name.as_deref(), &a.id).cmp(&(b.name.as_deref(), &b.id)));
        Ok(routes)
    }

    /// `RouteProperties.xml` of the route in `dir`, loose or from an archive.
    fn route_properties(&self, dir: &Path) -> Result<Option<(Vec<u8>, bool)>> {
        const NAME: &str = "RouteProperties.xml";
        if let Some(loose) = resolve(dir, NAME) {
            return Ok(Some((fs::read(loose)?, false)));
        }
        for archive in archives_in(dir)? {
            if let Some(data) = self.read_from_archive(&archive, NAME)? {
                return Ok(Some((data, true)));
            }
        }
        Ok(None)
    }

    /// Read one asset file.
    ///
    /// Search order: a loose file in the product folder, then the `.ap`
    /// archives in the product folder, then `.ap` archives one level up in
    /// the provider folder (those hold paths that start with the product
    /// name). Loose files come first so that patches and reskins can
    /// override packed content.
    pub fn read_asset(&self, provider: &str, product: &str, path: &str) -> Result<Vec<u8>> {
        let not_found = || Error::NotFound(format!(r"asset {provider}\{product}\{path}"));
        let provider_dir = resolve(&self.root, "Assets")
            .and_then(|assets| resolve(&assets, provider))
            .ok_or_else(not_found)?;
        if let Some(product_dir) = resolve(&provider_dir, product).filter(|p| p.is_dir()) {
            if let Some(loose) = resolve(&product_dir, path) {
                if loose.is_file() {
                    return Ok(fs::read(loose)?);
                }
            }
            for archive in archives_in(&product_dir)? {
                if let Some(data) = self.read_from_archive(&archive, path)? {
                    return Ok(data);
                }
            }
        }
        let prefixed = format!("{product}/{path}");
        for archive in archives_in(&provider_dir)? {
            if let Some(data) = self.read_from_archive(&archive, &prefixed)? {
                return Ok(data);
            }
        }
        Err(not_found())
    }

    /// Load the blueprint a `cAbsoluteBlueprintID` points at.
    ///
    /// Blueprint ids name the `.xml` source; the installed file is normally
    /// the `.bin` next to it, so that is tried first.
    pub fn load_blueprint(&self, provider: &str, product: &str, id: &str) -> Result<Document> {
        if let Some(stem) = strip_suffix_ignore_case(id, ".xml") {
            match self.read_asset(provider, product, &format!("{stem}.bin")) {
                Ok(data) => return parse_document(&data),
                Err(Error::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        parse_document(&self.read_asset(provider, product, id)?)
    }

    fn read_from_archive(&self, archive: &Path, name: &str) -> Result<Option<Vec<u8>>> {
        let mut cache = self
            .archives
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !cache.contains_key(archive) {
            cache.insert(archive.to_path_buf(), ApArchive::open(archive)?);
        }
        let ap = cache.get_mut(archive).expect("just inserted");
        if !ap.contains(name) {
            return Ok(None);
        }
        ap.read(name).map(Some)
    }
}

/// Pick the text of a `Localisation::cUserLocalisedString` child of `parent`:
/// English if present, otherwise the first language that has any text.
pub fn localised(parent: &Element, field: &str) -> Option<String> {
    let strings = parent.path(&[field, "Localisation::cUserLocalisedString"])?;
    const LANGUAGES: [&str; 8] = [
        "English", "German", "French", "Italian", "Spanish", "Dutch", "Polish", "Russian",
    ];
    LANGUAGES
        .iter()
        .filter_map(|language| strings.string(language))
        .find(|text| !text.trim().is_empty())
        .map(str::to_string)
}

/// The `.ap` files directly inside `dir`, sorted for a stable search order.
fn archives_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut archives: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ap"))
        })
        .collect();
    archives.sort();
    Ok(archives)
}

/// Resolve `relative` under `base` the way Windows would: either slash
/// direction, any letter case. `..` and absolute parts are refused, because
/// these paths come out of downloaded content.
pub fn resolve(base: &Path, relative: &str) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for part in relative
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
    {
        if part == ".." || part.contains(':') {
            return None;
        }
        let direct = current.join(part);
        current = if direct.exists() {
            direct
        } else {
            let wanted = part.to_lowercase();
            fs::read_dir(&current)
                .ok()?
                .filter_map(|entry| entry.ok())
                .find(|entry| entry.file_name().to_string_lossy().to_lowercase() == wanted)?
                .path()
        };
    }
    Some(current)
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(suffix.len())?;
    (text.is_char_boundary(split) && text[split..].eq_ignore_ascii_case(suffix))
        .then(|| &text[..split])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opentsc-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const ROUTE_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<cRouteProperties xmlns:d="http://www.kuju.com/TnT/2003/Delta" d:version="1.0">
	<DisplayName><Localisation-cUserLocalisedString>
		<English d:type="cDeltaString"></English>
		<German d:type="cDeltaString">Teststrecke</German>
	</Localisation-cUserLocalisedString></DisplayName>
</cRouteProperties>"#;

    #[test]
    fn finds_routes_and_assets_case_insensitively() {
        let root = temp_dir("install");
        fs::create_dir_all(root.join("Content/Routes/abc-123")).unwrap();
        fs::create_dir_all(root.join("Content/Routes/only-scenarios/Scenarios")).unwrap();
        fs::write(
            root.join("Content/Routes/abc-123/RouteProperties.xml"),
            ROUTE_XML,
        )
        .unwrap();
        fs::create_dir_all(root.join("Assets/Kuju/RailSimulator/Scenery")).unwrap();
        fs::write(
            root.join("Assets/Kuju/RailSimulator/Scenery/Tree.xml"),
            ROUTE_XML,
        )
        .unwrap();

        let install = Install::open(&root).unwrap();
        let routes = install.routes().unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].id, "abc-123");
        assert_eq!(routes[0].name.as_deref(), Some("Teststrecke"));
        assert!(!routes[0].packed);

        let data = install
            .read_asset("kuju", "RAILSIMULATOR", r"scenery\tree.XML")
            .unwrap();
        assert_eq!(data, ROUTE_XML.as_bytes());
        // No .bin is installed, so the blueprint falls back to the XML source.
        let doc = install
            .load_blueprint("Kuju", "RailSimulator", r"Scenery\Tree.xml")
            .unwrap();
        assert_eq!(doc.root().unwrap().name, "cRouteProperties");
        assert!(matches!(
            install.read_asset("Kuju", "RailSimulator", r"Scenery\Missing.bin"),
            Err(Error::NotFound(_))
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_assets_from_product_and_provider_archives() {
        use crate::ap::tests::stored_zip;
        let root = temp_dir("archives");
        fs::create_dir_all(root.join("Content")).unwrap();
        fs::create_dir_all(root.join("Assets/RSC/Signals")).unwrap();
        fs::create_dir_all(root.join("Assets/Castle")).unwrap();
        fs::write(
            root.join("Assets/RSC/Signals/SignalsAssets.ap"),
            stored_zip(&[
                ("RailNetwork/Sig.bin", b"packed"),
                ("RailNetwork/Both.bin", b"packed"),
            ]),
        )
        .unwrap();
        fs::create_dir_all(root.join("Assets/RSC/Signals/RailNetwork")).unwrap();
        fs::write(
            root.join("Assets/RSC/Signals/RailNetwork/Both.bin"),
            b"loose",
        )
        .unwrap();
        // An archive in the provider folder: paths start with the product.
        fs::write(
            root.join("Assets/Castle/CastleAssets.ap"),
            stored_zip(&[("Scenery/Station.bin", b"provider-level")]),
        )
        .unwrap();

        let install = Install::open(&root).unwrap();
        assert_eq!(
            install
                .read_asset("RSC", "Signals", r"railnetwork\sig.bin")
                .unwrap(),
            b"packed"
        );
        assert_eq!(
            install
                .read_asset("RSC", "Signals", r"RailNetwork\Both.bin")
                .unwrap(),
            b"loose"
        );
        assert_eq!(
            install
                .read_asset("Castle", "Scenery", "Station.bin")
                .unwrap(),
            b"provider-level"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolve_refuses_to_leave_the_base() {
        let root = temp_dir("resolve");
        fs::create_dir_all(root.join("a")).unwrap();
        assert!(resolve(&root, "a").is_some());
        assert!(resolve(&root.join("a"), r"..\a").is_none());
        assert!(resolve(&root, r"C:\Windows").is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_folders_that_are_not_an_install() {
        let root = temp_dir("not-install");
        assert!(Install::open(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
