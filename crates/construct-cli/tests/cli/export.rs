use crate::support::*;

#[test]
fn export_without_o_writes_the_derived_name_into_cwd() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", "--world", world.to_str().unwrap(), "house"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = dir.path().join("house.mcstructure");
    assert!(written.is_file());
    assert_eq!(std::fs::read(&written).unwrap()[0], 0x0a);
}

#[test]
fn export_with_o_uses_the_given_name() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("custom.mcstructure");
    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(target.is_file());
}

#[test]
fn export_refuses_an_existing_target_and_points_at_force() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("taken.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"existing",
        "must not have overwritten"
    );
}

#[test]
fn export_force_overwrites() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("taken.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            target.to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_ne!(std::fs::read(&target).unwrap(), b"existing");
}

#[test]
fn exporting_several_structures_writes_one_file_each() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "barn",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.path().join("house.mcstructure").is_file());
    assert!(dir.path().join("barn.mcstructure").is_file());
}

#[test]
fn several_structures_with_o_is_a_usage_error() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "barn",
            "-n",
            dir.path().join("x.mcstructure").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_multi_export_refuses_before_writing_anything_if_one_target_exists() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("barn.mcstructure"), b"existing").unwrap();

    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "barn",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !dir.path().join("house.mcstructure").exists(),
        "must write nothing on refusal"
    );
}

#[test]
fn exporting_a_missing_structure_reports_structure_not_found() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["export", "--world", world.to_str().unwrap(), "nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_traversal_structure_name_is_refused_and_writes_nothing() {
    let (_tmp, world) =
        fixture_world_with_extra_structures(&[("mystructure:../../../../tmp/PWNED", b"PWNEDBYT")]);
    let dir = tempfile::tempdir().unwrap();
    let escape_target = dir.path().join("../../../../tmp/PWNED.mcstructure");
    let _ = std::fs::remove_file(&escape_target);

    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "../../../../tmp/PWNED",
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("-n"), "should point at -n:\n{err}");
    assert!(
        !escape_target.exists(),
        "must not write outside the output directory"
    );
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "must write nothing at all, not even a partial file in cwd"
    );

    let _ = std::fs::remove_file(&escape_target);
}

#[test]
fn o_without_an_extension_gets_mcstructure() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            dir.path().join("castle").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(dir.path().join("castle.mcstructure").is_file());
    assert!(!dir.path().join("castle").exists());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("castle.mcstructure"), "stdout:\n{text}");
}

#[test]
fn o_with_another_extension_is_refused() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            dir.path().join("castle.txt").to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".txt"),
        "the error names what was given: {stderr}"
    );
    assert!(
        stderr.contains("castle.mcstructure"),
        "and what to write instead: {stderr}"
    );
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "a usage error writes nothing"
    );
}

#[test]
fn o_with_an_uppercase_extension_is_accepted_as_given() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            dir.path().join("CASTLE.MCSTRUCTURE").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.path().join("CASTLE.MCSTRUCTURE").is_file());
}

#[test]
fn a_merge_target_without_an_extension_gets_mcstructure_too() {
    let a = merge_fixture([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = merge_fixture([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a), ("tower", &b)]);
    let dir = tempfile::tempdir().unwrap();

    let out = bin()
        .args([
            "export",
            "north",
            "tower",
            "--merge",
            "-n",
            dir.path().join("both").to_str().unwrap(),
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
    assert!(dir.path().join("both.mcstructure").is_file());
}

#[test]
fn other_traversal_shapes_are_also_refused() {
    let (_tmp, world) = fixture_world_with_extra_structures(&[
        ("mystructure:a/../../b", b"AAAAAAAA"),
        ("mystructure:/etc/evil", b"BBBBBBBB"),
        ("mystructure:..", b"CCCCCCCC"),
    ]);
    for name in ["a/../../b", "/etc/evil", ".."] {
        let out = bin()
            .args(["export", "--world", world.to_str().unwrap(), name])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "name {name:?} should be refused"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("-n"),
            "name {name:?} error should point at -n"
        );
    }
}

#[test]
fn a_colon_bearing_name_sanitizes_and_exports() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "understudy:players",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = dir.path().join("understudy_players.mcstructure");
    assert!(
        written.is_file(),
        "expected the sanitized filename understudy_players.mcstructure"
    );
    assert_eq!(std::fs::read(&written).unwrap()[0], 0x0a);
}

#[test]
fn a_multi_export_with_one_hostile_name_writes_nothing() {
    let (_tmp, world) =
        fixture_world_with_extra_structures(&[("mystructure:../../../../tmp/PWNED2", b"DDDDDDDD")]);
    let dir = tempfile::tempdir().unwrap();
    let escape_target = dir.path().join("../../../../tmp/PWNED2.mcstructure");
    let _ = std::fs::remove_file(&escape_target);

    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "../../../../tmp/PWNED2",
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(
        !dir.path().join("house.mcstructure").exists(),
        "a hostile name later in the list must not leave an earlier structure written"
    );
    assert!(!escape_target.exists());

    let _ = std::fs::remove_file(&escape_target);
}

#[test]
fn a_windows_reserved_device_name_is_sanitized_not_refused() {
    let (_tmp, world) = fixture_world_with_extra_structures(&[
        ("mystructure:CON", b"AAAAAAAA"),
        ("mystructure:com1", b"BBBBBBBB"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", "--world", world.to_str().unwrap(), "CON", "com1"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.path().join("_CON.mcstructure").is_file(),
        "reserved device name CON should be prefixed, not refused"
    );
    assert!(
        dir.path().join("_com1.mcstructure").is_file(),
        "device names are reserved case-insensitively"
    );
}

#[test]
fn an_empty_derived_name_is_refused_and_points_at_o() {
    let (_tmp, world) = fixture_world_with_extra_structures(&[("mystructure:", b"AAAAAAAA")]);
    let dir = tempfile::tempdir().unwrap();

    let out = bin()
        .current_dir(dir.path())
        .args(["export", "--world", world.to_str().unwrap(), ""])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("-n"), "should point at -n:\n{err}");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "must write nothing"
    );
}

#[test]
fn explicit_o_path_bypasses_the_derived_name_rules() {
    let (_tmp, world) = fixture_world();
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let out = bin()
        .current_dir(&inner)
        .args([
            "export",
            "--world",
            world.to_str().unwrap(),
            "house",
            "-n",
            "../outside.mcstructure",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(outer.path().join("outside.mcstructure").is_file());
}

#[test]
fn export_of_a_name_in_both_world_and_pack_is_refused_naming_both_sources() {
    let (root, world_name) =
        fixture_world_with_construct(&[("mystructure:collide", b"WORLDBYTES")], &[]);
    add_world_construct(
        &root.path().join("minecraftWorlds").join(world_name),
        &[("collide", b"PACKBYTES!")],
    );
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "--world",
            world_name,
            "collide",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("world-db"),
        "should name the database source:\n{err}"
    );
    assert!(
        err.contains("world-pack"),
        "should name the pack source:\n{err}"
    );
    assert!(
        err.contains("--source world-db") && err.contains("--source world-pack"),
        "the hint must offer each matched place as a --source value:\n{err}"
    );
}

#[test]
fn export_without_a_world_writes_the_shared_copy() {
    let root = world_seeing_one_name_in_both_packs();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", "house", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dir.path().join("house.mcstructure")).unwrap(),
        b"shared-copy",
        "with no --world the shared copy is the one exported"
    );
}

#[test]
fn export_with_a_world_writes_that_worlds_copy_not_the_shared_one() {
    let root = world_seeing_one_name_in_both_packs();
    let dir = tempfile::tempdir().unwrap();
    let out = bin_isolated(root.path())
        .current_dir(dir.path())
        .args([
            "export",
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
    assert_eq!(
        std::fs::read(dir.path().join("house.mcstructure")).unwrap(),
        b"world-copy",
        "--world selects the world's own copy, unambiguously"
    );
}

#[test]
fn export_with_a_world_cannot_reach_a_structure_only_in_the_shared_copy() {
    let root = world_with_construct(&[("bomber", b"x")]);
    let dir = tempfile::tempdir().unwrap();
    let out = bin_isolated(root.path())
        .current_dir(dir.path())
        .args([
            "export",
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
    assert!(
        !dir.path().join("bomber.mcstructure").exists(),
        "a miss writes nothing"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Test"),
        "the miss must name the world: {stderr}"
    );
}

#[test]
fn export_refuses_the_pack_flag_and_the_shared_source_under_a_world() {
    let root = world_seeing_one_name_in_both_packs();
    let dir = tempfile::tempdir().unwrap();

    let out = bin()
        .current_dir(dir.path())
        .args([
            "export",
            "house",
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
        .current_dir(dir.path())
        .args([
            "export",
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
    assert!(!dir.path().join("house.mcstructure").exists());
}

#[test]
fn export_with_no_world_refuses_the_sources_that_need_one() {
    let root = world_with_construct(&[("bomber", b"x")]);
    let dir = tempfile::tempdir().unwrap();
    for source in ["world-db", "world-pack"] {
        let out = bin()
            .current_dir(dir.path())
            .args([
                "export",
                "bomber",
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
        assert!(!dir.path().join("bomber.mcstructure").exists());
    }
}
