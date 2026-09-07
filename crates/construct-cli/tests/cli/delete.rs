use crate::support::*;

#[test]
fn delete_unlinks_a_pack_structure() {
    let root = world_with_construct(&[("bomber", b"x")]);
    let file = root
        .path()
        .join("development_behavior_packs/Construct[BP]/structures/bomber.mcstructure");
    assert!(file.exists());

    let out = bin_isolated(root.path())
        .args(["delete", "bomber", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!file.exists());
}

#[test]
fn delete_says_when_it_removed_from_the_shared_construct() {
    let root = world_with_construct(&[("bomber", b"x")]);

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "bomber",
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
    assert!(v["world"].is_null(), "{v}");
    assert_eq!(v["deleted"][0]["source"], "shared-pack");

    let root = world_with_construct(&[("bomber", b"x")]);
    let out = bin_isolated(root.path())
        .args(["delete", "bomber", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("development_behavior_packs")
            && text.contains("shared copy of Construct")
            && text.contains("every world using it"),
        "stdout:\n{text}"
    );
}

#[test]
fn delete_says_when_it_removed_from_a_worlds_own_construct() {
    let root = world_with_construct(&[("bomber", b"shared-copy")]);
    let local = root
        .path()
        .join("minecraftWorlds/Test/behavior_packs/Construct[BP]");
    std::fs::create_dir_all(local.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        local.join("manifest.json"),
    )
    .unwrap();
    std::fs::write(local.join("structures/bomber.mcstructure"), b"local-copy").unwrap();

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "bomber",
            "--source",
            "world-pack",
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

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Test's own copy of Construct") && text.contains("that world only"),
        "stdout:\n{text}"
    );
    assert!(
        !text.contains("shared copy of Construct"),
        "the shared copy was not touched: {text}"
    );
    assert!(
        !local.join("structures/bomber.mcstructure").exists(),
        "the world's own copy should be gone"
    );
    assert_eq!(
        std::fs::read(
            root.path()
                .join("development_behavior_packs/Construct[BP]/structures/bomber.mcstructure")
        )
        .unwrap(),
        b"shared-copy",
        "the shared copy must survive untouched"
    );
}

#[test]
fn delete_with_a_world_removes_that_worlds_copy_and_leaves_the_shared_one() {
    let root = world_seeing_one_name_in_both_packs();
    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "--world",
            "Test",
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
    assert!(!world_house(root.path()).exists(), "the world's copy went");
    assert!(
        shared_house(root.path()).is_file(),
        "--world must never reach the shared copy"
    );
}

#[test]
fn delete_without_a_world_removes_the_shared_copy_and_leaves_the_worlds() {
    let root = world_seeing_one_name_in_both_packs();
    let out = bin_isolated(root.path())
        .args(["delete", "house", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!shared_house(root.path()).exists(), "the shared copy went");
    assert!(
        world_house(root.path()).is_file(),
        "a shared delete must not reach into a world"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("every world using it"),
        "the reach of a shared removal must be stated"
    );
}

#[test]
fn delete_with_a_world_cannot_reach_a_structure_only_in_the_shared_copy() {
    let root = world_with_construct(&[("bomber", b"x")]);
    let shared = root
        .path()
        .join("development_behavior_packs/Construct[BP]/structures/bomber.mcstructure");
    assert!(shared.is_file());

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "bomber",
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
    assert!(shared.is_file(), "the shared copy must survive");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Test"),
        "the miss must name the world: {stderr}"
    );
}

#[test]
fn delete_refuses_the_pack_flag_and_the_shared_source_under_a_world() {
    let root = world_seeing_one_name_in_both_packs();

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
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

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "--world",
            "Test",
            "--source",
            "shared-pack",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--source shared-pack cannot be combined with --world"),
        "stderr:\n{stderr}"
    );
    assert!(shared_house(root.path()).is_file());
    assert!(world_house(root.path()).is_file());
}

#[test]
fn delete_removes_a_structure_from_a_world_database() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    close_world(&root.path().join("minecraftWorlds").join(world));

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "--world",
            world,
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
    assert!(String::from_utf8_lossy(&out.stdout).contains("from this world's database"));

    let listed = bin_isolated(root.path())
        .args([
            "structures",
            "--world",
            world,
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"house"), "house survived the delete: {v}");
    assert!(names.contains(&"barn"), "{v}");
    assert!(names.contains(&"understudy:players"), "{v}");
}

#[test]
fn delete_removes_the_database_copy_and_the_pack_copy_together() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);
    add_world_construct(&world_dir, &[("house", b"pack-copy")]);
    let pack_copy = world_dir.join("behavior_packs/Construct[BP]/structures/house.mcstructure");
    assert!(pack_copy.is_file());

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "--json",
            "--world",
            world,
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

    assert!(!pack_copy.exists(), "the pack copy survived");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = v["deleted"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "one row per copy removed: {v}");
    let sources: Vec<&str> = rows.iter().map(|r| r["source"].as_str().unwrap()).collect();
    assert!(sources.contains(&"world-db"), "{v}");
    assert!(sources.contains(&"world-pack"), "{v}");
}

#[test]
fn delete_source_world_leaves_the_pack_copy_alone() {
    let (root, world) = fixture_world_with_construct(&[], &[("house", b"pack-copy")]);
    close_world(&root.path().join("minecraftWorlds").join(world));
    let pack_copy = root
        .path()
        .join("development_behavior_packs/Construct[BP]/structures/house.mcstructure");

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "--source",
            "world-db",
            "--json",
            "--world",
            world,
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

    assert!(
        pack_copy.is_file(),
        "--source world-db must not touch a pack"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = v["deleted"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    assert_eq!(rows[0]["source"], "world-db");
}

#[test]
fn delete_source_pack_never_opens_the_world_database() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);
    add_world_construct(&world_dir, &[("tower", b"pack-copy")]);
    let before = db_fingerprint(&world_dir);

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "tower",
            "--source",
            "world-pack",
            "--world",
            world,
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
    assert_eq!(
        before,
        db_fingerprint(&world_dir),
        "the database was opened"
    );
}

#[test]
fn delete_of_a_batch_with_one_bad_name_removes_nothing_from_the_database() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "house",
            "nope",
            "--world",
            world,
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    let listed = bin_isolated(root.path())
        .args([
            "structures",
            "--world",
            world,
            "--json",
            "--source",
            "world-db",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"house"),
        "the named-first structure must survive a batch that could not resolve: {v}"
    );
}

#[test]
fn delete_removes_every_structure_named() {
    let root = world_with_construct(&[("barn", b"a"), ("silo", b"b"), ("hut", b"c")]);
    let dir = shared_structures(root.path());

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "barn",
            "silo",
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

    assert!(!dir.join("barn.mcstructure").exists());
    assert!(!dir.join("silo.mcstructure").exists());
    assert!(
        dir.join("hut.mcstructure").exists(),
        "a structure that was not named must survive"
    );
}

#[test]
fn delete_of_a_batch_with_one_bad_name_removes_nothing() {
    let root = world_with_construct(&[("barn", b"a"), ("silo", b"b")]);
    let dir = shared_structures(root.path());

    let out = bin_isolated(root.path())
        .args([
            "delete",
            "barn",
            "nosuchthing",
            "silo",
            "--source",
            "world-pack",
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    assert!(
        dir.join("barn.mcstructure").exists(),
        "nothing may be removed when any name in the batch fails to resolve"
    );
    assert!(dir.join("silo.mcstructure").exists());
}

#[test]
fn delete_json_carries_a_deleted_array_and_the_world() {
    let root = world_with_construct(&[("barn", b"a"), ("silo", b"b")]);
    let out = bin_isolated(root.path())
        .args([
            "delete",
            "barn",
            "silo",
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
    assert!(v["world"].is_null(), "{v}");
    let deleted = v["deleted"].as_array().unwrap();
    assert_eq!(deleted.len(), 2);
    assert_eq!(deleted[0]["name"], "barn");
    assert_eq!(deleted[0]["id"], "mystructure:barn");
    assert_eq!(deleted[0]["source"], "shared-pack");
    assert_eq!(deleted[1]["name"], "silo");
}

#[test]
fn delete_of_a_single_structure_still_emits_a_one_row_array() {
    let root = world_with_construct(&[("barn", b"a")]);
    let out = bin_isolated(root.path())
        .args([
            "delete",
            "barn",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["deleted"].as_array().unwrap().len(), 1);
    assert_eq!(v["deleted"][0]["name"], "barn");
}

#[test]
fn delete_with_no_structure_named_is_a_usage_error() {
    let root = world_with_construct(&[("barn", b"a")]);
    let out = bin_isolated(root.path())
        .args([
            "delete",
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
