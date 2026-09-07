mod support;

use construct_core::mcstructure::{self, VOID};
use support::{Build, block, compound, int_list};

#[test]
fn a_single_block_structure_decodes() {
    let b = Build::solid([1, 1, 1], [10, 64, -3], "minecraft:stone");
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.format_version, 1);
    assert_eq!(s.size, mcstructure::Size { x: 1, y: 1, z: 1 });
    assert_eq!(
        s.origin,
        mcstructure::Coord {
            x: 10,
            y: 64,
            z: -3
        }
    );
    assert_eq!(s.layers[0], vec![0]);
    assert_eq!(s.layers[1], vec![VOID]);
    assert_eq!(s.palette.len(), 1);
    assert_eq!(s.palette[0].name, "minecraft:stone");
    assert_eq!(s.palette[0].version, 18163713);
    assert!(s.block_position_data.is_empty());
    assert!(s.entities.is_empty());
}

#[test]
fn the_second_layer_carries_waterlogging() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    b.palette.push(block("minecraft:water"));
    b.layer1 = vec![1];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.layers[0], vec![0]);
    assert_eq!(s.layers[1], vec![1]);
    assert_eq!(s.palette[1].name, "minecraft:water");
}

#[test]
fn void_gaps_decode_as_negative_one() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, VOID];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert_eq!(s.layers[0], vec![0, VOID]);
}

#[test]
fn block_position_data_decodes_keyed_by_index() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    b.block_position_data = vec![(
        "1".to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![("id", nbtx::Value::String("Chest".into()))]),
        )]),
    )];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.block_position_data.len(), 1);
    assert!(s.block_position_data.contains_key(&1usize));
}

#[test]
fn a_block_position_data_of_the_wrong_tag_type_is_refused_not_dropped() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:chest");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    let nbtx::Value::Compound(mut structure) = root["structure"].clone() else {
        unreachable!()
    };
    let nbtx::Value::Compound(mut palette) = structure["palette"].clone() else {
        unreachable!()
    };
    let nbtx::Value::Compound(mut default) = palette["default"].clone() else {
        unreachable!()
    };
    default.insert("block_position_data".into(), nbtx::Value::List(vec![]));
    palette.insert("default".into(), nbtx::Value::Compound(default));
    structure.insert("palette".into(), nbtx::Value::Compound(palette));
    root.insert("structure".into(), nbtx::Value::Compound(structure));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(
        format!("{err}").contains("block_position_data"),
        "error must name the field: {err}"
    );
}

#[test]
fn entities_decode_untouched() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        (
            "Pos",
            nbtx::Value::List(vec![
                nbtx::Value::Float(1.5),
                nbtx::Value::Float(64.0),
                nbtx::Value::Float(-2.5),
            ]),
        ),
    ])];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert_eq!(s.entities.len(), 1);
    assert_eq!(s.entities[0], b.entities[0]);
}

#[test]
fn the_real_construct_fixture_decodes() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/construct.mcstructure"
    ))
    .unwrap();
    let s = mcstructure::decode(&bytes, "construct.mcstructure").unwrap();
    assert_eq!(s.size, mcstructure::Size { x: 7, y: 7, z: 7 });
    assert_eq!(s.layers[0].len(), 343);
    assert_eq!(s.layers[1].len(), 343);
    assert_eq!(s.palette.len(), 6);
}

#[test]
fn a_missing_required_field_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    root.remove("size");
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(
        format!("{err}").contains("size"),
        "error must name the field: {err}"
    );
}

#[test]
fn block_indices_with_other_than_two_layers_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    let nbtx::Value::Compound(mut structure) = root["structure"].clone() else {
        unreachable!()
    };
    structure.insert(
        "block_indices".into(),
        nbtx::Value::List(vec![int_list(&[0])]),
    );
    root.insert("structure".into(), nbtx::Value::Compound(structure));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(
        format!("{err}").contains('2'),
        "error must say two are required: {err}"
    );
}

#[test]
fn layers_of_different_lengths_are_refused() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer1 = vec![VOID];
    let err = mcstructure::decode(&b.bytes(), "test").unwrap_err();
    assert!(
        format!("{err}").contains("same"),
        "error must say they must match: {err}"
    );
}

#[test]
fn a_layer_length_that_disagrees_with_size_is_refused() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, 0, 0];
    b.layer1 = vec![VOID, VOID, VOID];
    let err = mcstructure::decode(&b.bytes(), "test").unwrap_err();
    assert!(
        format!("{err}").contains("size"),
        "error must blame size: {err}"
    );
}

#[test]
fn a_missing_default_palette_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    let nbtx::Value::Compound(mut structure) = root["structure"].clone() else {
        unreachable!()
    };
    structure.insert("palette".into(), compound(vec![]));
    root.insert("structure".into(), nbtx::Value::Compound(structure));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(
        format!("{err}").contains("no blocks"),
        "error must give the dedicated no-default-palette reason: {err}"
    );
}

#[test]
fn a_negative_size_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    root.insert("size".into(), int_list(&[-1, 1, 1]));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();
    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(
        format!("{err}").contains("negative dimension"),
        "error must name the negative dimension, not just any failure: {err}"
    );
}

#[test]
fn bytes_that_are_not_nbt_at_all_are_refused_by_name() {
    let err = mcstructure::decode(b"not nbt", "broken.mcstructure").unwrap_err();
    assert!(
        format!("{err}").contains("broken.mcstructure"),
        "the error must name the file: {err}"
    );
}

fn assert_round_trips(s: &construct_core::mcstructure::Structure) {
    let bytes = mcstructure::encode(s, "test").unwrap();
    let again = mcstructure::decode(&bytes, "test").unwrap();
    assert_eq!(again.format_version, s.format_version);
    assert_eq!(again.size, s.size);
    assert_eq!(again.origin, s.origin);
    assert_eq!(again.layers, s.layers);
    assert_eq!(again.palette, s.palette);
    assert_eq!(again.block_position_data, s.block_position_data);
    assert_eq!(again.entities, s.entities);
}

#[test]
fn a_single_block_structure_round_trips() {
    let b = Build::solid([1, 1, 1], [10, 64, -3], "minecraft:stone");
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_waterlogged_structure_round_trips() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    b.palette.push(block("minecraft:water"));
    b.layer1 = vec![1];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_void_gaps_round_trips() {
    let mut b = Build::solid([3, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, VOID, 0];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_block_entities_round_trips() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    b.block_position_data = vec![(
        "1".to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![("id", nbtx::Value::String("Chest".into()))]),
        )]),
    )];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_entities_round_trips() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        (
            "Pos",
            nbtx::Value::List(vec![
                nbtx::Value::Float(1.5),
                nbtx::Value::Float(64.0),
                nbtx::Value::Float(-2.5),
            ]),
        ),
    ])];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn an_empty_entity_list_survives_encoding() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert!(s.entities.is_empty());
    let bytes = mcstructure::encode(&s, "test").unwrap();
    assert!(mcstructure::decode(&bytes, "test").is_ok());
}

#[test]
fn the_real_construct_fixture_round_trips() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/construct.mcstructure"
    ))
    .unwrap();
    let s = mcstructure::decode(&bytes, "construct.mcstructure").unwrap();
    assert_round_trips(&s);

    let re = mcstructure::encode(&s, "construct.mcstructure").unwrap();
    assert_eq!(
        re.len(),
        bytes.len(),
        "re-encoding changed the payload length"
    );
}

#[test]
fn encoding_refuses_a_structure_whose_layers_disagree_with_its_size() {
    let b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut s = mcstructure::decode(&b.bytes(), "test").unwrap();
    s.layers[0].push(0);
    let err = mcstructure::encode(&s, "test").unwrap_err();
    assert!(format!("{err}").contains("size"), "{err}");
}

#[test]
fn encoding_refuses_a_palette_index_out_of_range() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut s = mcstructure::decode(&b.bytes(), "test").unwrap();
    s.layers[0] = vec![5];
    let err = mcstructure::encode(&s, "test").unwrap_err();
    assert!(format!("{err}").contains("palette"), "{err}");
}

#[test]
fn encoding_refuses_a_negative_size_dimension() {
    let s = mcstructure::Structure {
        format_version: 1,
        size: mcstructure::Size { x: -5, y: 3, z: 2 },
        origin: mcstructure::Coord { x: 0, y: 0, z: 0 },
        layers: [vec![], vec![]],
        palette: vec![],
        block_position_data: Default::default(),
        entities: vec![],
    };
    let err = mcstructure::encode(&s, "test").unwrap_err();
    let err_str = format!("{err}");
    assert!(
        err_str.contains("negative dimension"),
        "error must mention negative dimension: {err}"
    );
    assert!(
        err_str.contains("-5"),
        "error must name the negative value: {err}"
    );
}

#[test]
fn deeply_nested_nbt_is_refused_rather_than_overflowing_the_stack() {
    let depth = 50_000;
    let mut bytes = Vec::new();
    for _ in 0..depth {
        bytes.extend_from_slice(&[0x0a, 0x01, 0x00, b'a']);
    }
    bytes.resize(bytes.len() + depth, 0x00);
    let err = mcstructure::decode(&bytes, "deep.mcstructure").unwrap_err();
    assert!(
        format!("{err}").contains("deep.mcstructure"),
        "the refusal must name the file: {err}"
    );
}
