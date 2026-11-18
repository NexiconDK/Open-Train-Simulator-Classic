//! Route data: where the track runs and what the ground looks like.
//!
//! This crate turns the generic trees read by `opentsc-formats` into typed
//! route data. It has no rendering code; [`map`] writes a plain SVG so the
//! result can be checked by eye.

pub mod files;
pub mod geom;
pub mod graph;
pub mod map;
pub mod route;
pub mod terrain;
pub mod tile;
pub mod track;

pub use route::Route;
pub use tile::{TileCoord, TILE_SIZE};

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// A file could not be read or decoded.
    Format(opentsc_formats::Error),
    /// A file decoded fine but does not hold what a route file should.
    Data(String),
    /// The route has no such file.
    Missing(String),
}

impl Error {
    pub(crate) fn data(message: impl Into<String>) -> Self {
        Error::Data(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(e) => e.fmt(f),
            Error::Data(message) => write!(f, "unexpected route data: {message}"),
            Error::Missing(path) => write!(f, "the route has no file '{path}'"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Format(e) => Some(e),
            _ => None,
        }
    }
}

impl From<opentsc_formats::Error> for Error {
    fn from(e: opentsc_formats::Error) -> Self {
        Error::Format(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Format(e.into())
    }
}
