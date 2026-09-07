//! The `.mcstructure` format: model, codec, and geometry.
//!
//! Field layout, ZYX index order, and load-time validation rules come from
//! `docs/bedrock-mcstructure-files.md`, cross-checked against 13 real files.

pub mod decode;
pub mod encode;
pub mod geometry;
pub(crate) mod nbt;

pub use decode::{BlockState, Structure, VOID, decode};
pub use encode::encode;
pub use geometry::{BoundingBox, Coord, Size};
