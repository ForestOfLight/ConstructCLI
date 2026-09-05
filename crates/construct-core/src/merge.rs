//! Merging several structures into one that reassembles them at their
//! recorded world origins.
//!
//! The output's `structure_world_origin` is the min corner of the union of
//! every piece's bounding box, which is what makes entity positions need no
//! rewriting: an entity's placed position is `Pos - origin + load_position`,
//! so shifting the origin and the load position together cancels out. See
//! the plan's "Entity positions need no translation" note and §9.
//!
//! Space inside the union that no piece covers is filled with **air**, so
//! placing the result clears the gaps between the pieces instead of leaving
//! whatever terrain is already there. A structure void a piece records for
//! itself is not a gap: it is a builder saying "leave this alone", and it
//! survives the fill.

use crate::error::{CoreError, Result};
use crate::mcstructure::{BlockState, BoundingBox, Coord, Size, Structure, VOID};
use std::collections::BTreeMap;

/// How to resolve two pieces both contributing a block at one position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnOverlap {
    /// The piece appearing later in the argument list wins.
    #[default]
    Last,
    /// The piece appearing earlier in the argument list wins.
    First,
    /// Refuse the merge.
    Error,
}

#[derive(Debug, Clone)]
pub struct MergeOptions {
    pub on_overlap: OnOverlap,
    /// Refuse a union bounding box with more blocks than this. An allocation
    /// guard, not a game limit. Per cell the blit allocates: the two `i32`
    /// layers (8 bytes), plus `owner` tracking which piece last wrote each
    /// layer cell (a `u32` per layer, 8 bytes) — 16 bytes so far. On top of
    /// that, `placement` adds one `BTreeMap` entry per non-void layer-0
    /// block, which for a dense merge is the largest of the three terms and
    /// is not a fixed per-cell cost, so it is not counted in this cap.
    pub max_volume: i64,
}

/// 64 million blocks — about 1 GB of layer and owner data at the cap, before
/// `placement`'s per-block `BTreeMap` entries (see `max_volume`'s doc).
pub const DEFAULT_MAX_VOLUME: i64 = 64_000_000;

/// Beyond this the result still loads, but placing it is slow. The vanilla
/// structure-block save limit, from the reference documentation.
const PERFORMANCE_WARN_VOLUME: i64 = 64 * 256 * 64;

/// Sentinel `owner` value meaning "no piece has written this cell yet".
/// `owner` holds a piece's index into the `pieces` slice; piece counts are
/// tiny (a handful at most), so `u32::MAX` can never collide with a real
/// index.
const NO_OWNER: u32 = u32::MAX;

/// The palette entry that fills the space between the pieces.
const AIR: &str = "minecraft:air";

/// A cell of the output grid no piece has claimed.
///
/// Distinct from [`VOID`], which a piece can legitimately record for itself:
/// a structure void means "leave whatever is here alone", and merge keeps it.
/// `GAP` is the rest — inside the union's bounding box but outside every
/// piece's — and is replaced by air before the structure is built. The value
/// is unreachable as a real index, and `encode` refuses any layer value that
/// is neither [`VOID`] nor a palette index, so a leak cannot reach a file.
const GAP: i32 = i32::MIN;

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            on_overlap: OnOverlap::default(),
            max_volume: DEFAULT_MAX_VOLUME,
        }
    }
}

/// One pair of pieces that contended for at least one position.
#[derive(Debug, Clone)]
pub struct Overlap {
    pub count: u64,
    /// The names of the two contending pieces, loser first.
    pub pieces: Vec<String>,
}

#[derive(Debug)]
pub struct MergeReport {
    pub structure: Structure,
    pub overlaps: Vec<Overlap>,
    pub warnings: Vec<String>,
}

fn refused(reason: impl Into<String>) -> CoreError {
    CoreError::MergeRefused {
        reason: reason.into(),
    }
}

/// `a + b`, or `None` on overflow.
///
/// `piece.origin` is read from a file this tool did not write and carries no
/// range validation, so it can be any `i32` — including values a block's
/// local offset would overflow when added to. Plain `+` would panic in debug
/// builds and silently wrap in release ones (placing a block at the wrong
/// position with no error at all), so overflow here is treated exactly like
/// an out-of-range coordinate: the caller skips the block. This mirrors how
/// `geometry.rs` already treats untrusted `origin`/`size` values throughout.
fn checked_add(a: Coord, b: Coord) -> Option<Coord> {
    Some(Coord {
        x: a.x.checked_add(b.x)?,
        y: a.y.checked_add(b.y)?,
        z: a.z.checked_add(b.z)?,
    })
}

/// `a - b`, or `None` on overflow. See [`checked_add`].
fn checked_sub(a: Coord, b: Coord) -> Option<Coord> {
    Some(Coord {
        x: a.x.checked_sub(b.x)?,
        y: a.y.checked_sub(b.y)?,
        z: a.z.checked_sub(b.z)?,
    })
}

/// Where a piece's own flattened block index lands in the merged grid.
///
/// `None` when the block cannot be placed: a coordinate whose world position
/// is not representable (see [`checked_add`]), or one falling outside the
/// union — neither of which a file this tool wrote can produce, and both of
/// which the callers treat as "this piece contributes nothing here".
fn out_index(piece: &Structure, i: usize, size: &Size, min: Coord) -> Option<usize> {
    let local = piece.size.coord_of(i)?;
    let world = checked_add(piece.origin, local)?;
    let target = checked_sub(world, min)?;
    size.index_of(target)
}

/// The merged palette's index for air, appending an entry if no piece had one.
///
/// An existing `minecraft:air` entry is reused, so merging structures that
/// already contain air does not leave the result with two air entries. A new
/// entry copies the highest block `version` in the palette: versions drive the
/// game's block-upgrade path, and an entry stamped older than its neighbours
/// would invite an upgrade pass the rest of the structure does not get. An
/// empty palette leaves nothing to copy, and 0 — "unversioned", upgrade it —
/// is harmless for air, which has never changed shape.
fn air_index(palette: &mut Vec<BlockState>) -> i32 {
    if let Some(i) = palette.iter().position(|e| e.name == AIR) {
        return i as i32;
    }
    let version = palette.iter().map(|e| e.version).max().unwrap_or(0);
    palette.push(BlockState {
        name: AIR.to_string(),
        states: nbtx::Value::Compound(std::collections::HashMap::new()),
        version,
    });
    (palette.len() - 1) as i32
}

/// Merges `pieces` into one structure positioned at the min corner of the
/// union of their bounding boxes.
pub fn merge(pieces: &[(String, Structure)], options: &MergeOptions) -> Result<MergeReport> {
    if pieces.is_empty() {
        return Err(refused("no structures to merge"));
    }

    // §9: identical origins mean the pieces would stack in one spot. This
    // includes the all-zero case, which is what an unset origin looks like.
    if pieces.len() > 1 {
        let first = pieces[0].1.origin;
        if pieces.iter().all(|(_, s)| s.origin == first) {
            return Err(refused(format!(
                "every structure has the same origin [{}, {}, {}], so the pieces would stack \
                 in one spot rather than reassemble",
                first.x, first.y, first.z
            )));
        }
    }

    let mut warnings = Vec::new();

    // §9: the mixed case is *not* refused — [0,0,0] is indistinguishable from
    // a legitimate save at world origin.
    if pieces.len() > 1 {
        let zeroed: Vec<&str> = pieces
            .iter()
            .filter(|(_, s)| s.origin == (Coord { x: 0, y: 0, z: 0 }))
            .map(|(n, _)| n.as_str())
            .collect();
        if !zeroed.is_empty() {
            warnings.push(format!(
                "origin may be unset on: {} — these will be placed at world origin",
                zeroed.join(", ")
            ));
        }
    }

    let boxes: Vec<BoundingBox> = pieces
        .iter()
        .map(|(_, s)| BoundingBox::of(s.origin, s.size))
        .collect();
    let union = BoundingBox::union(&boxes).ok_or_else(|| refused("no structures to merge"))?;
    let size = union.size();
    let volume = size.volume();

    if volume > options.max_volume {
        return Err(refused(format!(
            "the merged bounding box is {} x {} x {} = {volume} blocks, too large to build in \
             memory (limit {})",
            size.x, size.y, size.z, options.max_volume
        )));
    }
    if volume > PERFORMANCE_WARN_VOLUME {
        warnings.push(format!(
            "the merged structure is large: {} x {} x {} = {volume} blocks. It will load, but \
             placing it may be slow",
            size.x, size.y, size.z
        ));
    }

    let structures: Vec<Structure> = pieces.iter().map(|(_, s)| s.clone()).collect();
    let (mut palette, remaps) = unify_palettes(&structures);

    let cells =
        usize::try_from(volume).map_err(|_| refused("merged size is too large to address"))?;
    // Layer 0 starts as gap — nothing has claimed any of it yet. Layer 1
    // starts as void and stays that way wherever no piece writes: the second
    // layer is void for most blocks in the files the game itself writes (see
    // the reference documentation), and filling it with air would be a change
    // to the extra/liquid layer nobody asked for.
    let mut layers = [vec![GAP; cells], vec![VOID; cells]];
    // Which piece last wrote each cell of each layer, so an overlap can name
    // both contenders and `block_position_data` can follow the winner. A
    // `u32` with a sentinel rather than `Option<usize>`: the latter is 16
    // bytes (measured), which would make this array cost 4x what the two
    // index layers cost combined for no benefit — piece counts never
    // approach `u32::MAX`.
    let mut owner: [Vec<u32>; 2] = [vec![NO_OWNER; cells], vec![NO_OWNER; cells]];
    // Where each piece's block indices landed in the merged grid, so
    // `block_position_data` can be moved to the same cell without recomputing
    // the coordinate arithmetic a second time.
    let mut placement: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); pieces.len()];
    let mut overlaps: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    // Per-piece count of block_indices values that are neither VOID nor a
    // valid index into that piece's own palette. The decoder deliberately
    // accepts such values — the format documentation says the game places
    // air for them, so refusing the file outright would reject structures
    // the game itself loads. But this piece's `remaps[p]` has exactly one
    // entry per *palette* entry, so an out-of-range block_indices value is
    // not a valid index into it, and must never be used to index it.
    let mut bad_indices: Vec<u64> = vec![0; pieces.len()];

    // Pass one: mark the structure voids the pieces recorded for themselves,
    // so the air fill at the end can tell them from the space between the
    // pieces. This must finish before any block is written, or a later
    // piece's void would erase an earlier piece's block — a piece
    // contributes only where it is not void (§9), in both directions.
    for (_, piece) in pieces.iter() {
        for (i, &index) in piece.layers[0].iter().enumerate() {
            if index != VOID {
                continue;
            }
            if let Some(out_i) = out_index(piece, i, &size, union.min) {
                layers[0][out_i] = VOID;
            }
        }
    }

    for (p, (_, piece)) in pieces.iter().enumerate() {
        for layer in 0..2 {
            for (i, &index) in piece.layers[layer].iter().enumerate() {
                if index == VOID {
                    continue;
                }
                let Some(out_i) = out_index(piece, i, &size, union.min) else {
                    continue;
                };

                // Required correction: `index` comes from a file this tool
                // did not write, and the decoder accepts a value outside the
                // palette. Indexing `remaps[p]` with it unchecked would
                // panic. The piece contributes nothing here instead, leaving
                // the cell to the air fill below — which is what the game
                // does with an out-of-range index too, so the merged result
                // places what loading the original would have placed.
                let Some(&remapped) = remaps[p].get(index as usize) else {
                    bad_indices[p] += 1;
                    continue;
                };

                // Layer 0 only — `block_position_data` is layer-agnostic,
                // since two block entities cannot share a block space. This
                // is recorded only once the block index is known good: a
                // position skipped by the guard above contributed no block,
                // so it must not carry a `block_position_data` entry either.
                if layer == 0 {
                    placement[p].insert(i, out_i);
                }
                // `p` is an index into `pieces`, which is tiny, so this cast
                // never truncates (see `NO_OWNER`'s doc).
                let p_owner = p as u32;
                match owner[layer][out_i] {
                    NO_OWNER => {
                        layers[layer][out_i] = remapped;
                        owner[layer][out_i] = p_owner;
                    }
                    previous => {
                        *overlaps.entry((previous as usize, p)).or_default() += 1;
                        if options.on_overlap == OnOverlap::Last {
                            layers[layer][out_i] = remapped;
                            owner[layer][out_i] = p_owner;
                        }
                    }
                }
            }
        }
    }

    for (p, &count) in bad_indices.iter().enumerate() {
        if count > 0 {
            warnings.push(format!(
                "{} has {count} block index value(s) outside its palette; treated as air, \
                 which is what the game places for them",
                pieces[p].0
            ));
        }
    }

    let overlaps: Vec<Overlap> = overlaps
        .into_iter()
        .map(|((a, b), count)| Overlap {
            count,
            pieces: vec![pieces[a].0.clone(), pieces[b].0.clone()],
        })
        .collect();

    if options.on_overlap == OnOverlap::Error && !overlaps.is_empty() {
        let detail: Vec<String> = overlaps
            .iter()
            .map(|o| {
                format!(
                    "{} blocks between {} and {}",
                    o.count, o.pieces[0], o.pieces[1]
                )
            })
            .collect();
        return Err(refused(format!(
            "structures overlap and --on-overlap=error was given: {}",
            detail.join("; ")
        )));
    }

    // Everything still marked `GAP` is space between the pieces: inside the
    // union, outside every piece's own bounding box. Fill it with air so
    // placing the merged structure clears those gaps, rather than leaving
    // them to whatever terrain the structure lands on. The air entry is
    // appended only if a gap actually exists, so a merge that tiles its
    // union exactly gets the palette its pieces had. Appending cannot
    // disturb `remaps`, whose indices were assigned before it.
    let mut air = None;
    for cell in layers[0].iter_mut() {
        if *cell == GAP {
            *cell = *air.get_or_insert_with(|| air_index(&mut palette));
        }
    }

    // `block_position_data` follows the block that won its cell. Walking the
    // pieces in order and letting a later winner overwrite an earlier entry
    // reproduces the same resolution the blit used, so a chest's contents can
    // never end up attached to a block that lost (§9).
    let mut block_position_data = BTreeMap::new();
    for (p, (_, piece)) in pieces.iter().enumerate() {
        for (&local_index, data) in &piece.block_position_data {
            let Some(&out_i) = placement[p].get(&local_index) else {
                continue;
            };
            // Insert only for the piece that owns the cell. There is exactly
            // one owner per cell, so no stale entry can survive and nothing
            // needs removing. An `else { remove }` arm here would be actively
            // wrong under OnOverlap::First, where the winner writes first and
            // the later loser would delete the winner's data.
            if owner[0][out_i] == p as u32 {
                block_position_data.insert(out_i, data.clone());
            }
        }
    }

    // Entities are carried verbatim. `Pos` is an absolute world position and
    // placement computes `Pos - origin + load_position`; since the merged
    // origin is the union's min corner, that arithmetic already lands every
    // entity correctly. Rewriting `Pos` here would shift them twice.
    let entities: Vec<nbtx::Value> = pieces
        .iter()
        .flat_map(|(_, s)| s.entities.iter().cloned())
        .collect();

    Ok(MergeReport {
        structure: Structure {
            format_version: pieces[0].1.format_version,
            size,
            origin: union.min,
            layers,
            palette,
            block_position_data,
            entities,
        },
        overlaps,
        warnings,
    })
}

/// Builds one palette covering every piece, plus a per-piece remap from that
/// piece's own indices to the merged palette's.
///
/// Entries are deduplicated on the whole `(name, states, version)` triple.
/// Deduplicating on `name` alone would collapse an open door onto a shut one
/// and a stair facing north onto one facing south.
pub(crate) fn unify_palettes(pieces: &[Structure]) -> (Vec<BlockState>, Vec<Vec<i32>>) {
    let mut palette: Vec<BlockState> = Vec::new();
    let mut remaps = Vec::with_capacity(pieces.len());

    for piece in pieces {
        let mut remap = Vec::with_capacity(piece.palette.len());
        for entry in &piece.palette {
            // A linear scan, not a HashMap: `BlockState` cannot be `Eq`
            // (`nbtx::Value` holds floats), and a hand-written `impl Eq` would
            // be a reflexivity claim a HashMap silently relies on. Palettes are
            // small — the largest across 13 real files is 31 entries — so the
            // quadratic term is not a real cost.
            let index = match palette.iter().position(|e| e == entry) {
                Some(i) => i as i32,
                None => {
                    palette.push(entry.clone());
                    (palette.len() - 1) as i32
                }
            };
            remap.push(index);
        }
        remaps.push(remap);
    }

    (palette, remaps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcstructure::{BlockState, Coord, Size, Structure};
    use std::collections::BTreeMap;

    fn state(name: &str) -> BlockState {
        BlockState {
            name: name.to_string(),
            states: nbtx::Value::Compound(std::collections::HashMap::new()),
            version: 1,
        }
    }

    fn piece(palette: Vec<BlockState>) -> Structure {
        Structure {
            format_version: 1,
            size: Size { x: 1, y: 1, z: 1 },
            origin: Coord { x: 0, y: 0, z: 0 },
            layers: [vec![0], vec![-1]],
            palette,
            block_position_data: BTreeMap::new(),
            entities: vec![],
        }
    }

    #[test]
    fn identical_blocks_collapse_to_one_entry() {
        let a = piece(vec![state("minecraft:stone")]);
        let b = piece(vec![state("minecraft:stone")]);
        let (palette, remaps) = unify_palettes(&[a, b]);
        assert_eq!(palette.len(), 1);
        assert_eq!(remaps, vec![vec![0], vec![0]]);
    }

    #[test]
    fn different_blocks_each_get_an_entry_in_first_seen_order() {
        let a = piece(vec![state("minecraft:stone"), state("minecraft:dirt")]);
        let b = piece(vec![state("minecraft:dirt"), state("minecraft:oak_log")]);
        let (palette, remaps) = unify_palettes(&[a, b]);
        assert_eq!(
            palette.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["minecraft:stone", "minecraft:dirt", "minecraft:oak_log"]
        );
        assert_eq!(remaps, vec![vec![0, 1], vec![1, 2]]);
    }

    #[test]
    fn blocks_differing_only_in_states_stay_distinct() {
        // A stair facing north and one facing south are the same `name` and the
        // same `version`. Collapsing them would silently rotate half a build.
        let mut open = state("minecraft:oak_door");
        open.states = nbtx::Value::Compound(std::collections::HashMap::from([(
            "open_bit".to_string(),
            nbtx::Value::Byte(1),
        )]));
        let shut = state("minecraft:oak_door");
        let (palette, remaps) = unify_palettes(&[piece(vec![open, shut])]);
        assert_eq!(palette.len(), 2);
        assert_eq!(remaps, vec![vec![0, 1]]);
    }

    #[test]
    fn blocks_differing_only_in_version_stay_distinct() {
        let mut older = state("minecraft:stone");
        older.version = 17879555;
        let newer = state("minecraft:stone");
        let (palette, _) = unify_palettes(&[piece(vec![older, newer])]);
        assert_eq!(palette.len(), 2);
    }
}
