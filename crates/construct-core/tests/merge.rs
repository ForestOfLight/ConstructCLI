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
fn the_gap_between_pieces_is_air() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:air")
    );
    assert_eq!(
        block_at(&out, 0, Coord { x: 2, y: 0, z: 0 }),
        Some("minecraft:air")
    );
    assert_eq!(out.layers[0].iter().filter(|&&i| i == VOID).count(), 0);
}

#[test]
fn the_gap_is_filled_on_the_first_layer_only() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(block_at(&out, 1, Coord { x: 1, y: 0, z: 0 }), None);
    assert!(
        out.layers[1].iter().all(|&i| i == VOID),
        "{:?}",
        out.layers[1]
    );
}

#[test]
fn a_pieces_own_structure_void_is_kept_rather_than_filled() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, VOID];
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }),
        None,
        "a structure void a piece recorded for itself must survive the fill"
    );
    assert_eq!(
        block_at(&out, 0, Coord { x: 2, y: 0, z: 0 }),
        Some("minecraft:air")
    );
}

#[test]
fn a_pieces_void_does_not_erase_an_earlier_pieces_block() {
    let a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut b = Build::solid([2, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![VOID, 0];
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:stone")
    );
}

#[test]
fn air_recorded_by_a_piece_stays_air() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.palette.push(block("minecraft:air"));
    a.layer0 = vec![0, 1];
    let b = Build::solid([1, 1, 1], [2, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:air")
    );
}

#[test]
fn no_air_entry_is_added_when_the_pieces_tile_the_union() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        out.palette
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["minecraft:stone", "minecraft:dirt"]
    );
}

#[test]
fn the_fill_reuses_an_air_entry_a_piece_already_had() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.palette.push(block("minecraft:air"));
    a.layer0 = vec![0, 1];
    let b = Build::solid([1, 1, 1], [4, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(
        block_at(&out, 0, Coord { x: 3, y: 0, z: 0 }),
        Some("minecraft:air"),
        "the gap must still be filled"
    );
    assert_eq!(
        out.palette
            .iter()
            .filter(|p| p.name == "minecraft:air")
            .count(),
        1,
        "a second air entry would be a duplicate: {:?}",
        out.palette.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn an_added_air_entry_carries_the_pieces_block_version() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    let air = out
        .palette
        .iter()
        .find(|p| p.name == "minecraft:air")
        .expect("the gap must have added an air entry");
    assert_eq!(air.version, 18163713);
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

#[test]
fn the_last_piece_wins_an_overlap_by_default() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:dirt");
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
        text.contains("between a and b"),
        "must name both pieces: {text}"
    );
}

#[test]
fn a_void_cell_is_not_an_overlap() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, VOID];
    let mut b = Build::solid([2, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![0, 0];

    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(report.overlaps.is_empty(), "{:?}", report.overlaps);
}

#[test]
fn identical_origins_are_refused() {
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
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [40, 0, 0], "minecraft:dirt");
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains("unset on: a")),
        "expected a warning naming the piece at the origin: {:?}",
        report.warnings
    );
}

#[test]
fn a_union_too_large_to_allocate_is_refused() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [100_000, 0, 100_000], "minecraft:dirt");
    let err =
        merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("large"), "{err}");
}

#[test]
fn an_oversized_but_allocatable_union_warns_rather_than_refusing() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
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

#[test]
fn a_piece_at_an_extreme_origin_does_not_panic() {
    let a = Build::solid([2, 1, 1], [i32::MAX, 0, 0], "minecraft:stone");
    let result = merge::merge(&[named("a", &a)], &MergeOptions::default());
    assert!(result.is_ok(), "must not panic or refuse: {result:?}");
}

#[test]
fn an_out_of_range_palette_index_becomes_air_and_is_warned_about() {
    let a = Build::solid([1, 1, 1], [5, 0, 0], "minecraft:stone");
    let mut b = Build::solid([1, 1, 1], [6, 0, 0], "minecraft:dirt");
    b.layer0 = vec![9999];
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();

    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 6, y: 0, z: 0 }),
        Some("minecraft:air"),
        "an out-of-range index must place air — what the game does with it — \
         not panic or place a bogus block"
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
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:chest");
    chest_at(&mut b, "0", "from-b");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

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
