use std::process::Command;

/// A binary invocation with a pinned, empty environment, so discovery finds
/// exactly what the test puts there and nothing from the host machine.
fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_construct"));
    let empty = std::env::temp_dir().join("construct-empty-home");
    std::fs::create_dir_all(&empty).unwrap();
    c.env("HOME", &empty).env("USERPROFILE", &empty);
    c.env_remove("APPDATA").env_remove("LOCALAPPDATA");
    c.env_remove("CONSTRUCT_COM_MOJANG")
        .env_remove("CONSTRUCT_INSTALLATION");
    c.env("CONSTRUCT_CONFIG", empty.join("no-such-config.toml"));
    c
}

#[test]
fn help_lists_the_read_commands() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in ["worlds", "list", "export"] {
        assert!(text.contains(cmd), "--help should mention {cmd}:\n{text}");
    }
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let out = bin().arg("nonsense").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn worlds_json_is_exactly_one_document_on_stdout() {
    // The GUI contract: stdout parses as JSON with no scanning.
    // Points at a real (empty) com.mojang so an installation exists and a
    // payload is emitted — the exit-3 case is a different test.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args([
            "worlds",
            "--json",
            "--com-mojang",
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
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("stdout was not one JSON doc: {e}\n{text}"));
    assert_eq!(parsed["schema"], 1);
    assert!(parsed["worlds"].is_array());
    assert!(parsed["warnings"].is_array());
}

#[test]
fn warnings_go_to_stderr_and_never_pollute_json_stdout() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args([
            "worlds",
            "--json",
            "--com-mojang",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    // Whatever is on stderr, stdout must still parse.
    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}

#[test]
fn a_path_referenced_world_works_with_no_installations_at_all() {
    // §6 promises a filesystem path is a valid world reference. That must not
    // depend on a Minecraft install existing — CI runners have none.
    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("SomeWorld");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let out = bin()
        .args(["list", world.to_str().unwrap()])
        .output()
        .unwrap();
    // The db is not a real leveldb, so this fails — but it must NOT fail with
    // exit 3 "no installation found", which would mean the check preempted
    // resolution.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("no Minecraft installation found"),
        "installation check preempted a path reference:\n{err}"
    );
}

#[test]
fn no_installation_found_exits_3_and_lists_probed_paths() {
    let out = bin()
        .args(["worlds", "--com-mojang", "/nonexistent"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("/nonexistent"),
        "should name what it probed:\n{err}"
    );
}

#[test]
fn a_malformed_reference_is_exit_2_even_with_no_installations() {
    // The resolve_world closure must not rewrite a MalformedReference into
    // NoInstallations just because installations.is_empty() — the input was
    // ill-formed regardless of how many installations exist.
    let out = bin().args(["list", "a/b/c/d/e/f/g"]).output().unwrap();
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
        .args(["list", "/nonexistent/deep/path"])
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
fn a_bare_name_with_no_installations_is_genuinely_exit_3() {
    // This one IS the right explanation: nothing was found, and there was
    // nothing to search. Asserted with the message too, so a future
    // over-correction that removes the masking entirely gets caught.
    let out = bin().args(["list", "somename"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no Minecraft installation found"),
        "a genuinely-missing world with zero installations should say so:\n{err}"
    );
}

#[test]
#[cfg(unix)]
fn an_unreadable_world_is_exit_1_not_3_with_no_installations() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("SomeWorld");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let level_dat = world.join("level.dat");

    std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o000)).unwrap();

    // Verify the test setup: level.dat should not be readable. Skip if
    // permissions cannot be enforced (e.g. running as root).
    if std::fs::read(&level_dat).is_ok() {
        std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o644)).ok();
        return;
    }

    let out = bin()
        .args(["list", world.to_str().unwrap()])
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

/// Extract the shared fixture world and return its directory.
fn fixture_world() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let gz = std::fs::File::open("../construct-core/tests/fixtures/world.tar.gz")
        .expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(tmp.path())
        .unwrap();
    let world = tmp.path().join("test_level");
    (tmp, world)
}

#[test]
fn list_by_path_shows_structures_with_their_source() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["list", world.to_str().unwrap()])
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
        text.contains("world"),
        "source column should say world:\n{text}"
    );
}

#[test]
fn list_json_carries_schema_and_entries() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["list", world.to_str().unwrap(), "--json"])
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
fn list_source_pack_is_empty_in_stage_one() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args([
            "list",
            world.to_str().unwrap(),
            "--source",
            "pack",
            "--json",
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["structures"].as_array().unwrap().len(), 0);
}

#[test]
fn list_of_a_missing_world_exits_3() {
    let out = bin()
        .args(["list", "definitely-not-a-world"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn export_without_o_writes_the_derived_name_into_cwd() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "house"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = dir.path().join("house.mcstructure");
    assert!(written.is_file());
    // Byte transparency: the file is the database value, untouched.
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
            world.to_str().unwrap(),
            "house",
            "-o",
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
            world.to_str().unwrap(),
            "house",
            "-o",
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
            world.to_str().unwrap(),
            "house",
            "-o",
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
        .args(["export", world.to_str().unwrap(), "house", "barn"])
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
    // -o names a single file; it cannot name several.
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export",
            world.to_str().unwrap(),
            "house",
            "barn",
            "-o",
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
        .args(["export", world.to_str().unwrap(), "house", "barn"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !dir.path().join("house.mcstructure").exists(),
        "must write nothing on refusal"
    );
}

#[test]
fn exporting_a_missing_structure_exits_3() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["export", world.to_str().unwrap(), "nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
}

/// Extracts the fixture world, then inserts additional structure keys
/// directly through the leveldb API, bypassing any validation a normal
/// `/structure` save would go through. Structure names in a world's
/// database are not guaranteed to have been authored by the person running
/// `construct` against it — that's exactly what these tests exercise.
fn fixture_world_with_extra_structures(
    extra: &[(&str, &[u8])],
) -> (tempfile::TempDir, std::path::PathBuf) {
    let (tmp, world) = fixture_world();
    {
        let db_dir = world.join("db");
        let db = bedrock_level::db::Database::open(db_dir.to_str().unwrap()).unwrap();
        for (name, bytes) in extra {
            let key = construct_core::store::key::encode(name);
            db.insert(&key, *bytes).unwrap();
        }
        // `db` drops here, releasing leveldb's LOCK before the CLI subprocess
        // opens the same database.
    }
    (tmp, world)
}

#[test]
fn a_traversal_structure_name_is_refused_and_writes_nothing() {
    // Exact reproduction of the demonstrated exploit: a structure named
    // `../../../../tmp/PWNED` in the default (`mystructure:`) namespace used
    // to write a file outside the current directory.
    let (_tmp, world) =
        fixture_world_with_extra_structures(&[("mystructure:../../../../tmp/PWNED", b"PWNEDBYT")]);
    let dir = tempfile::tempdir().unwrap();
    let escape_target = dir.path().join("../../../../tmp/PWNED.mcstructure");
    let _ = std::fs::remove_file(&escape_target);

    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "../../../../tmp/PWNED"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("-o"), "should point at -o:\n{err}");
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
fn other_traversal_shapes_are_also_refused() {
    let (_tmp, world) = fixture_world_with_extra_structures(&[
        ("mystructure:a/../../b", b"AAAAAAAA"),
        ("mystructure:/etc/evil", b"BBBBBBBB"),
        ("mystructure:..", b"CCCCCCCC"),
    ]);
    for name in ["a/../../b", "/etc/evil", ".."] {
        let out = bin()
            .args(["export", world.to_str().unwrap(), name])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "name {name:?} should be refused"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("-o"),
            "name {name:?} error should point at -o"
        );
    }
}

#[test]
fn a_colon_bearing_name_sanitizes_and_exports() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "understudy:players"])
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
    // Byte transparency still holds after sanitizing the filename.
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
    // Unlike traversal, a device-name collision is a portability problem, not
    // a security boundary: the export must still succeed, just under a safe
    // filename.
    let (_tmp, world) = fixture_world_with_extra_structures(&[
        ("mystructure:CON", b"AAAAAAAA"),
        ("mystructure:com1", b"BBBBBBBB"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "CON", "com1"])
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
    // A key of exactly `structuretemplate_mystructure:` decodes to an empty
    // display name, which would derive the filename `.mcstructure` — a
    // hidden file with no name.
    let (_tmp, world) = fixture_world_with_extra_structures(&[("mystructure:", b"AAAAAAAA")]);
    let dir = tempfile::tempdir().unwrap();

    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), ""])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("-o"), "should point at -o:\n{err}");
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
            world.to_str().unwrap(),
            "house",
            "-o",
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
