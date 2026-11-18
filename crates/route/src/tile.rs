//! The tile grid a route is cut into.

use crate::geom::Vec2;

/// Edge length of a tile in metres.
pub const TILE_SIZE: f64 = 1024.0;

/// Index of a tile. Tile `(0, 0)` has its south-west corner at the route
/// origin; `x` counts tiles to the east, `z` to the north.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    pub x: i32,
    pub z: i32,
}

impl TileCoord {
    /// Parse a tile file name such as `+000057-000098.bin`.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        // The second sign separates the two numbers.
        let split = stem
            .char_indices()
            .skip(1)
            .find(|(_, c)| matches!(c, '+' | '-'))?
            .0;
        let parse = |part: &str| -> Option<i32> {
            let digits = part.get(1..)?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let value: i32 = digits.parse().ok()?;
            match part.as_bytes()[0] {
                b'+' => Some(value),
                b'-' => Some(-value),
                _ => None,
            }
        };
        Some(TileCoord {
            x: parse(&stem[..split])?,
            z: parse(&stem[split..])?,
        })
    }

    /// The file name the game uses for this tile.
    pub fn file_name(&self) -> String {
        let part = |v: i32| format!("{}{:06}", if v < 0 { '-' } else { '+' }, v.unsigned_abs());
        format!("{}{}.bin", part(self.x), part(self.z))
    }

    /// South-west corner in route metres.
    pub fn origin(&self) -> Vec2 {
        [self.x as f64 * TILE_SIZE, self.z as f64 * TILE_SIZE]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_round_trip() {
        for (name, x, z) in [
            ("+000057-000098.bin", 57, -98),
            ("-000001+000000.bin", -1, 0),
            ("+000000+000000.bin", 0, 0),
        ] {
            let coord = TileCoord::from_file_name(name).unwrap();
            assert_eq!(coord, TileCoord { x, z });
            assert_eq!(coord.file_name(), name);
        }
    }

    #[test]
    fn other_names_are_rejected() {
        for name in [
            "Tracks.bin",
            "+12.bin",
            "000001+000002.bin",
            "+00000a+000001.bin",
            "",
        ] {
            assert_eq!(TileCoord::from_file_name(name), None, "{name}");
        }
    }
}
