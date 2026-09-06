//! `install::adopt`: rescuing a Construct that was hand-dropped into
//! `behavior_packs` instead of `development_behavior_packs`.

use construct_core::install::adopt::{self, AdoptKind};
use construct_core::pack::CONSTRUCT_BP_UUID;
use std::path::{Path, PathBuf};

/// A minimal but real pack directory: a parseable manifest carrying `uuid`.
fn pack_at(root: &Path, folder: &str, uuid: &str) -> PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"format_version":2,
                "header":{{"name":"{folder}","uuid":"{uuid}","version":[1,2,0]}},
                "modules":[{{"type":"data","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}}]}}"#
        ),
    )
    .unwrap();
    dir
}

fn structure(pack: &Path, relative: &str, contents: &[u8]) {
    let path = pack.join("structures").join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The two sibling roots under one `com.mojang`.
fn roots() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dev = tmp.path().join("development_behavior_packs");
    let stray = tmp.path().join("behavior_packs");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::create_dir_all(&stray).unwrap();
    (tmp, dev, stray)
}

#[test]
fn a_root_with_no_construct_in_it_is_nothing_to_adopt() {
    let (_tmp, dev, stray) = roots();
    pack_at(&stray, "SomeoneElsesPack", "11111111-1111-1111-1111-111111111111");

    assert!(adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().is_none());
    assert!(stray.join("SomeoneElsesPack/manifest.json").is_file());
}

#[test]
fn a_missing_stray_root_is_nothing_to_adopt() {
    let (tmp, dev, _stray) = roots();
    let absent = tmp.path().join("no_such_root");

    assert!(adopt::adopt(&dev, &absent, CONSTRUCT_BP_UUID).unwrap().is_none());
}

#[test]
fn a_stray_construct_moves_into_the_development_root() {
    let (_tmp, dev, stray) = roots();
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"the user's house");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(adopted.kind, AdoptKind::Moved);
    assert_eq!(adopted.to, dev.join("Construct[BP]"));
    assert_eq!(
        read(&dev.join("Construct[BP]/structures/house.mcstructure")),
        b"the user's house"
    );
    assert!(!from.exists(), "the stray copy is gone from behavior_packs");
}

#[test]
fn a_moved_pack_takes_a_free_name_when_its_own_is_taken() {
    // An unrelated pack already occupies the folder name. Overwriting it
    // would destroy someone else's pack; `place` matches by UUID, so the
    // folder name it lands under does not matter.
    let (_tmp, dev, stray) = roots();
    let squatter = pack_at(&dev, "Construct[BP]", "11111111-1111-1111-1111-111111111111");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"the user's house");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(adopted.to, dev.join("Construct[BP]-2"));
    assert_eq!(
        read(&dev.join("Construct[BP]-2/structures/house.mcstructure")),
        b"the user's house"
    );
    assert_eq!(
        construct_core::pack::manifest::read(&squatter).unwrap().uuid,
        "11111111-1111-1111-1111-111111111111",
        "the unrelated pack is untouched"
    );
}

#[test]
fn a_stray_beside_an_installed_copy_merges_its_unique_structures() {
    let (_tmp, dev, stray) = roots();
    let installed = pack_at(&dev, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&installed, "installed.mcstructure", b"already here");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"the user's house");
    structure(&from, "stuff/towers/diamond.mcstructure", b"nested");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(adopted.kind, AdoptKind::Merged);
    assert_eq!(adopted.merged, 2);
    assert!(adopted.rescued.is_empty());
    assert_eq!(
        read(&installed.join("structures/house.mcstructure")),
        b"the user's house"
    );
    assert_eq!(
        read(&installed.join("structures/stuff/towers/diamond.mcstructure")),
        b"nested",
        "a namespaced structure keeps its subfolder layout"
    );
    assert_eq!(
        read(&installed.join("structures/installed.mcstructure")),
        b"already here",
        "the installed copy's own structures are left alone"
    );
    assert!(!from.exists(), "the stray copy is gone from behavior_packs");
}

#[test]
fn a_structure_present_in_both_copies_with_the_same_bytes_is_not_duplicated() {
    let (_tmp, dev, stray) = roots();
    let installed = pack_at(&dev, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&installed, "house.mcstructure", b"identical");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"identical");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(adopted.merged, 0);
    assert!(adopted.rescued.is_empty());
    assert!(!installed.join("structures/house-1.mcstructure").exists());
}

#[test]
fn a_structure_that_differs_between_the_copies_is_kept_under_a_free_name() {
    let (_tmp, dev, stray) = roots();
    let installed = pack_at(&dev, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&installed, "house.mcstructure", b"the installed house");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"a different house");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(
        read(&installed.join("structures/house.mcstructure")),
        b"the installed house",
        "the installed copy wins its own path"
    );
    assert_eq!(
        read(&installed.join("structures/house-1.mcstructure")),
        b"a different house",
        "and the stray's version is rescued beside it"
    );
    assert_eq!(adopted.merged, 1);
    assert_eq!(
        adopted.rescued,
        vec![adopt::Rescued {
            from: "house.mcstructure".to_string(),
            to: "house-1.mcstructure".to_string(),
        }]
    );
}

#[test]
fn a_rescue_skips_past_names_that_are_themselves_taken() {
    let (_tmp, dev, stray) = roots();
    let installed = pack_at(&dev, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&installed, "house.mcstructure", b"the installed house");
    structure(&installed, "house-1.mcstructure", b"an earlier rescue");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"a different house");

    adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(
        read(&installed.join("structures/house-1.mcstructure")),
        b"an earlier rescue"
    );
    assert_eq!(
        read(&installed.join("structures/house-2.mcstructure")),
        b"a different house"
    );
}

#[test]
fn a_nested_structure_is_rescued_inside_its_own_namespace_folder() {
    let (_tmp, dev, stray) = roots();
    let installed = pack_at(&dev, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&installed, "stuff/tower.mcstructure", b"the installed tower");
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "stuff/tower.mcstructure", b"a different tower");

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(
        read(&installed.join("structures/stuff/tower-1.mcstructure")),
        b"a different tower",
        "the rescue keeps the namespace, so the id stays stuff:tower-1"
    );
    assert_eq!(
        adopted.rescued,
        vec![adopt::Rescued {
            from: "stuff/tower.mcstructure".to_string(),
            to: "stuff/tower-1.mcstructure".to_string(),
        }]
    );
}

#[test]
fn a_stray_with_no_structures_folder_still_merges_and_is_removed() {
    // The resource pack's shape: nothing to carry across, but the duplicate
    // still has to stop shadowing the development copy.
    let (_tmp, dev, stray) = roots();
    pack_at(&dev, "Construct[RP]", CONSTRUCT_BP_UUID);
    let from = pack_at(&stray, "Construct[RP]", CONSTRUCT_BP_UUID);

    let adopted = adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(adopted.merged, 0);
    assert!(!from.exists());
}

#[test]
fn adopting_is_a_no_op_the_second_time() {
    let (_tmp, dev, stray) = roots();
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"the user's house");

    adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();
    assert!(adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().is_none());
    assert_eq!(
        read(&dev.join("Construct[BP]/structures/house.mcstructure")),
        b"the user's house"
    );
}

#[test]
fn a_dev_root_that_does_not_exist_yet_is_created_for_the_move() {
    let tmp = tempfile::tempdir().unwrap();
    let dev = tmp.path().join("development_behavior_packs");
    let stray = tmp.path().join("behavior_packs");
    std::fs::create_dir_all(&stray).unwrap();
    let from = pack_at(&stray, "Construct[BP]", CONSTRUCT_BP_UUID);
    structure(&from, "house.mcstructure", b"the user's house");

    adopt::adopt(&dev, &stray, CONSTRUCT_BP_UUID).unwrap().unwrap();

    assert_eq!(
        read(&dev.join("Construct[BP]/structures/house.mcstructure")),
        b"the user's house"
    );
}
