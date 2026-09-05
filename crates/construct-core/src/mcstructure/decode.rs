//! Decoding `.mcstructure` bytes into [`Structure`].
//!
//! Most of the validation here mirrors the load-time rules the game itself
//! enforces, documented in `docs/bedrock-mcstructure-files.md`: exactly two
//! index layers, both the same length, that length equal to the product of
//! `size`, and a `default` palette present. Two checks go beyond that
//! documentation and are this tool's own added strictness rather than a
//! documented game rule: a negative `size` dimension is rejected outright,
//! and a `block_position_data` key at or past the volume is rejected too.
//! Refusing here turns a structure that would fail to load — or load wrong,
//! silently — into an error naming the field.

use super::geometry::{Coord, Size};
use super::nbt::{as_compound, as_int, as_int_vec, as_list, as_triple, bad, field};
use crate::error::Result;
use std::collections::BTreeMap;

/// The index meaning "no block here": existing terrain is left untouched.
pub const VOID: i32 = -1;

/// One entry of `block_palette`.
///
/// Deliberately not `Eq`: `nbtx::Value` implements `PartialEq` and `Hash` but
/// has no `Eq` impl and cannot have one, because it holds `Float(f32)` and
/// `Double(f64)`. Palette deduplication therefore uses a linear scan rather
/// than a `HashMap` — see `merge::unify_palettes`.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct BlockState {
    pub name: String,
    /// Kept as raw NBT: state values are strings, ints, or bytes depending on
    /// the property, and this tool never needs to interpret them — only to
    /// compare them for palette deduplication.
    pub states: nbtx::Value,
    pub version: i32,
}

/// A decoded `.mcstructure`.
#[derive(Debug, Clone)]
pub struct Structure {
    pub format_version: i32,
    pub size: Size,
    /// Where in the world this was saved. Merge reads this as the piece's
    /// position; entity positions are stored relative to it.
    pub origin: Coord,
    /// The two index layers, each `size.volume()` long. Layer 1 is usually all
    /// [`VOID`] except where a block is waterlogged.
    pub layers: [Vec<i32>; 2],
    pub palette: Vec<BlockState>,
    /// Extra per-block data, keyed by flattened block index. The value is the
    /// whole `<index>` compound — which may hold `block_entity_data`,
    /// `tick_queue_data`, or both — carried verbatim.
    pub block_position_data: BTreeMap<usize, nbtx::Value>,
    /// Entities as raw NBT, exactly as stored. `Pos` is an absolute world
    /// position at save time; see the merge module for why that means merge
    /// never rewrites it.
    pub entities: Vec<nbtx::Value>,
}

pub fn decode(bytes: &[u8], what: &str) -> Result<Structure> {
    let mut cursor = bytes;
    let root: nbtx::Value = nbtx::from_le_bytes(&mut cursor)
        .map_err(|e| bad(what, format!("not readable as little-endian NBT: {e}")))?;

    let format_version = as_int(
        field(&root, "format_version", what)?,
        "format_version",
        what,
    )?;
    let [sx, sy, sz] = as_triple(field(&root, "size", what)?, "size", what)?;
    if sx < 0 || sy < 0 || sz < 0 {
        return Err(bad(
            what,
            format!("size has a negative dimension: [{sx}, {sy}, {sz}]"),
        ));
    }
    let size = Size {
        x: sx,
        y: sy,
        z: sz,
    };
    let [ox, oy, oz] = as_triple(
        field(&root, "structure_world_origin", what)?,
        "structure_world_origin",
        what,
    )?;

    let structure = field(&root, "structure", what)?;

    let raw_layers = as_list(
        field(structure, "block_indices", what)?,
        "block_indices",
        what,
    )?;
    if raw_layers.len() != 2 {
        return Err(bad(
            what,
            format!(
                "block_indices needs exactly 2 layers, found {}",
                raw_layers.len()
            ),
        ));
    }
    let layer0 = as_int_vec(&raw_layers[0], "block_indices[0]", what)?;
    let layer1 = as_int_vec(&raw_layers[1], "block_indices[1]", what)?;
    if layer0.len() != layer1.len() {
        return Err(bad(
            what,
            format!(
                "the two block_indices layers must be the same length: {} and {}",
                layer0.len(),
                layer1.len()
            ),
        ));
    }
    let volume = usize::try_from(size.volume()).map_err(|_| {
        bad(
            what,
            format!("size [{sx}, {sy}, {sz}] is too large to address"),
        )
    })?;
    if layer0.len() != volume {
        return Err(bad(
            what,
            format!(
                "block_indices has {} entries but size [{sx}, {sy}, {sz}] needs {volume}",
                layer0.len()
            ),
        ));
    }

    let palette_group = field(structure, "palette", what)?;
    let default = field(palette_group, "default", what).map_err(|_| {
        bad(
            what,
            "no `default` palette; the game places no blocks at all for such a file",
        )
    })?;

    let mut palette = Vec::new();
    for entry in as_list(
        field(default, "block_palette", what)?,
        "block_palette",
        what,
    )? {
        palette.push(BlockState {
            name: match field(entry, "name", what)? {
                nbtx::Value::String(s) => s.clone(),
                _ => return Err(bad(what, "a block_palette entry's `name` is not a string")),
            },
            states: field(entry, "states", what)?.clone(),
            version: as_int(field(entry, "version", what)?, "version", what)?,
        });
    }

    let mut block_position_data = BTreeMap::new();
    for (key, value) in as_compound(
        field(default, "block_position_data", what)?,
        "block_position_data",
        what,
    )? {
        let index: usize = key.parse().map_err(|_| {
            bad(
                what,
                format!("block_position_data key {key:?} is not a block index"),
            )
        })?;
        if index >= volume {
            return Err(bad(
                what,
                format!("block_position_data key {key:?} is outside the structure"),
            ));
        }
        block_position_data.insert(index, value.clone());
    }

    let entities = as_list(field(structure, "entities", what)?, "entities", what)?.clone();

    Ok(Structure {
        format_version,
        size,
        origin: Coord {
            x: ox,
            y: oy,
            z: oz,
        },
        layers: [layer0, layer1],
        palette,
        block_position_data,
        entities,
    })
}
