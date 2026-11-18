//! Terrain height tiles (`Terrain/±XXXXXX±ZZZZZZ.bin`).
//!
//! Each tile is a grid of 128 × 128 heights in metres, stored as 32-bit
//! floats. Samples are 8 m apart. Rows run south to north and, inside a row,
//! samples run west to east; the first sample sits on the tile's south-west
//! corner. A tile does not repeat its neighbour's edge: the sample after the
//! last one of a row is the first one of the next tile.

use opentsc_formats::{Document, Node};

use crate::tile::{TileCoord, TILE_SIZE};
use crate::track::{number, record};
use crate::{Error, Result};

/// Samples along each edge of a tile.
pub const SAMPLES: usize = 128;

/// Distance between neighbouring samples in metres.
pub const SPACING: f64 = TILE_SIZE / SAMPLES as f64;

#[derive(Debug, Clone, PartialEq)]
pub struct TerrainTile {
    pub coord: TileCoord,
    /// `SAMPLES * SAMPLES` heights, south row first, west to east in a row.
    heights: Vec<f32>,
}

impl TerrainTile {
    pub fn parse(doc: &Document) -> Result<Self> {
        let tile = record(doc)?
            .child("cHeightFieldTile")
            .ok_or_else(|| Error::data("no cHeightFieldTile in the file"))?;
        let coord = TileCoord {
            x: number(tile, "X")? as i32,
            z: number(tile, "Y")? as i32,
        };
        let blob = tile
            .children
            .iter()
            .find_map(|node| match node {
                Node::Blob(bytes) => Some(bytes),
                _ => None,
            })
            .ok_or_else(|| Error::data("terrain tile without height data"))?;
        if blob.len() != SAMPLES * SAMPLES * 4 {
            return Err(Error::data(format!(
                "terrain tile has {} bytes of heights, expected {}",
                blob.len(),
                SAMPLES * SAMPLES * 4
            )));
        }
        let heights = blob
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(TerrainTile { coord, heights })
    }

    /// All heights, south row first.
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// Height of the sample `east` steps from the west edge and `north`
    /// steps from the south edge. Both must be below [`SAMPLES`].
    pub fn sample(&self, east: usize, north: usize) -> f32 {
        self.heights[north * SAMPLES + east]
    }

    /// Lowest and highest sample.
    pub fn range(&self) -> (f32, f32) {
        self.heights
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &h| {
                (lo.min(h), hi.max(h))
            })
    }

    /// Average height of the tile.
    pub fn mean(&self) -> f32 {
        (self.heights.iter().map(|&h| h as f64).sum::<f64>() / self.heights.len() as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentsc_formats::{Element, Value};

    fn tile_document(x: i32, y: i32, blob: Vec<u8>) -> Document {
        let tile = Element {
            name: "cHeightFieldTile".into(),
            id: 1,
            children: vec![
                Node::Value {
                    name: "X".into(),
                    value: Value::I32(x),
                },
                Node::Value {
                    name: "Y".into(),
                    value: Value::I32(y),
                },
                Node::Blob(blob),
            ],
        };
        let record = Element {
            name: "Record".into(),
            id: 0,
            children: vec![Node::Element(tile)],
        };
        let root = Element {
            name: "cRecordSet".into(),
            id: 2,
            children: vec![Node::Element(record)],
        };
        Document {
            nodes: vec![
                Node::Hint {
                    flag: 0,
                    value: 65540,
                },
                Node::Element(root),
            ],
        }
    }

    #[test]
    fn reads_heights_in_row_order() {
        // Height = 100 + east step + 1000 * north step.
        let mut blob = Vec::new();
        for north in 0..SAMPLES {
            for east in 0..SAMPLES {
                blob.extend_from_slice(
                    &(100.0 + east as f32 + 1000.0 * north as f32).to_le_bytes(),
                );
            }
        }
        let tile = TerrainTile::parse(&tile_document(57, -98, blob)).unwrap();
        assert_eq!(tile.coord, TileCoord { x: 57, z: -98 });
        assert_eq!(tile.sample(0, 0), 100.0);
        assert_eq!(tile.sample(5, 0), 105.0);
        assert_eq!(tile.sample(0, 2), 2100.0);
        assert_eq!(tile.range(), (100.0, 100.0 + 127.0 + 127_000.0));
        assert_eq!(SPACING, 8.0);
    }

    #[test]
    fn wrong_size_is_rejected() {
        assert!(TerrainTile::parse(&tile_document(0, 0, vec![0; 100])).is_err());
    }
}
