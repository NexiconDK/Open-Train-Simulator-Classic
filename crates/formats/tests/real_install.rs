//! Tests against a real installation.
//!
//! Game files are never committed to this repository, so these tests only do
//! something when `OPENTSC_RAILWORKS` points at your own RailWorks folder:
//!
//! ```text
//! OPENTSC_RAILWORKS="D:\SteamLibrary\steamapps\common\RailWorks" cargo test
//! ```

use std::fs;
use std::path::PathBuf;

use opentsc_formats::install::resolve;
use opentsc_formats::{parse_document, Install};

fn railworks() -> Option<PathBuf> {
    match std::env::var_os("OPENTSC_RAILWORKS") {
        Some(path) => Some(PathBuf::from(path)),
        None => {
            eprintln!("OPENTSC_RAILWORKS is not set; skipping");
            None
        }
    }
}

#[test]
fn lists_routes() {
    let Some(root) = railworks() else { return };
    let install = Install::open(root).expect("open installation");
    let routes = install.routes().expect("list routes");
    assert!(!routes.is_empty(), "no routes found");
    let unnamed: Vec<_> = routes
        .iter()
        .filter(|r| r.name.is_none())
        .map(|r| &r.id)
        .collect();
    assert!(
        unnamed.is_empty(),
        "routes without a readable name: {unnamed:?}"
    );
}

#[test]
fn parses_the_track_network_of_every_unpacked_route() {
    let Some(root) = railworks() else { return };
    let install = Install::open(root).expect("open installation");
    for route in install
        .routes()
        .expect("list routes")
        .iter()
        .filter(|r| !r.packed)
    {
        let Some(tracks) = resolve(&route.path, "Networks/Tracks.bin") else {
            continue;
        };
        let data = fs::read(&tracks).expect("read Tracks.bin");
        let doc = parse_document(&data).unwrap_or_else(|e| panic!("{}: {e}", tracks.display()));
        let root = doc.root().expect("root element");
        assert!(
            root.find("Network::cTrackNetwork").is_some(),
            "{}: no track network inside",
            tracks.display()
        );
    }
}
