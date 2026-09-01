use construct_core::store::StructureStore;
use construct_core::store::bedrock::BedrockStore;
use std::path::PathBuf;

/// Extracts the fixture world into a fresh temp dir. Every test gets its own
/// copy, so a test that writes cannot affect another.
fn extract() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let gz = std::fs::File::open("tests/fixtures/world.tar.gz").expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(tmp.path())
        .expect("unpack");
    let db = tmp.path().join("test_level/db");
    (tmp, db)
}

#[test]
fn lists_only_structure_keys() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    let mut ids = store.ids().unwrap();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "mystructure:barn".to_string(),
            "mystructure:house".to_string(),
            "understudy:players".to_string(),
        ],
        "the fixture world has many other keys; none of them are structures"
    );
}

#[test]
fn gets_a_structure_by_bare_name() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    let bytes = store.get("house").unwrap().expect("house is present");
    assert!(!bytes.is_empty());
    // A .mcstructure is little-endian NBT, which begins with TAG_Compound.
    assert_eq!(bytes[0], 0x0a);
}

#[test]
fn gets_a_structure_by_qualified_name() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    assert!(store.get("understudy:players").unwrap().is_some());
    // The bare form must NOT find a non-default namespace.
    assert!(store.get("players").unwrap().is_none());
}

#[test]
fn a_missing_structure_is_none_not_an_error() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    assert!(store.get("nope").unwrap().is_none());
}

#[test]
fn opening_a_nonexistent_database_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(BedrockStore::open(&tmp.path().join("no-db")).is_err());
}

#[test]
#[should_panic(expected = "outside a temp directory")]
fn the_guard_refuses_a_path_outside_temp() {
    construct_core::store::bedrock::guard_test_path(std::path::Path::new(
        "/Users/someone/world/db",
    ));
}
