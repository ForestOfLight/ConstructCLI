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
    let store = BedrockStore::open_copy(&db).unwrap();
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
    let store = BedrockStore::open_copy(&db).unwrap();
    let bytes = store.get("house").unwrap().expect("house is present");
    assert!(!bytes.is_empty());
    // A .mcstructure is little-endian NBT, which begins with TAG_Compound.
    assert_eq!(bytes[0], 0x0a);
}

#[test]
fn gets_a_structure_by_qualified_name() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open_copy(&db).unwrap();
    assert!(store.get("understudy:players").unwrap().is_some());
    // The bare form must NOT find a non-default namespace.
    assert!(store.get("players").unwrap().is_none());
}

#[test]
fn a_missing_structure_is_none_not_an_error() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open_copy(&db).unwrap();
    assert!(store.get("nope").unwrap().is_none());
}

#[test]
fn opening_a_nonexistent_database_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(BedrockStore::open_copy(&tmp.path().join("no-db")).is_err());
}

#[test]
#[should_panic(expected = "outside a temp directory")]
fn the_guard_refuses_a_path_outside_temp() {
    construct_core::store::bedrock::guard_copy_path(std::path::Path::new(
        "/Users/someone/world/db",
    ));
}

/// A `World` pointing at an extracted fixture, for the write path.
///
/// `open_live` takes a `&World` rather than a `&Path` on purpose — it is the
/// one opener with no temp-directory guard, so it must not be reachable by
/// handing a path to a general-purpose function. Building one here is the
/// cost of that, and it is the point.
fn fixture_world(dir: &std::path::Path) -> construct_core::discovery::World {
    construct_core::discovery::World {
        installation: "test".into(),
        account: None,
        folder: "test_level".into(),
        display_name: "test_level".into(),
        path: dir.join("test_level"),
        last_played: None,
        last_played_source: construct_core::discovery::LastPlayedSource::DirMtime,
        size_bytes: 0,
    }
}

#[test]
fn open_live_removes_a_structure_and_a_fresh_open_agrees() {
    let (tmp, _db) = extract();
    let world = fixture_world(tmp.path());

    {
        let live = BedrockStore::open_live(&world).unwrap();
        assert!(live.get("house").unwrap().is_some(), "fixture has house");
        assert!(live.remove("mystructure:house").unwrap(), "it was removed");
    }

    // A separate open, after the write handle has been dropped and flushed.
    let reopened = BedrockStore::open_live(&world).unwrap();
    assert!(reopened.get("house").unwrap().is_none());
    // Only the named key went.
    assert!(reopened.get("barn").unwrap().is_some());
    assert!(reopened.get("understudy:players").unwrap().is_some());
}

#[test]
fn removing_a_structure_that_is_not_there_reports_false() {
    // leveldb's own `Delete` succeeds for a key that was never present, so
    // without the lookup beforehand "deleted" would mean nothing.
    let (tmp, _db) = extract();
    let world = fixture_world(tmp.path());
    let live = BedrockStore::open_live(&world).unwrap();
    assert!(!live.remove("nope").unwrap());
}

#[test]
fn a_bare_name_removes_only_the_default_namespace() {
    // `understudy:players` must survive a `delete players`, the same way
    // `get` refuses to find it under a bare name.
    let (tmp, _db) = extract();
    let world = fixture_world(tmp.path());
    let live = BedrockStore::open_live(&world).unwrap();
    assert!(!live.remove("players").unwrap());
    assert!(live.get("understudy:players").unwrap().is_some());
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

/// Builds a directory shaped like a leveldb: one immutable table, and the
/// three kinds of file leveldb appends to or replaces in place.
fn fake_db(at: &std::path::Path) {
    std::fs::create_dir_all(at).unwrap();
    std::fs::write(at.join("000005.ldb"), vec![7u8; 4096]).unwrap();
    std::fs::write(at.join("000006.log"), b"log records").unwrap();
    std::fs::write(at.join("MANIFEST-000004"), b"manifest records").unwrap();
    std::fs::write(at.join("CURRENT"), b"MANIFEST-000004\n").unwrap();
}

#[cfg(unix)]
fn same_inode(a: &std::path::Path, b: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (std::fs::metadata(a).unwrap(), std::fs::metadata(b).unwrap());
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(unix)]
#[test]
fn a_snapshot_hardlinks_the_immutable_table_files() {
    // `.ldb` files are written once and thereafter only read or unlinked, so a
    // link is as good as a copy and costs nothing. This is the whole point of
    // the exercise: a gigabyte world should not need a gigabyte of copying.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("db");
    fake_db(&src);
    let dst = tmp.path().join("snap");

    snapshot::link_or_copy_dir(&src, &dst).unwrap();

    assert!(
        same_inode(&src.join("000005.ldb"), &dst.join("000005.ldb")),
        "an immutable table must be linked, not copied"
    );
}

#[cfg(unix)]
#[test]
fn a_snapshot_copies_the_files_leveldb_writes_in_place() {
    // The log and the manifest are *appended to* in place, and CURRENT is
    // replaced. Linking those would let a running game's writes bleed into the
    // snapshot mid-read, which is precisely the tearing the snapshot exists to
    // prevent.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("db");
    fake_db(&src);
    let dst = tmp.path().join("snap");

    snapshot::link_or_copy_dir(&src, &dst).unwrap();

    for name in ["000006.log", "MANIFEST-000004", "CURRENT"] {
        assert!(
            !same_inode(&src.join(name), &dst.join(name)),
            "{name} is written in place and must be a real copy"
        );
    }
}

#[test]
fn a_snapshot_is_unaffected_by_later_writes_to_the_live_log() {
    // The behavioural statement of the test above: whatever the mechanism, an
    // autosave landing after the snapshot must not change what the snapshot
    // reads.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("db");
    fake_db(&src);
    let dst = tmp.path().join("snap");

    snapshot::link_or_copy_dir(&src, &dst).unwrap();
    std::fs::write(src.join("000006.log"), b"log records + an autosave").unwrap();

    assert_eq!(
        std::fs::read(dst.join("000006.log")).unwrap(),
        b"log records",
        "the snapshot must hold the bytes that were there when it was taken"
    );
}

#[test]
fn discarding_a_snapshot_leaves_the_linked_originals_intact() {
    // Unlinking a hardlink drops one name, not the file. If this were wrong,
    // dropping the TempDir at the end of every read would delete the world's
    // tables — the worst bug this change could introduce.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("db");
    fake_db(&src);
    let snap = tempfile::tempdir().unwrap();
    let dst = snap.path().join("db");

    snapshot::link_or_copy_dir(&src, &dst).unwrap();
    drop(snap);

    assert_eq!(
        std::fs::read(src.join("000005.ldb")).unwrap(),
        vec![7u8; 4096],
        "the world's table survived the snapshot being discarded"
    );
}

#[test]
fn a_snapshot_reports_only_the_bytes_it_actually_copied() {
    // The 4096-byte table is linked; the three small files are copied. The
    // reported figure is what the read cost, so it must not count linked bytes.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("db");
    fake_db(&src);
    let dst = tmp.path().join("snap");

    let copied = snapshot::link_or_copy_dir(&src, &dst).unwrap();
    let small = 11 + 16 + 16; // log + manifest + CURRENT

    assert_eq!(copied, small, "linked bytes are not copied bytes");
}

#[test]
fn a_read_links_the_world_tables_instead_of_copying_them() {
    // The reported figure is bytes copied. If the read is linking the tables,
    // it is necessarily well below the size of the directory it snapshotted.
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());

    let opened = construct_core::store::open_world_store(&world).unwrap();
    let copied = opened.via_snapshot.expect("read went through a snapshot");
    let total = snapshot::dir_size(&db);

    assert!(
        copied < total,
        "a linking snapshot copies less than the whole directory: {copied} of {total}"
    );
}
