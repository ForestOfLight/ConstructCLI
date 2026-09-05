//! The `.mcstructure` format: model, codec, and geometry.
//!
//! The field layout, the ZYX index order, and the game's load-time validation
//! rules come from `docs/bedrock-mcstructure-files.md` (third-party format
//! documentation by tryashtar, github.com/tryashtar), cross-checked against 13
//! real files exported from the developer's own worlds.

pub mod decode;
pub mod encode;
pub mod geometry;
pub(crate) mod nbt;

pub use decode::{BlockState, Structure, VOID, decode};
pub use encode::encode;
pub use geometry::{BoundingBox, Coord, Size};
