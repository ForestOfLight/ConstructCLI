use construct_core::pack;
use std::path::Path;

/// Writes a minimal pack directory and returns its path.
fn make_pack(
    root: &Path,
    folder: &str,
    uuid: &str,
    version: [u32; 3],
    resources: bool,
) -> std::path::PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let module = if resources { "resources" } else { "data" };
    let manifest = format!(
        r#"{{"format_version":2,
            "header":{{"name":"{folder}","uuid":"{uuid}","version":[{},{},{}]}},
            "modules":[{{"type":"{module}","uuid":"11111111-1111-1111-1111-111111111111","version":[1,0,0]}}]}}"#,
        version[0], version[1], version[2]
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    dir
}

#[test]
fn construct_is_found_by_uuid_under_any_folder_name() {
    let root = tempfile::tempdir().unwrap();
    make_pack(
        root.path(),
        "SomethingElse",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    make_pack(
        root.path(),
        "Canopy[BP]",
        "aaaaaaaa-0000-0000-0000-000000000000",
        [1, 0, 0],
        false,
    );

    let found =
        pack::find_by_uuid(root.path(), pack::CONSTRUCT_BP_UUID).expect("should find Construct");
    assert_eq!(found.dir.file_name().unwrap(), "SomethingElse");
    assert_eq!(found.manifest.version, [1, 2, 0]);
}

#[test]
fn a_directory_without_a_manifest_is_skipped_not_an_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("junk")).unwrap();
    std::fs::write(root.path().join("loose-file.txt"), "x").unwrap();
    make_pack(
        root.path(),
        "Real",
        pack::CONSTRUCT_BP_UUID,
        [1, 0, 0],
        false,
    );

    let packs = pack::packs_in(root.path());
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].manifest.uuid, pack::CONSTRUCT_BP_UUID);
}

#[test]
fn a_pack_with_a_broken_manifest_is_skipped_rather_than_failing_the_scan() {
    // One corrupt pack must not hide every other pack on the machine.
    let root = tempfile::tempdir().unwrap();
    let broken = root.path().join("Broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("manifest.json"), "{ not json").unwrap();
    make_pack(
        root.path(),
        "Good",
        pack::CONSTRUCT_BP_UUID,
        [1, 0, 0],
        false,
    );

    assert_eq!(pack::packs_in(root.path()).len(), 1);
}

#[test]
fn a_missing_root_yields_no_packs() {
    assert!(pack::packs_in(Path::new("/no/such/root")).is_empty());
}

#[test]
fn the_pack_roots_are_the_documented_folder_names() {
    let base = Path::new("/com.mojang");
    assert_eq!(
        pack::behavior_root(base),
        Path::new("/com.mojang/development_behavior_packs")
    );
    assert_eq!(
        pack::resource_root(base),
        Path::new("/com.mojang/development_resource_packs")
    );
}
