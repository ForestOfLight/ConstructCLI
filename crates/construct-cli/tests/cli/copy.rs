use crate::support::*;

#[test]
fn copy_reads_from_a_real_world_database_into_the_destinations_pack() {
    let (root, src_name) = fixture_world_with_construct(&[], &[]);
    let worlds_dir = root.path().join("minecraftWorlds");
    let dst_world = add_destination_world(&worlds_dir, "RealDestination");

    let out = bin()
        .args([
            "copy",
            src_name,
            "RealDestination",
            "house",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"][0]["name"], "house");
    assert_eq!(v["from"], format!("flag1/{src_name}"));
    assert_eq!(v["to"], "flag1/RealDestination");

    let written = dst_world.join("behavior_packs/Construct[BP]/structures/house.mcstructure");
    assert!(written.is_file());
    assert_eq!(std::fs::read(&written).unwrap()[0], 0x0a);
}

#[test]
fn copy_writes_bytes_into_the_destination_worlds_own_construct() {
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("levelname.txt"), "Other").unwrap();
    std::fs::write(other.join("level.dat"), b"x").unwrap();
    let bp = other.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "--source",
            "shared-pack",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"][0]["name"], "barn");
    assert_eq!(v["from"], "flag1/Test");
    assert_eq!(v["to"], "flag1/Other");
    assert_eq!(v["target"], "world-pack");

    let written = bp.join("structures/barn.mcstructure");
    assert_eq!(std::fs::read(&written).unwrap(), b"barn-bytes");
}

#[test]
fn copy_creates_the_destination_worlds_structures_pack() {
    let root = world_with_construct(&[]);
    let source_local = root
        .path()
        .join("minecraftWorlds/Test/behavior_packs/Construct[BP]");
    std::fs::create_dir_all(source_local.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        source_local.join("manifest.json"),
    )
    .unwrap();
    std::fs::write(
        source_local.join("structures/barn.mcstructure"),
        b"barn-bytes",
    )
    .unwrap();

    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("levelname.txt"), "Other").unwrap();
    std::fs::write(other.join("level.dat"), b"x").unwrap();

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "--source",
            "world-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Other's structures pack") && text.contains("that world only"),
        "stdout:\n{text}"
    );
    assert_eq!(
        std::fs::read(other.join("behavior_packs/ConstructStructures/structures/barn.mcstructure"))
            .unwrap(),
        b"barn-bytes"
    );
}

#[test]
fn copy_refuses_an_existing_target_unless_forced() {
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("level.dat"), b"x").unwrap();
    let bp = other.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();
    std::fs::write(bp.join("structures/barn.mcstructure"), b"theirs").unwrap();

    let args = [
        "copy",
        "Test",
        "Other",
        "barn",
        "--source",
        "shared-pack",
        "--path",
    ];
    let out = bin().args(args).arg(root.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        std::fs::read(bp.join("structures/barn.mcstructure")).unwrap(),
        b"theirs"
    );

    let out = bin()
        .args(args)
        .arg(root.path())
        .arg("--force")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        std::fs::read(bp.join("structures/barn.mcstructure")).unwrap(),
        b"barn-bytes"
    );
}

#[test]
fn copy_of_a_name_that_is_not_there_suggests_near_matches() {
    let root = world_with_construct(&[("barn", b"x")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("level.dat"), b"x").unwrap();
    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "bar",
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("barn"));
}

#[test]
fn copy_moves_every_structure_named() {
    let root = world_with_construct(&[("barn", b"barn-bytes"), ("silo", b"silo-bytes")]);
    let dst = destination_with_construct(root.path(), "Other");

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "silo",
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let structures = dst.join("behavior_packs/Construct[BP]/structures");
    assert_eq!(
        std::fs::read(structures.join("barn.mcstructure")).unwrap(),
        b"barn-bytes"
    );
    assert_eq!(
        std::fs::read(structures.join("silo.mcstructure")).unwrap(),
        b"silo-bytes"
    );
}

#[test]
fn copy_of_a_batch_with_one_bad_name_writes_nothing() {
    let root = world_with_construct(&[("barn", b"barn-bytes"), ("silo", b"silo-bytes")]);
    let dst = destination_with_construct(root.path(), "Other");

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "nosuchthing",
            "silo",
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    let structures = dst.join("behavior_packs/Construct[BP]/structures");
    assert!(
        !structures.join("barn.mcstructure").exists(),
        "nothing may be written when any name in the batch fails to resolve"
    );
    assert!(!structures.join("silo.mcstructure").exists());
}

#[test]
fn copy_refuses_the_whole_batch_when_one_target_already_exists() {
    let root = world_with_construct(&[("barn", b"barn-bytes"), ("silo", b"silo-bytes")]);
    let dst = destination_with_construct(root.path(), "Other");
    let structures = dst.join("behavior_packs/Construct[BP]/structures");
    std::fs::write(structures.join("silo.mcstructure"), b"theirs").unwrap();

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "silo",
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    assert!(!structures.join("barn.mcstructure").exists());
    assert_eq!(
        std::fs::read(structures.join("silo.mcstructure")).unwrap(),
        b"theirs"
    );
}

#[test]
fn copy_json_carries_a_written_array_with_from_to_and_scope_at_the_top() {
    let root = world_with_construct(&[("barn", b"barn-bytes"), ("silo", b"silo-bytes")]);
    destination_with_construct(root.path(), "Other");

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "silo",
            "--source",
            "shared-pack",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["from"], "flag1/Test");
    assert_eq!(v["to"], "flag1/Other");
    assert_eq!(v["target"], "world-pack");

    let written = v["written"].as_array().unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0]["name"], "barn");
    assert_eq!(written[0]["id"], "mystructure:barn");
    assert_eq!(written[0]["bytes"], 10);
    assert_eq!(written[1]["name"], "silo");
}

#[test]
fn copy_of_a_single_structure_still_emits_a_one_row_array() {
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    destination_with_construct(root.path(), "Other");

    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "barn",
            "--source",
            "shared-pack",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"].as_array().unwrap().len(), 1);
    assert_eq!(v["written"][0]["name"], "barn");
}

#[test]
fn copy_with_no_structure_named_is_a_usage_error() {
    let root = world_with_construct(&[("barn", b"a")]);
    let out = bin()
        .args([
            "copy",
            "Test",
            "Other",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
