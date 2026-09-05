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
    // A single-character `contains('a') && contains('b')` is satisfied by the
    // fixed template words alone ("**a**nd", "**b**locks"), so it would still
    // pass with the piece names redacted from the message. Pin the actual
    // phrase the error builds around the two names instead.
    assert!(
        text.contains("between a and b"),
        "must name both pieces: {text}"
    );
}

#[test]
fn a_void_cell_is_not_an_overlap() {
    // Two pieces whose bounding boxes genuinely overlap in one world cell,
    // but only one of them has a block there. Reporting that as an overlap
    // would cry wolf on every merge.
    //
    // a spans world x=0..2 at y=0,z=0; b spans world x=1..3 at y=0,z=0. The
    // only shared cell is world (1,0,0): a's local index 1, b's local index
    // 0. a is void there; b has a block. No cell is doubly non-void, so
    // there must be no overlap.
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, VOID];
    let mut b = Build::solid([2, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![0, 0];

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
    // A single-character `contains('a')` is satisfied by the word "may" in
    // the template text alone, so this ties the fixed template text
    // immediately preceding a piece name to the specific name "a".
    assert!(
        report.warnings.iter().any(|w| w.contains("unset on: a")),
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

// --- extreme but structurally valid input ---

#[test]
fn a_piece_at_an_extreme_origin_does_not_panic() {
    // origin.x comes from a file this tool did not write and is not range
    // checked. i32::MAX plus even a two-block size overflows plain i32
    // addition in the blit's per-block coordinate math, well before the
    // allocation guard ever sees it: BoundingBox::of saturates, so the
    // union's reported volume looks tiny even though the per-block
    // arithmetic underneath would overflow.
    let a = Build::solid([2, 1, 1], [i32::MAX, 0, 0], "minecraft:stone");
    let result = merge::merge(&[named("a", &a)], &MergeOptions::default());
    assert!(result.is_ok(), "must not panic or refuse: {result:?}");
}

// --- malformed input: out-of-range palette index ---

#[test]
fn an_out_of_range_palette_index_is_treated_as_void_and_warned_about() {
    // The decoder deliberately accepts a block_indices value outside the
    // palette (the game places air for it), so merge must tolerate it rather
    // than index its per-piece remap table out of bounds.
    //
    // Neither piece sits at [0,0,0]: the unrelated "origin may be unset on: a
    // — these will be placed at world origin" warning that `a` would
    // otherwise trigger contains the letter 'b' (in "be"), which previously
    // let a bare `contains('b')` pass even with the palette-index warning
    // deleted. Moving both origins off [0,0,0] removes that confound
    // entirely, on top of asserting a distinctive substring below.
    let a = Build::solid([1, 1, 1], [5, 0, 0], "minecraft:stone");
    let mut b = Build::solid([1, 1, 1], [6, 0, 0], "minecraft:dirt");
    b.layer0 = vec![9999];
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();

    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 6, y: 0, z: 0 }),
        None,
        "an out-of-range index must contribute nothing, not panic or place a bogus block"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("b ") && w.contains("outside its palette")),
        "expected a warning naming piece b for an out-of-range palette index: {:?}",
        report.warnings
    );
}

// --- block_position_data and entities ---

use support::compound;

fn chest_at(b: &mut Build, index: &str, label: &str) {
    b.block_position_data.push((
        index.to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![
                ("id", nbtx::Value::String("Chest".into())),
                ("label", nbtx::Value::String(label.into())),
            ]),
        )]),
    ));
}

fn label_at(s: &Structure, world: Coord) -> Option<String> {
    let local = Coord {
        x: world.x - s.origin.x,
        y: world.y - s.origin.y,
        z: world.z - s.origin.z,
    };
    let i = s.size.index_of(local)?;
    let entry = s.block_position_data.get(&i)?;
    let nbtx::Value::Compound(m) = entry else {
        return None;
    };
    let nbtx::Value::Compound(bed) = m.get("block_entity_data")? else {
        return None;
    };
    match bed.get("label")? {
        nbtx::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

#[test]
fn block_position_data_keys_are_recomputed_for_the_merged_grid() {
    // The index is relative to a grid whose dimensions changed, so carrying
    // the key across unchanged would attach the data to the wrong block.
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:chest");
    chest_at(&mut a, "0", "from-a");
    let b = Build::solid([1, 1, 1], [0, 0, 4], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        label_at(&out, Coord { x: 0, y: 0, z: 0 }).as_deref(),
        Some("from-a")
    );
    assert_eq!(out.block_position_data.len(), 1);
}

#[test]
fn block_position_data_follows_the_block_that_won_the_overlap() {
    // §9: "a chest's contents survive from a block that lost the overlap and
    // end up attached to the wrong thing" is the failure this prevents. It is
    // invisible in a block-only comparison, so it is asserted explicitly.
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:chest");
    chest_at(&mut b, "0", "from-b");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    // b is last, so b wins the contested cell — and its chest data must be the
    // data that survives there.
    assert_eq!(
        label_at(&out, Coord { x: 1, y: 0, z: 0 }).as_deref(),
        Some("from-b")
    );
}

#[test]
fn block_position_data_follows_the_winner_under_on_overlap_first() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:chest");
    chest_at(&mut b, "0", "from-b");

    let options = MergeOptions {
        on_overlap: OnOverlap::First,
        ..MergeOptions::default()
    };
    let out = merge::merge(&[named("a", &a), named("b", &b)], &options)
        .unwrap()
        .structure;

    assert_eq!(
        label_at(&out, Coord { x: 1, y: 0, z: 0 }).as_deref(),
        Some("from-a")
    );
}

#[test]
fn tick_queue_data_is_carried_along_with_block_entity_data() {
    // The `<index>` compound may hold block_entity_data, tick_queue_data, or
    // both. Merge carries the whole compound rather than picking fields out.
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:water");
    a.block_position_data.push((
        "0".to_string(),
        compound(vec![(
            "tick_queue_data",
            nbtx::Value::List(vec![compound(vec![("tick_delay", nbtx::Value::Int(5))])]),
        )]),
    ));
    let b = Build::solid([1, 1, 1], [0, 0, 3], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    let entry = out.block_position_data.values().next().unwrap();
    let nbtx::Value::Compound(m) = entry else {
        panic!()
    };
    assert!(m.contains_key("tick_queue_data"));
}

#[test]
fn entities_are_carried_with_their_positions_untouched() {
    // Entity Pos is an absolute world position, and placement subtracts the
    // structure's origin. Because the merged origin is the union's min corner,
    // that subtraction already puts every entity in the right place — so
    // rewriting Pos here would move entities by the origin delta, twice.
    let mut a = Build::solid([1, 1, 1], [10, 0, 0], "minecraft:air");
    let pos = nbtx::Value::List(vec![
        nbtx::Value::Float(10.5),
        nbtx::Value::Float(64.0),
        nbtx::Value::Float(0.5),
    ]);
    a.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        ("Pos", pos.clone()),
    ])];
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.entities.len(), 1);
    let nbtx::Value::Compound(e) = &out.entities[0] else {
        panic!()
    };
    assert_eq!(e.get("Pos"), Some(&pos), "entity Pos must not be rewritten");
}

#[test]
fn entities_from_every_piece_are_collected_in_argument_order() {
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    a.entities = vec![compound(vec![(
        "identifier",
        nbtx::Value::String("a".into()),
    )])];
    let mut b = Build::solid([1, 1, 1], [5, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![(
        "identifier",
        nbtx::Value::String("b".into()),
    )])];

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    assert_eq!(out.entities.len(), 2);
}

#[test]
fn a_merged_structure_encodes_and_decodes_back_equal() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let b = Build::solid([1, 1, 1], [0, 0, 4], "minecraft:stone");

    let merged = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    let bytes = mcstructure::encode(&merged, "merged").unwrap();
    let again = mcstructure::decode(&bytes, "merged").unwrap();

    assert_eq!(again.size, merged.size);
    assert_eq!(again.origin, merged.origin);
    assert_eq!(again.layers, merged.layers);
    assert_eq!(again.palette, merged.palette);
    assert_eq!(again.block_position_data, merged.block_position_data);
    assert_eq!(again.entities, merged.entities);
}
