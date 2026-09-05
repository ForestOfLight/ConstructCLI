mod support;

use construct_core::mcstructure::{self, Coord, Size, Structure, VOID};
use construct_core::merge::{self, MergeOptions, OnOverlap};
use support::{Build, block};

fn decode(b: &Build) -> Structure {
    mcstructure::decode(&b.bytes(), "test").unwrap()
}

fn named(name: &str, b: &Build) -> (String, Structure) {
    (name.to_string(), decode(b))
}

/// The block at a world coordinate on a layer, as a palette *name*, or None
/// for void. Stating assertions in world space keeps them readable and
/// independent of the merged grid's own indexing.
fn block_at(s: &Structure, layer: usize, world: Coord) -> Option<&str> {
    let local = Coord {
        x: world.x - s.origin.x,
        y: world.y - s.origin.y,
        z: world.z - s.origin.z,
    };
    let i = s.size.index_of(local)?;
    let index = s.layers[layer][i];
    if index == VOID {
        return None;
    }
    Some(s.palette[index as usize].name.as_str())
}

#[test]
fn merging_one_structure_returns_it_unchanged() {
    // Spec §12's merge-placement property, base case.
    let b = Build::solid([2, 3, 4], [10, 64, -8], "minecraft:stone");
    let one = decode(&b);
    let out = merge::merge(&[named("only", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.size, one.size);
    assert_eq!(out.origin, one.origin);
    assert_eq!(out.layers, one.layers);
    assert_eq!(
        out.palette
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        one.palette
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn two_disjoint_pieces_land_at_their_world_positions() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.origin, Coord { x: 0, y: 0, z: 0 });
    assert_eq!(out.size, Size { x: 4, y: 1, z: 1 });
    assert_eq!(
        block_at(&out, 0, Coord { x: 0, y: 0, z: 0 }),
        Some("minecraft:stone")
    );
    assert_eq!(
        block_at(&out, 0, Coord { x: 3, y: 0, z: 0 }),
        Some("minecraft:dirt")
    );
}

#[test]
fn the_gap_between_pieces_is_void_not_air() {
    // Void leaves existing terrain alone; air would carve holes in whatever
    // the merged structure is placed over (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }), None);
    assert_eq!(block_at(&out, 0, Coord { x: 2, y: 0, z: 0 }), None);
    let void_count = out.layers[0].iter().filter(|&&i| i == VOID).count();
    assert_eq!(void_count, 2);
}

#[test]
fn the_merged_origin_is_the_minimum_corner_including_negatives() {
    let a = Build::solid([1, 1, 1], [5, 5, 5], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [-2, 0, 3], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    assert_eq!(out.origin, Coord { x: -2, y: 0, z: 3 });
    assert_eq!(out.size, Size { x: 8, y: 6, z: 3 });
}

#[test]
fn a_piece_contributes_only_where_it_is_not_void() {
    // Piece b is void at world x=0, so a's block there must survive even
    // though b comes later in argument order. b cannot share a's origin
    // [0,0,0] here — identical origins are unconditionally refused (§9), as
    // covered separately below — so b is widened to world x=[-1,2) instead
    // of placed flush with a; its own local x=1 (not x=0) is the one that
    // lands on world x=0 and is left void.
    let a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut b = Build::solid([3, 1, 1], [-1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![VOID, VOID, 0];
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 0, y: 0, z: 0 }),
        Some("minecraft:stone")
    );
    assert_eq!(
        block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:dirt")
    );
}

#[test]
fn the_second_layer_merges_independently_of_the_first() {
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    a.palette.push(block("minecraft:water"));
    a.layer1 = vec![1];
    let b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:stone");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 1, Coord { x: 0, y: 0, z: 0 }),
        Some("minecraft:water")
    );
    assert_eq!(block_at(&out, 1, Coord { x: 1, y: 0, z: 0 }), None);
}

// --- overlap ---

#[test]
fn the_last_piece_wins_an_overlap_by_default() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:dirt");
    // Distinct origins are required, so shift b's *size* to overlap instead.
    let mut a = a;
    a.size = [2, 1, 1];
    a.layer0 = vec![0, 0];
    a.layer1 = vec![VOID, VOID];
    let mut b = b;
    b.origin = [1, 0, 0];

    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:dirt")
    );
    assert_eq!(report.overlaps.len(), 1);
    assert_eq!(report.overlaps[0].count, 1);
    assert_eq!(
        report.overlaps[0].pieces,
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn the_first_piece_wins_under_on_overlap_first() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![0];

    let options = MergeOptions {
        on_overlap: OnOverlap::First,
        ..MergeOptions::default()
    };
    let report = merge::merge(&[named("a", &a), named("b", &b)], &options).unwrap();
    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:stone")
    );
    assert_eq!(report.overlaps[0].count, 1);
}

#[test]
fn on_overlap_error_refuses_and_names_the_pieces() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");

    let options = MergeOptions {
        on_overlap: OnOverlap::Error,
        ..MergeOptions::default()
    };
    let err = merge::merge(&[named("a", &a), named("b", &b)], &options).unwrap_err();
    let text = format!("{err}");
    assert!(
        text.contains('a') && text.contains('b'),
        "must name both: {text}"
    );
}

#[test]
fn a_void_cell_is_not_an_overlap() {
    // Two pieces occupying the same space, but only one has a block there.
    // Reporting that as an overlap would cry wolf on every merge.
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, VOID];
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:dirt");
    b.layer0 = vec![VOID, 0];
    b.origin = [0, 1, 0];

    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(report.overlaps.is_empty(), "{:?}", report.overlaps);
}

// --- refusals ---

#[test]
fn identical_origins_are_refused() {
    // The pieces would stack in one spot (§9).
    let a = Build::solid([1, 1, 1], [4, 4, 4], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [4, 4, 4], "minecraft:dirt");
    let err =
        merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("origin"), "{err}");
}

#[test]
fn all_zero_origins_are_refused_too() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:dirt");
    assert!(merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).is_err());
}

#[test]
fn a_mix_of_zero_and_real_origins_proceeds_with_a_warning() {
    // [0,0,0] is indistinguishable from a legitimate save at world origin, so
    // refusing would be wrong about as often as it was right (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [40, 0, 0], "minecraft:dirt");
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains('a')),
        "expected a warning naming the piece at the origin: {:?}",
        report.warnings
    );
}

#[test]
fn a_union_too_large_to_allocate_is_refused() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    // Two axes, not one: [100_000, 0, 0] alone is a union of only 100_001
    // blocks, far below DEFAULT_MAX_VOLUME, and would not refuse at all.
    let b = Build::solid([1, 1, 1], [100_000, 0, 100_000], "minecraft:dirt");
    let err =
        merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("large"), "{err}");
}

#[test]
fn an_oversized_but_allocatable_union_warns_rather_than_refusing() {
    // Minecraft loads structures past 64*256*64 without trouble, so this is a
    // performance warning, not a limit (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    // 2001 x 1 x 2001 = 4,004,001 blocks: above PERFORMANCE_WARN_VOLUME
    // (64*256*64 = 1,048,576) and below DEFAULT_MAX_VOLUME (64,000,000), which
    // is precisely the band this test is about.
    let b = Build::solid([1, 1, 1], [2000, 0, 2000], "minecraft:dirt");
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains("large")),
        "expected a size warning: {:?}",
        report.warnings
    );
}

#[test]
fn merging_nothing_is_refused() {
    let err = merge::merge(&[], &MergeOptions::default()).unwrap_err();
    assert!(
        format!("{err}").contains("nothing") || format!("{err}").contains("no structures"),
        "{err}"
    );
}

// --- malformed input: out-of-range palette index ---

#[test]
fn an_out_of_range_palette_index_is_treated_as_void_and_warned_about() {
    // The decoder deliberately accepts a block_indices value outside the
    // palette (the game places air for it), so merge must tolerate it rather
    // than index its per-piece remap table out of bounds.
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![9999];
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();

    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 1, y: 0, z: 0 }),
        None,
        "an out-of-range index must contribute nothing, not panic or place a bogus block"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains('b')),
        "expected a warning naming the piece with the bad index: {:?}",
        report.warnings
    );
}
