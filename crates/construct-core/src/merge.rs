//! Merging several structures into one that reassembles them at their
//! recorded world origins.
//!
//! The output's `structure_world_origin` is the min corner of the union of
//! every piece's bounding box, which is what makes entity positions need no
//! rewriting: an entity's placed position is `Pos - origin + load_position`,
//! so shifting the origin and the load position together cancels out. See
//! the plan's "Entity positions need no translation" note and §9.

use crate::mcstructure::{BlockState, Structure};

/// Builds one palette covering every piece, plus a per-piece remap from that
/// piece's own indices to the merged palette's.
///
/// Entries are deduplicated on the whole `(name, states, version)` triple.
/// Deduplicating on `name` alone would collapse an open door onto a shut one
/// and a stair facing north onto one facing south.
#[allow(dead_code)]
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
