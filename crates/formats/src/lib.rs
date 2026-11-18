//! Readers for the on-disk formats of a RailWorks / Train Simulator Classic
//! installation.
//!
//! Nothing in this crate contains game code or game content. Every format here
//! was worked out by looking at files in a legally owned installation and at
//! the XML sources some add-ons ship next to their binaries. See
//! `docs/FORMATS.md` for the notes.
//!
//! * [`serz`]: the `SERZ` binary container used by `.bin`, `.proxybin`,
//!   `.GeoPcDx`, `.TgPcDx` and most route and scenario files.
//! * [`xml`]: the XML twin of that container (`RouteProperties.xml` and
//!   friends), read into and written from the same [`Document`] tree.
//! * [`ap`]: `.ap` asset archives.
//! * [`pak`]: `Blueprints.pak` blueprint caches.
//! * [`install`]: finding routes and assets inside an installation.

pub mod ap;
pub mod error;
pub mod install;
pub mod node;
pub mod pak;
pub mod serz;
pub mod xml;

pub use error::{Error, Result};
pub use install::{parse_document, Install, RouteInfo};
pub use node::{Document, Element, Node, ScalarType, Value};
