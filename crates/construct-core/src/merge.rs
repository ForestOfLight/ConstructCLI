//! Merging several structures into one that reassembles them at their
//! recorded world origins.
//!
//! The output's `structure_world_origin` is the min corner of the union of
//! every piece's bounding box. That is what makes entity positions need no
//! rewriting: a placed position is `Pos - origin + load_position`, so shifting
//! origin and load position together cancels out (§9).
//!
//! Space inside the union that no piece covers is filled with **air**, so
//! placing the result clears the gaps rather than leaving the terrain there. A
//! structure void a piece records for itself is not a gap — it is a builder
//! saying "leave this alone" — and survives the fill.

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
    /// guard, not a game limit.
    ///
    /// The blit allocates 16 bytes per cell: two `i32` layers, plus a `u32`
    /// `owner` per layer tracking which piece last wrote it. `placement` adds
    /// one `BTreeMap` entry per non-void layer-0 block on top — the largest
    /// term for a dense merge, but not a fixed per-cell cost, so it is not
    /// counted here.
    pub max_volume: i64,
}

/// 64 million blocks — about 1 GB of layer and owner data at the cap, before
/// `placement`'s per-block `BTreeMap` entries (see `max_volume`'s doc).
pub const DEFAULT_MAX_VOLUME: i64 = 64_000_000;

const PERFORMANCE_WARN_VOLUME: i64 = 64 * 256 * 64;

const NO_OWNER: u32 = u32::MAX;

const AIR: &str = "minecraft:air";

const GAP: i32 = i32::MIN;

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            on_overlap: OnOverlap::default(),
            max_volume: DEFAULT_MAX_VOLUME,
        }
    }
}

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

fn checked_add(a: Coord, b: Coord) -> Option<Coord> {
    Some(Coord {
        x: a.x.checked_add(b.x)?,
        y: a.y.checked_add(b.y)?,
        z: a.z.checked_add(b.z)?,
    })
}

fn checked_sub(a: Coord, b: Coord) -> Option<Coord> {
    Some(Coord {
        x: a.x.checked_sub(b.x)?,
        y: a.y.checked_sub(b.y)?,
        z: a.z.checked_sub(b.z)?,
    })
}

fn out_index(piece: &Structure, i: usize, size: &Size, min: Coord) -> Option<usize> {
    let local = piece.size.coord_of(i)?;
    let world = checked_add(piece.origin, local)?;
    let target = checked_sub(world, min)?;
    size.index_of(target)
}

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
    let mut layers = [vec![GAP; cells], vec![VOID; cells]];
    let mut owner: [Vec<u32>; 2] = [vec![NO_OWNER; cells], vec![NO_OWNER; cells]];
    let mut placement: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); pieces.len()];
    let mut overlaps: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    let mut bad_indices: Vec<u64> = vec![0; pieces.len()];

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

                let Some(&remapped) = remaps[p].get(index as usize) else {
                    bad_indices[p] += 1;
                    continue;
                };

                if layer == 0 {
                    placement[p].insert(i, out_i);
                }
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

    let mut air = None;
    for cell in layers[0].iter_mut() {
        if *cell == GAP {
            *cell = *air.get_or_insert_with(|| air_index(&mut palette));
        }
    }

    let mut block_position_data = BTreeMap::new();
    for (p, (_, piece)) in pieces.iter().enumerate() {
        for (&local_index, data) in &piece.block_position_data {
            let Some(&out_i) = placement[p].get(&local_index) else {
                continue;
            };
            if owner[0][out_i] == p as u32 {
                block_position_data.insert(out_i, data.clone());
            }
        }
    }

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

pub(crate) fn unify_palettes(pieces: &[Structure]) -> (Vec<BlockState>, Vec<Vec<i32>>) {
    let mut palette: Vec<BlockState> = Vec::new();
    let mut remaps = Vec::with_capacity(pieces.len());

    for piece in pieces {
        let mut remap = Vec::with_capacity(piece.palette.len());
        for entry in &piece.palette {
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
