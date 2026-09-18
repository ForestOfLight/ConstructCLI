//! The `.mcstructure` format: model, codec, and geometry.
//!
//! Field layout, ZYX index order, and load-time validation rules come from
//! `docs/bedrock-mcstructure-files.md`, cross-checked against 13 real files.
//! The second layout the game writes, format 2, is described in
//! `docs/mcstructure-format-2.md`; both decode into the same [`Structure`], so
//! nothing downstream — merge above all — has to know which one a file used.

pub mod decode;
pub mod encode;
pub mod geometry;
pub(crate) mod nbt;

pub use decode::{BlockState, Structure, VOID, decode};
pub use encode::{OUTPUT_FORMAT_VERSION, encode};
pub use geometry::{BoundingBox, Coord, Size};
