use crate::support::*;

#[test]
fn structures_by_path_shows_structures_with_their_source() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["structures", "--world", world.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("house"), "{text}");
    assert!(text.contains("barn"), "{text}");
    assert!(
        text.contains("world-db"),
        "source column should say world-db:\n{text}"
    );
}

#[test]
fn structures_json_carries_schema_and_entries() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["structures", "--world", world.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"house"));
    assert!(
        names.contains(&"understudy:players"),
        "non-default namespaces stay qualified"
    );
}

#[test]
fn structures_source_pack_with_no_reachable_construct_is_not_found() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args([
            "structures",
            "--world",
            world.to_str().unwrap(),
            "--source",
            "world-pack",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn structures_shows_a_long_name_in_full_not_truncated() {
    let long_name = "amelix_concrete_convertor";
    assert!(long_name.len() > 24);
    let (_tmp, world) =
        fixture_world_with_extra_structures(&[(&format!("mystructure:{long_name}"), b"AAAAAAAA")]);
    let out = bin()
        .args(["structures", "--world", world.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(long_name),
        "full name must appear untruncated:\n{text}"
    );
    assert!(
        !text.contains('…'),
        "no ellipsis should appear in list output:\n{text}"
    );
}

#[test]
fn structures_of_a_missing_world_reports_world_not_found() {
    let out = bin()
        .args(["structures", "--world", "definitely-not-a-world"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn structures_shows_pack_structures_without_touching_the_world_database() {
    let root = world_with_construct(&[("bomber", b"12345")]);
    let out = bin()
        .args([
            "structures",
            "--world",
            "Test",
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
    assert_eq!(v["structures"][0]["name"], "bomber");
    assert_eq!(v["structures"][0]["source"], "shared-pack");
    assert_eq!(v["structures"][0]["size_bytes"], 5);
}

#[test]
fn source_pack_on_a_machine_without_construct_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let out = bin()
        .args([
            "structures",
            "--world",
            "Test",
            "--source",
            "world-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}

#[test]
fn a_name_in_both_world_and_pack_survives_unshadowed_in_the_structures_listing() {
    let (root, world_name) = fixture_world_with_construct(
        &[("mystructure:collide", b"WORLDBYTES")],
        &[("collide", b"PACKBYTES!")],
    );
    let out = bin()
        .args([
            "structures",
            "--world",
            world_name,
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
    let collide_entries: Vec<&serde_json::Value> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["name"] == "collide")
        .collect();
    assert_eq!(
        collide_entries.len(),
        2,
        "both the world and pack copies of `collide` must be listed, not one shadowing the other: {v}"
    );
    let sources: std::collections::BTreeSet<&str> = collide_entries
        .iter()
        .map(|e| e["source"].as_str().unwrap())
        .collect();
    assert_eq!(sources, ["shared-pack", "world-db"].into_iter().collect());
}

#[test]
fn structures_with_no_world_shows_only_the_shared_copy() {
    let root = world_with_construct(&[("shared_prefab", b"x")]);
    let src = root.path().join("mine.mcstructure");
    std::fs::write(&src, b"y").unwrap();
    assert!(
        bin()
            .args([
                "import",
                src.to_str().unwrap(),
                "--world",
                "Test",
                "--path",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap()
            .status
            .success()
    );

    let out = bin()
        .args([
            "structures",
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
    assert_eq!(v["world"], serde_json::Value::Null, "no world was named");
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["shared_prefab"], "{v}");
    assert_eq!(v["structures"][0]["source"], "shared-pack");
}

#[test]
fn structures_with_no_world_refuses_the_sources_that_need_one() {
    let root = world_with_construct(&[]);
    for source in ["world-db", "world-pack"] {
        let out = bin()
            .args([
                "structures",
                "--source",
                source,
                "--path",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "--source {source}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("--source {source} needs a world")),
            "stderr:\n{stderr}"
        );
    }
}

#[test]
fn structures_with_no_world_and_no_construct_points_at_install() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args(["structures", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}

#[test]
fn structures_says_which_pack_each_structure_is_in() {
    let (root, world) = fixture_world_with_construct(&[], &[("shared_prefab", b"x")]);
    let src = root.path().join("mine.mcstructure");
    std::fs::write(&src, b"y").unwrap();
    assert!(
        bin()
            .args([
                "import",
                src.to_str().unwrap(),
                "--world",
                world,
                "--path",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap()
            .status
            .success()
    );

    let out = bin()
        .args([
            "structures",
            "--world",
            world,
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.lines()
            .any(|l| l.starts_with("mine") && l.contains("world-pack")),
        "stdout:\n{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("shared_prefab") && l.contains("shared-pack")),
        "stdout:\n{text}"
    );
}

#[test]
fn structures_takes_a_shared_source_under_a_world_where_export_and_delete_refuse_it() {
    let root = world_seeing_one_name_in_both_packs();

    let out = bin()
        .args([
            "structures",
            "--world",
            "Test",
            "--pack",
            "shared",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument '--pack'"),
        "stderr:\n{stderr}"
    );

    let out = bin()
        .args([
            "structures",
            "--world",
            "Test",
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
    let rows = v["structures"].as_array().unwrap();
    assert!(!rows.is_empty(), "{v}");
    assert!(
        rows.iter().all(|e| e["source"] == "shared-pack"),
        "every row must be from the pack that was named: {v}"
    );
}

#[test]
fn a_world_listing_shows_both_packs_it_sees() {
    let root = world_seeing_one_name_in_both_packs();
    let rows = |source: &str| -> serde_json::Value {
        let out = bin()
            .args([
                "structures",
                "--world",
                "Test",
                "--source",
                source,
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
        serde_json::from_slice(&out.stdout).unwrap()
    };

    for source in ["world-pack", "shared-pack"] {
        let v = rows(source);
        let houses: Vec<&serde_json::Value> = v["structures"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["name"] == "house")
            .collect();
        assert_eq!(houses.len(), 1, "--source {source}: {v}");
        assert_eq!(houses[0]["source"], source, "{v}");
    }
}

#[test]
fn a_name_in_two_packs_serving_one_world_warns() {
    let root = world_with_construct(&[("house", b"shared")]);
    let src = root.path().join("house.mcstructure");
    std::fs::write(&src, b"mine").unwrap();

    let out = bin()
        .args([
            "import",
            src.to_str().unwrap(),
            "--world",
            "Test",
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
        stderr.contains("conflict") && stderr.contains("house"),
        "stderr:\n{stderr}"
    );
}
