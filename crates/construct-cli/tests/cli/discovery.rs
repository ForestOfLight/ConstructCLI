use crate::support::*;

#[test]
fn add_classifies_supported_paths_and_rejects_other_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.toml");
    let com_mojang = tmp.path().join("games/com.mojang");
    let world = tmp.path().join("world");
    let structures = tmp.path().join("structures");
    std::fs::create_dir_all(com_mojang.join("minecraftWorlds")).unwrap();
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"stub").unwrap();
    std::fs::create_dir_all(&structures).unwrap();

    for path in [&com_mojang, &world] {
        let out = bin_with_config(&config)
            .args(["add", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let settings: toml::Value = toml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        settings["roots"][0]["path"].as_str(),
        com_mojang.canonicalize().unwrap().to_str()
    );
    assert_eq!(
        settings["other_worlds"][0].as_str(),
        world.canonicalize().unwrap().to_str()
    );

    let out = bin_with_config(&config)
        .args(["add", structures.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("expected a com.mojang directory or a world directory")
    );
}

#[test]
fn a_path_referenced_world_works_with_no_installations_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("SomeWorld");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let out = bin()
        .args(["structures", "--world", world.to_str().unwrap()])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("no Minecraft installation found"),
        "installation check preempted a path reference:\n{err}"
    );
}

#[test]
fn no_installation_found_reports_no_installations_and_lists_probed_paths() {
    let out = bin()
        .args(["worlds", "--path", "/nonexistent"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("/nonexistent"),
        "should name what it probed:\n{err}"
    );
}

#[test]
fn path_lists_a_bare_world_folder_under_the_path_installation() {
    let tmp = tempfile::tempdir().unwrap();
    let world = bare_world(tmp.path(), "Standalone", "My Save");

    let out = bin()
        .args(["worlds", "--json", "--path", world.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let worlds = listed_worlds(&out);
    assert_eq!(worlds.len(), 1, "{worlds:?}");
    assert_eq!(worlds[0]["installation"], "path");
    assert_eq!(worlds[0]["folder"], "Standalone");
    assert_eq!(worlds[0]["display_name"], "My Save");
    assert_eq!(worlds[0]["qualified"], "path/Standalone");
}

#[test]
fn path_takes_a_com_mojang_root_and_a_world_folder_in_one_invocation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("games/com.mojang");
    bare_world(&root.join("minecraftWorlds"), "InRoot", "In Root");
    let loose = bare_world(tmp.path(), "Loose", "Loose");

    let out = bin()
        .args([
            "worlds",
            "--json",
            "--path",
            root.to_str().unwrap(),
            "--path",
            loose.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let worlds = listed_worlds(&out);
    let mut qualified: Vec<&str> = worlds
        .iter()
        .map(|w| w["qualified"].as_str().unwrap())
        .collect();
    qualified.sort_unstable();
    assert_eq!(qualified, ["flag1/InRoot", "path/Loose"]);
}

#[test]
fn a_world_under_a_given_root_is_not_listed_twice_when_also_named_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("games/com.mojang");
    let world = bare_world(&root.join("minecraftWorlds"), "Both", "Both");

    let out = bin()
        .args([
            "worlds",
            "--json",
            "--path",
            root.to_str().unwrap(),
            "--path",
            world.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let worlds = listed_worlds(&out);
    assert_eq!(worlds.len(), 1, "{worlds:?}");
    assert_eq!(worlds[0]["qualified"], "flag1/Both");
}

#[test]
fn a_world_folder_given_by_path_is_then_addressable_by_name() {
    let (tmp, world) = fixture_world();
    let out = bin_isolated(tmp.path())
        .args([
            "structures",
            "--world",
            "test_level",
            "--path",
            world.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"house"), "{names:?}");
}

#[test]
fn a_malformed_reference_is_exit_2_even_with_no_installations() {
    let out = bin()
        .args(["structures", "--world", "a/b/c/d/e/f/g"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("malformed") || err.contains("Expected a world name"),
        "should mention a malformed reference, not a missing installation:\n{err}"
    );
    assert!(
        !err.contains("no Minecraft installation found"),
        "must not be masked as NoInstallations:\n{err}"
    );
}

#[test]
fn a_path_looking_reference_that_does_not_exist_is_exit_2_with_no_installations() {
    let out = bin()
        .args(["structures", "--world", "/nonexistent/deep/path"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("looks like a filesystem path"),
        "should say it looks like a path:\n{err}"
    );
    assert!(
        !err.contains("no Minecraft installation found"),
        "must not be masked as NoInstallations:\n{err}"
    );
}

#[test]
fn a_bare_name_with_no_installations_is_genuinely_no_installations() {
    let out = bin()
        .args(["structures", "--world", "somename"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no Minecraft installation found"),
        "a genuinely-missing world with zero installations should say so:\n{err}"
    );
}

#[test]
#[cfg(unix)]
fn an_unreadable_world_is_not_masked_as_no_installations() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("SomeWorld");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let level_dat = world.join("level.dat");

    std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o000)).unwrap();

    if std::fs::read(&level_dat).is_ok() {
        std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o644)).ok();
        return;
    }

    let out = bin()
        .args(["structures", "--world", world.to_str().unwrap()])
        .output()
        .unwrap();

    std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o644)).ok();

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("could not read"),
        "should report the unreadable-world message:\n{err}"
    );
    assert!(
        !err.contains("no Minecraft installation found"),
        "must not be masked as NoInstallations:\n{err}"
    );
}

#[test]
fn a_world_is_addressable_by_a_level_name_containing_slashes() {
    let (tmp, world) = fixture_world();
    let level_name = "Advanced Automation 10/13/21 23:33:18";
    std::fs::write(world.join("levelname.txt"), level_name).unwrap();

    let out = bin_isolated(tmp.path())
        .args([
            "structures",
            "--world",
            level_name,
            "--path",
            world.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "a level name with slashes should resolve; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = v["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"house"), "{names:?}");
}

#[test]
fn the_folder_name_still_resolves_a_world_with_a_slashed_level_name() {
    let (tmp, world) = fixture_world();
    std::fs::write(
        world.join("levelname.txt"),
        "Advanced Automation 10/13/21 23:33:18",
    )
    .unwrap();

    let out = bin_isolated(tmp.path())
        .args([
            "structures",
            "--world",
            "test_level",
            "--path",
            world.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
