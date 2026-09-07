use crate::support::*;

#[test]
fn import_derives_a_name_from_the_file_stem_and_reports_it() {
    let root = world_with_construct(&[]);
    let src = root.path().join("My House.mcstructure");
    std::fs::write(&src, b"structure-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            src.to_str().unwrap(),
            "--world",
            "Test",
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
    assert_eq!(v["written"][0]["name"], "My_House");
    assert_eq!(v["written"][0]["id"], "mystructure:My_House");
    let written = root.path().join(
        "minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/My_House.mcstructure",
    );
    assert_eq!(std::fs::read(&written).unwrap(), b"structure-bytes");
}

#[test]
fn import_accepts_a_name_with_capitals() {
    let root = world_with_construct(&[]);
    let src = root.path().join("counter.mcstructure");
    std::fs::write(&src, b"structure-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            src.to_str().unwrap(),
            "--name",
            "10HzCounter",
            "--world",
            "Test",
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
    assert_eq!(v["written"][0]["name"], "10HzCounter");
    let written = root
        .path()
        .join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/10HzCounter.mcstructure");
    assert_eq!(std::fs::read(&written).unwrap(), b"structure-bytes");
}

#[test]
fn import_refuses_an_unusable_name_instead_of_mangling_it() {
    let root = world_with_construct(&[]);
    let src = root.path().join("café.mcstructure");
    std::fs::write(&src, b"x").unwrap();

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
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--name"),
        "should point at --name:\n{stderr}"
    );
}

#[test]
fn import_refuses_to_overwrite_without_force() {
    let root = world_with_construct(&[]);
    let original = root.path().join("original.mcstructure");
    std::fs::write(&original, b"original").unwrap();
    assert!(
        bin()
            .args([
                "import",
                original.to_str().unwrap(),
                "--name",
                "house",
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

    let src = root.path().join("house.mcstructure");
    std::fs::write(&src, b"replacement").unwrap();
    let args = [
        "import",
        src.to_str().unwrap(),
        "--world",
        "Test",
        "--path",
        root.path().to_str().unwrap(),
    ];

    let out = bin().args(args).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    let target = root.path().join(
        "minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/house.mcstructure",
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"original");

    let out = bin().args(args).arg("--force").output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read(&target).unwrap(), b"replacement");
}

#[test]
fn import_says_when_it_wrote_into_the_shared_construct() {
    let root = world_with_construct(&[]);
    let src = root.path().join("tower.mcstructure");
    std::fs::write(&src, b"x").unwrap();

    let out = bin()
        .args([
            "import",
            src.to_str().unwrap(),
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("development_behavior_packs")
            && text.contains("shared copy of Construct")
            && text.contains("every world using it"),
        "stdout:\n{text}"
    );
    assert!(
        !text.contains("own copy"),
        "this world has no copy of its own: {text}"
    );
}

#[test]
fn import_says_when_it_wrote_into_a_worlds_own_construct() {
    let root = world_with_construct(&[]);
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
    let src = root.path().join("tower.mcstructure");
    std::fs::write(&src, b"x").unwrap();

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

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Test's own copy of Construct") && text.contains("that world only"),
        "stdout:\n{text}"
    );
    assert!(
        !text.contains("shared copy of Construct"),
        "the world's own copy won, so the shared wording must not appear: {text}"
    );
}

#[test]
fn import_says_the_world_must_be_reloaded() {
    let root = world_with_construct(&[]);
    let src = root.path().join("tower.mcstructure");
    std::fs::write(&src, b"x").unwrap();
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
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.to_lowercase().contains("reload"), "stdout:\n{text}");
}

#[test]
fn import_without_construct_points_at_install() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let src = root.path().join("x.mcstructure");
    std::fs::write(&src, b"x").unwrap();
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
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}

#[test]
fn a_second_import_uses_the_structures_pack_the_first_one_created() {
    let root = world_with_construct(&[]);
    let src = root.path().join("tower.mcstructure");
    std::fs::write(&src, b"x").unwrap();
    let import = |name: &str| {
        bin()
            .args([
                "import",
                src.to_str().unwrap(),
                "--name",
                name,
                "--world",
                "Test",
                "--path",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    assert!(import("first").status.success());

    let out = import("second");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Test's structures pack") && text.contains("that world only"),
        "stdout:\n{text}"
    );
    assert!(
        !text.contains("created"),
        "the pack already existed: {text}"
    );
    let structures = root
        .path()
        .join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures");
    assert!(structures.join("first.mcstructure").is_file());
    assert!(structures.join("second.mcstructure").is_file());
}

#[test]
fn import_takes_every_file_named() {
    let root = world_with_construct(&[]);
    let barn = root.path().join("barn.mcstructure");
    let silo = root.path().join("silo.mcstructure");
    std::fs::write(&barn, b"barn-bytes").unwrap();
    std::fs::write(&silo, b"silo-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            barn.to_str().unwrap(),
            silo.to_str().unwrap(),
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

    assert_eq!(
        std::fs::read(imported_into_test(root.path(), "barn")).unwrap(),
        b"barn-bytes"
    );
    assert_eq!(
        std::fs::read(imported_into_test(root.path(), "silo")).unwrap(),
        b"silo-bytes"
    );
}

#[test]
fn import_of_a_batch_with_one_unreadable_file_writes_nothing() {
    let root = world_with_construct(&[]);
    let barn = root.path().join("barn.mcstructure");
    std::fs::write(&barn, b"barn-bytes").unwrap();
    let missing = root.path().join("nosuchfile.mcstructure");

    let out = bin()
        .args([
            "import",
            barn.to_str().unwrap(),
            missing.to_str().unwrap(),
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    assert!(
        !imported_into_test(root.path(), "barn").exists(),
        "nothing may be written when any file in the batch cannot be read"
    );
}

#[test]
fn import_refuses_two_files_that_would_derive_one_name() {
    let root = world_with_construct(&[]);
    let a = root.path().join("a");
    let b = root.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("house.mcstructure"), b"first").unwrap();
    std::fs::write(b.join("house.mcstructure"), b"second").unwrap();

    let out = bin()
        .args([
            "import",
            a.join("house.mcstructure").to_str().unwrap(),
            b.join("house.mcstructure").to_str().unwrap(),
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("house"),
        "must name the collision:\n{stderr}"
    );
    assert!(
        !stderr.contains("unexpected argument"),
        "must be refused for the name collision, not for taking two files:\n{stderr}"
    );

    assert!(
        !imported_into_test(root.path(), "house").exists(),
        "nothing may be written when two files claim one name"
    );
}

#[test]
fn import_name_with_more_than_one_file_is_a_usage_error() {
    let root = world_with_construct(&[]);
    let barn = root.path().join("barn.mcstructure");
    let silo = root.path().join("silo.mcstructure");
    std::fs::write(&barn, b"a").unwrap();
    std::fs::write(&silo, b"b").unwrap();

    let out = bin()
        .args([
            "import",
            barn.to_str().unwrap(),
            silo.to_str().unwrap(),
            "--name",
            "whatever",
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--name") && !stderr.contains("unexpected argument"),
        "must be refused for --name, not for taking two files:\n{stderr}"
    );

    assert!(!imported_into_test(root.path(), "whatever").exists());
    assert!(!imported_into_test(root.path(), "barn").exists());
}

#[test]
fn import_json_carries_a_written_array_with_pack_and_scope_at_the_top() {
    let root = world_with_construct(&[]);
    let barn = root.path().join("barn.mcstructure");
    let silo = root.path().join("silo.mcstructure");
    std::fs::write(&barn, b"barn-bytes").unwrap();
    std::fs::write(&silo, b"silo-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            barn.to_str().unwrap(),
            silo.to_str().unwrap(),
            "--world",
            "Test",
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
    assert_eq!(v["target"], "world-pack");
    assert!(v["pack"].as_str().unwrap().contains("ConstructStructures"));

    let written = v["written"].as_array().unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0]["name"], "barn");
    assert_eq!(written[0]["id"], "mystructure:barn");
    assert_eq!(written[0]["bytes"], 10);
    assert_eq!(written[1]["name"], "silo");
}

#[test]
fn import_of_a_single_file_still_emits_a_one_row_array() {
    let root = world_with_construct(&[]);
    let barn = root.path().join("barn.mcstructure");
    std::fs::write(&barn, b"barn-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            barn.to_str().unwrap(),
            "--world",
            "Test",
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
fn import_with_no_file_named_is_a_usage_error() {
    let root = world_with_construct(&[]);
    let out = bin()
        .args([
            "import",
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn import_of_a_directory_mirrors_its_tree_under_the_named_folder() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("CF-Q1.mcstructure"), b"q1-bytes").unwrap();
    std::fs::write(src.join("sub/tower.mcstructure"), b"tower-bytes").unwrap();

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

    assert_eq!(
        std::fs::read(imported_tree_path(root.path(), "Amelix/CF-Q1.mcstructure")).unwrap(),
        b"q1-bytes"
    );
    assert_eq!(
        std::fs::read(imported_tree_path(
            root.path(),
            "Amelix/sub/tower.mcstructure"
        ))
        .unwrap(),
        b"tower-bytes"
    );
}

#[test]
fn import_of_a_directory_reports_the_namespaced_ids() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("CF-Q1.mcstructure"), b"q1").unwrap();
    std::fs::write(src.join("sub/tower.mcstructure"), b"t").unwrap();

    let out = bin()
        .args([
            "--json",
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<&str> = v["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["Amelix:CF-Q1", "Amelix:sub/tower"]);
}

#[test]
fn import_of_a_directory_skips_files_that_are_not_structures() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("barn.mcstructure"), b"barn").unwrap();
    std::fs::write(src.join("README.md"), b"notes").unwrap();
    std::fs::write(src.join(".DS_Store"), b"junk").unwrap();

    let out = bin()
        .args([
            "--json",
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<&str> = v["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["Amelix:barn"]);
}

#[test]
fn import_of_a_directory_with_no_structures_in_it_fails() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Empty");
    std::fs::create_dir_all(src.join("deeper")).unwrap();
    std::fs::write(src.join("README.md"), b"notes").unwrap();

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
        !out.status.success(),
        "an empty folder must not report success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Empty"),
        "must name the folder that held nothing:\n{stderr}"
    );
}

#[test]
fn import_name_with_a_directory_is_a_usage_error() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("barn.mcstructure"), b"barn").unwrap();

    let out = bin()
        .args([
            "import",
            src.to_str().unwrap(),
            "--name",
            "renamed",
            "--world",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--name"),
        "must say which flag is the problem:\n{stderr}"
    );
    assert!(
        !imported_tree_path(root.path(), "Amelix/barn.mcstructure").exists(),
        "a usage error must attempt nothing"
    );
}

#[test]
fn import_takes_files_and_directories_in_one_batch() {
    let root = world_with_construct(&[]);
    let loose = root.path().join("silo.mcstructure");
    std::fs::write(&loose, b"silo-bytes").unwrap();
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("barn.mcstructure"), b"barn-bytes").unwrap();

    let out = bin()
        .args([
            "import",
            loose.to_str().unwrap(),
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

    assert_eq!(
        std::fs::read(imported_tree_path(root.path(), "silo.mcstructure")).unwrap(),
        b"silo-bytes"
    );
    assert_eq!(
        std::fs::read(imported_tree_path(root.path(), "Amelix/barn.mcstructure")).unwrap(),
        b"barn-bytes"
    );
}

#[test]
fn import_of_a_directory_writes_nothing_when_one_file_already_exists() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("barn.mcstructure"), b"barn-bytes").unwrap();

    let args = |root: &std::path::Path, src: &std::path::Path| {
        vec![
            "import".to_string(),
            src.to_str().unwrap().to_string(),
            "--world".to_string(),
            "Test".to_string(),
            "--path".to_string(),
            root.to_str().unwrap().to_string(),
        ]
    };
    assert!(
        bin()
            .args(args(root.path(), &src))
            .output()
            .unwrap()
            .status
            .success()
    );

    std::fs::write(src.join("silo.mcstructure"), b"silo-bytes").unwrap();
    let out = bin().args(args(root.path(), &src)).output().unwrap();
    assert!(
        !out.status.success(),
        "the existing barn must refuse the batch"
    );
    assert!(
        !imported_tree_path(root.path(), "Amelix/silo.mcstructure").exists(),
        "no file may land when any file in the folder collides"
    );
}

#[test]
fn import_of_a_directory_derives_a_usable_name_for_each_folder() {
    let root = world_with_construct(&[]);
    let src = root.path().join("My Builds");
    std::fs::create_dir_all(src.join("tall towers")).unwrap();
    std::fs::write(src.join("tall towers/big one.mcstructure"), b"x").unwrap();

    let out = bin()
        .args([
            "--json",
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["written"][0]["id"], "My_Builds:tall_towers/big_one");
    assert!(imported_tree_path(root.path(), "My_Builds/tall_towers/big_one.mcstructure").exists());
}

#[test]
fn import_of_a_directory_warns_once_about_the_namespace_not_once_per_file() {
    let root = world_with_construct(&[]);
    let src = root.path().join("Amelix");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("barn.mcstructure"), b"a").unwrap();
    std::fs::write(src.join("silo.mcstructure"), b"b").unwrap();
    std::fs::write(src.join("sub/tower.mcstructure"), b"c").unwrap();

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
    let warnings = stderr
        .lines()
        .filter(|l| l.contains("mystructure namespace"))
        .count();
    assert_eq!(warnings, 1, "one warning per namespace:\n{stderr}");
    assert!(
        stderr.contains("Amelix"),
        "the warning must name the namespace:\n{stderr}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.trim_start().starts_with("into "))
            .count(),
        1,
        "the destination pack is one answer for the batch:\n{stdout}"
    );
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("imported "))
            .count(),
        3,
        "{stdout}"
    );
}
