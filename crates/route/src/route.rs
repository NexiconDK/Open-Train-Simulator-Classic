//! One route: its name and typed access to its data files.

use std::path::Path;

use opentsc_formats::install::localised;
use opentsc_formats::{parse_document, xml};

use crate::files::RouteFiles;
use crate::terrain::TerrainTile;
use crate::tile::TileCoord;
use crate::track::{parse_track_network, TrackNetwork, TrackTile};
use crate::Result;

const TRACK_TILES: &str = "Networks/Track Tiles";
const TERRAIN_TILES: &str = "Terrain";

pub struct Route {
    /// Folder name under `Content/Routes`.
    pub id: String,
    /// Display name from `RouteProperties.xml`.
    pub name: Option<String>,
    files: RouteFiles,
}

impl Route {
    /// Open the route stored in `dir` (a folder under `Content/Routes`).
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let files = RouteFiles::open(dir)?;
        let properties = xml::parse_bytes(&files.read("RouteProperties.xml")?)?;
        let name = properties
            .root()
            .and_then(|root| localised(root, "DisplayName"));
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Route { id, name, files })
    }

    pub fn files(&self) -> &RouteFiles {
        &self.files
    }

    /// Tiles that have track geometry, sorted.
    pub fn track_tile_coords(&self) -> Result<Vec<TileCoord>> {
        self.tile_coords(TRACK_TILES)
    }

    pub fn track_tile(&self, coord: TileCoord) -> Result<TrackTile> {
        let data = self
            .files
            .read(&format!("{TRACK_TILES}/{}", coord.file_name()))?;
        TrackTile::parse(&parse_document(&data)?)
    }

    /// Ribbons and nodes of the route, from `Networks/Tracks.bin`.
    pub fn track_network(&self) -> Result<TrackNetwork> {
        parse_track_network(&parse_document(&self.files.read("Networks/Tracks.bin")?)?)
    }

    /// Tiles that have terrain heights, sorted.
    pub fn terrain_tile_coords(&self) -> Result<Vec<TileCoord>> {
        self.tile_coords(TERRAIN_TILES)
    }

    pub fn terrain_tile(&self, coord: TileCoord) -> Result<TerrainTile> {
        let data = self
            .files
            .read(&format!("{TERRAIN_TILES}/{}", coord.file_name()))?;
        TerrainTile::parse(&parse_document(&data)?)
    }

    fn tile_coords(&self, folder: &str) -> Result<Vec<TileCoord>> {
        let mut coords: Vec<TileCoord> = self
            .files
            .list(folder)?
            .iter()
            .filter_map(|name| TileCoord::from_file_name(name))
            .collect();
        coords.sort();
        Ok(coords)
    }
}
