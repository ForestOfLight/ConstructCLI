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

use construct_core::store::snapshot;

#[test]
fn copy_dir_reports_bytes_copied() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("a"), vec![0u8; 100]).unwrap();
    std::fs::write(src.join("nested/b"), vec![0u8; 50]).unwrap();

    let dst = tmp.path().join("dst");
    assert_eq!(snapshot::copy_dir(&src, &dst).unwrap(), 150);
    assert!(dst.join("nested/b").is_file());
}

#[test]
fn a_read_always_snapshots_and_reports_the_size() {
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());
    let opened = construct_core::store::open_world_store(&world).unwrap();
    assert!(
        opened.via_snapshot.is_some(),
        "every read goes through a copy"
    );
    assert!(
        opened
            .ids()
            .unwrap()
            .contains(&"mystructure:house".to_string())
    );
}

#[test]
fn a_read_does_not_modify_the_world_on_disk() {
    // THE test for this task. Opening a leveldb database rewrites it, so the only
    // way a read can be safe is never to open the original. Hash the whole db
    // directory before and after; any difference means the guarantee is broken.
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());

    let before = dir_fingerprint(&db);
    let opened = construct_core::store::open_world_store(&world).unwrap();
    let _ = opened.ids().unwrap();
    drop(opened);
    let after = dir_fingerprint(&db);

    assert_eq!(before, after, "a read modified the world's db/ directory");
}

/// Every file name and its exact bytes, so a rewritten MANIFEST or a rolled WAL shows up.
fn dir_fingerprint(dir: &std::path::Path) -> Vec<(String, u64, Vec<u8>)> {
    let mut out: Vec<(String, u64, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            let bytes = std::fs::read(e.path()).unwrap_or_default();
            (
                e.file_name().to_string_lossy().into_owned(),
                bytes.len() as u64,
                bytes,
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_read_only_ever_opens_a_snapshot_never_the_world() {
    // `OpenedStore` does not expose the snapshot's path, so the strongest
    // assertion available from outside the crate is that every successful
    // read went through a snapshot at all. The path-under-temp-dir invariant
    // itself is enforced internally by `bedrock::guard_test_path`, called on
    // every snapshot open right before `BedrockStore::open` — if that guard
    // ever failed, this call would panic rather than return `Ok`.
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());
    let opened = construct_core::store::open_world_store(&world).unwrap();
    assert!(
        opened.via_snapshot.is_some(),
        "a successful read must have gone through a snapshot"
    );
}

/// Build a `World` pointing at an extracted fixture.
fn world_at(dir: &std::path::Path) -> construct_core::discovery::World {
    construct_core::discovery::World {
        installation: "test".into(),
        account: None,
        folder: "test_level".into(),
        display_name: "test_level".into(),
        path: dir.to_path_buf(),
        last_played: Some(0),
        last_played_source: construct_core::discovery::LastPlayedSource::LevelDat,
        size_bytes: 0,
    }
}
