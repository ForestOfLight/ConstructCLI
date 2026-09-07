use crate::support::*;

#[test]
fn merge_without_o_is_a_usage_error() {
    let root = world_with_construct(&[]);
    let out = bin()
        .args([
            "export",
            "a",
            "b",
            "--merge",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn merge_writes_one_file_from_several_structures() {
    let a = merge_fixture([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = merge_fixture([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a), ("tower", &b)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export",
            "north",
            "tower",
            "--merge",
            "-n",
            target.to_str().unwrap(),
            "--json",
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
    assert!(target.is_file());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["merged"]["sources"].as_array().unwrap().len(), 2);
    assert_eq!(v["merged"]["size"], serde_json::json!([4, 1, 1]));
    assert_eq!(v["merged"]["origin"], serde_json::json!([0, 0, 0]));

    let bytes = std::fs::read(&target).unwrap();
    let s = construct_core::mcstructure::decode(&bytes, "merged").unwrap();
    assert_eq!(
        s.size,
        construct_core::mcstructure::Size { x: 4, y: 1, z: 1 }
    );
}

#[test]
fn merge_refuses_an_existing_target_without_force() {
    let a = merge_fixture([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = merge_fixture([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a), ("tower", &b)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args([
            "export",
            "north",
            "tower",
            "--merge",
            "-n",
            target.to_str().unwrap(),
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(&target).unwrap(), b"existing");
}

#[test]
fn merge_reports_overlap_on_stderr_and_in_the_payload() {
    let mut a = support_build([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = support_build([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a.bytes()), ("tower", &b.bytes())]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export",
            "north",
            "tower",
            "--merge",
            "-n",
            target.to_str().unwrap(),
            "--json",
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("overlap"),
        "expected an overlap warning: {stderr}"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w
            .as_str()
            .is_some_and(|s| s.to_lowercase().contains("overlap")))),
        "overlap must appear in the payload too: {v}"
    );
}

#[test]
fn merge_with_on_overlap_error_exits_1_and_writes_nothing() {
    let mut a = support_build([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = support_build([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a.bytes()), ("tower", &b.bytes())]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export",
            "north",
            "tower",
            "--merge",
            "--on-overlap",
            "error",
            "-n",
            target.to_str().unwrap(),
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!target.exists(), "a refused merge must write nothing");
}

#[test]
fn merging_one_structure_is_allowed() {
    let a = merge_fixture([2, 2, 2], [7, 7, 7], "minecraft:stone");
    let root = world_with_construct(&[("solo", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("one.mcstructure");

    let out = bin()
        .args([
            "export",
            "solo",
            "--merge",
            "-n",
            target.to_str().unwrap(),
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
    assert!(target.is_file());
}
