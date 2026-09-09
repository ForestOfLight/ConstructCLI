use std::process::Command;

/// A binary invocation with a pinned, empty environment, so discovery finds
/// exactly what the test puts there and nothing from the host machine.
fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_construct"));
    let empty = std::env::temp_dir().join("construct-empty-home");
    std::fs::create_dir_all(&empty).unwrap();
    c.env("HOME", &empty).env("USERPROFILE", &empty);
    c.env_remove("APPDATA").env_remove("LOCALAPPDATA");
    // With `HOME`/`USERPROFILE` pointed at a scratch directory there is no
    // platform data directory to fall back on. On Unix one is still derived
    // from the environment, but on Windows the lookup goes to the real known
    // folder and fails, so anything that takes a backup — `install` enabling
    // Beta APIs, say — failed there and only there. Name the directory instead
    // of relying on the fallback. Tests that care where backups land set
    // `[backups] dir`, which still wins over this.
    c.env("CONSTRUCT_DATA_DIR", &empty);
    c.env("CONSTRUCT_CONFIG", empty.join("no-such-config.toml"));
    c
}

/// `bin()` with a data directory of its own, so its writemarks are its own.
///
/// Every CLI test shares one temp `HOME`, and `writemark` keys a world on its
/// qualified reference — which for the real-leveldb fixture is `flag1/test_level`
/// in *every* test that uses it. Running in parallel they overwrite each other's
/// marks, and a test that depends on its own mark surviving then falls through
/// to the full in-use watch and blows its timing assertion. One data directory
/// per test root removes the sharing.
fn bin_isolated(root: &std::path::Path) -> Command {
    let mut c = bin();
    c.env("CONSTRUCT_DATA_DIR", root.join("state"));
    c
}

fn bin_with_config(config: &std::path::Path) -> Command {
    let mut command = bin();
    command.env("CONSTRUCT_CONFIG", config);
    command
}

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
fn help_lists_the_read_commands() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in ["worlds", "structures", "export"] {
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
    // payload is emitted — the no-installations case is a different test.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args(["worlds", "--json", "--path", root.path().to_str().unwrap()])
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
fn a_failure_under_json_emits_one_document_carrying_error_kind() {
    // The contract that replaces exit codes 3, 4 and 5. The code says only
    // that it failed; `error.kind` says what failed, and is the only place
    // that information exists now.
    let out = bin()
        .args(["worlds", "--json", "--path", "/nonexistent"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("a failure must still be one JSON doc: {e}\n{text}"));
    assert_eq!(v["error"]["kind"], "no-installations");
    assert!(
        v["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("no Minecraft installation found")),
        "the error must carry its human message too: {v}"
    );
    // Same envelope as a success payload — a caller parses one shape.
    assert_eq!(v["schema"], 1);
    assert!(v["warnings"].is_array());
}

#[test]
fn a_failure_without_json_still_prints_nothing_on_stdout() {
    // The error document is a `--json` feature. Plain output keeps stdout for
    // results and puts the reason on stderr, exactly as before.
    let out = bin()
        .args(["worlds", "--path", "/nonexistent"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "stdout should be empty without --json, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("error:"));
}

#[test]
fn a_usage_error_from_a_core_error_still_carries_its_kind() {
    // Exit 2 survives the retirement, and a CoreError that exits 2 emits the
    // document like any other — a caller can tell "you were ambiguous" from
    // "you mistyped a flag", which is what exit-3-vs-2 used to encode.
    let out = bin()
        .args(["structures", "--world", "a/b/c/d/e/f/g", "--json"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "malformed-reference");
}

#[test]
fn a_hand_rolled_usage_error_prints_no_json() {
    // Deliberately excluded: the hand-rolled checks in main.rs and clap's own
    // errors are grammar mistakes, not results. Exit 2 already says "fix the
    // input", so they stay stderr-only. Pinned so the exclusion is a decision
    // rather than an oversight.
    let out = bin()
        .args(["export", "a", "b", "--merge", "--json"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stdout.is_empty(),
        "stdout should be empty, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn a_clap_usage_error_prints_no_json() {
    let out = bin().args(["--json", "nonsense"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn a_missing_world_reports_world_not_found_under_json() {
    // The other half of the retired exit 3: with an installation present,
    // a missing world is `world-not-found` rather than `no-installations`.
    // One number covered both; two kinds tell them apart.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args([
            "structures",
            "--world",
            "NoSuchWorld",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "world-not-found");
}

#[test]
fn warnings_go_to_stderr_and_never_pollute_json_stdout() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args(["worlds", "--json", "--path", root.path().to_str().unwrap()])
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
        .args(["structures", "--world", world.to_str().unwrap()])
        .output()
        .unwrap();
    // The db is not a real leveldb, so this fails — but it must NOT fail with
    // "no Minecraft installation found", which would mean the check preempted
    // resolution.
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

/// A world folder with a fake `level.dat` — enough for enumeration, which only
/// needs the file to exist. Tests that actually open the database use
/// `fixture_world`.
fn bare_world(parent: &std::path::Path, folder: &str, name: &str) -> std::path::PathBuf {
    let dir = parent.join(folder);
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(dir.join("level.dat"), b"x").unwrap();
    std::fs::write(dir.join("levelname.txt"), name).unwrap();
    dir
}

fn listed_worlds(out: &std::process::Output) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not JSON: {e}\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    v["worlds"].as_array().cloned().unwrap_or_default()
}

#[test]
fn path_lists_a_bare_world_folder_under_the_path_installation() {
    // A save folder outside any com.mojang is a first-class world: `--path`
    // takes it directly rather than requiring a root that contains it.
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
    // The whole point of the rename: one flag, either kind of directory, mixed
    // freely and repeated.
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
    // Naming both the root and one of its worlds is an easy thing to type. The
    // world keeps the identity discovery gave it — `flag1/`, not `path/`.
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
    // Folding the world into discovery is what makes this work: the reference
    // `test_level` is a folder name, resolved against the discovered set, not a
    // filesystem path.
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
    // The resolve_world closure must not rewrite a MalformedReference into
    // NoInstallations just because installations.is_empty() — the input was
    // ill-formed regardless of how many installations exist.
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
    // This one IS the right explanation: nothing was found, and there was
    // nothing to search. The exit code cannot say which of the two it was any
    // more — both are 1 — so the message is what discriminates, and asserting
    // it catches a future over-correction that removes the masking entirely.
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
    // Both failures exit 1, so the code proves nothing here. The stderr
    // assertions below are the discriminator; `error.kind` is the machine-
    // readable equivalent, pinned in the json contract tests.
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
fn a_world_is_addressable_by_a_level_name_containing_slashes() {
    // Minecraft's default level name embeds a date — "Advanced Automation
    // 10/13/21 23:33:18". Those slashes are part of the name, not the
    // `<installation>/<account>/<world>` separators, so the name must resolve
    // rather than being read as segments or rejected as malformed.
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
    // The reported bug had the folder name working and the level name failing.
    // Fixing the level name must not cost the folder name.
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
    // Superseded by stage 2: `--source world-pack` used to just filter to an empty
    // list, since nothing populated Source::Pack yet. Now that it does, an
    // explicit `--source world-pack` with no reachable Construct is the thing the
    // brief calls out explicitly: "then the user asked for exactly the thing
    // that is not there" — so it errors rather than silently returning empty.
    // (This world is path-referenced and no `--path` is given, so there
    // is no installation to search either — a stronger case for "not there"
    // than a merely-uninstalled Construct.)
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
    // The NAME column in `structures` is the identifier the user types into
    // `export` (unlike `worlds`, which has a separate untruncated REFERENCE
    // column). A truncated name here would hand back something that no
    // longer resolves. 25 chars, over the 24-wide column, so this fails if
    // truncation is reintroduced.
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
    // -n names a single file; it cannot name several.
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
        let db = construct_core::store::bedrock::retry_past_transient_locks(|| {
            bedrock_level::db::Database::open(db_dir.to_str().unwrap())
        })
        .unwrap();
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
    // `-n castle` is unambiguous, and the only file Minecraft loads is a
    // `.mcstructure`, so the extension is completed rather than demanded.
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
    // The path printed is the one written, not the one asked for.
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("castle.mcstructure"), "stdout:\n{text}");
}

#[test]
fn o_with_another_extension_is_refused() {
    // The bytes would be right and the file would be one the game never
    // offers to load. Usage error: nothing is written.
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
    // The filesystems this runs on are case-insensitive; refusing
    // `CASTLE.MCSTRUCTURE` would refuse a name that already works.
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
    // `--merge` writes its target through a different branch of the command,
    // so it gets its own check that the extension rule reached it.
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
    // A key of exactly `structuretemplate_mystructure:` decodes to an empty
    // display name, which would derive the filename `.mcstructure` — a
    // hidden file with no name.
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

/// A com.mojang tree with one world and, optionally, Construct installed.
fn world_with_construct(structures: &[(&str, &[u8])]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    // Needed for `discovery::enumerate` to see this as a world at all — it
    // requires `level.dat` to exist, which the brief's snippet omitted.
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    std::fs::create_dir_all(world.join("db")).unwrap();

    let bp = root.path().join("development_behavior_packs/Construct[BP]");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();
    for (name, bytes) in structures {
        let p = bp.join("structures").join(format!("{name}.mcstructure"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    root
}

#[test]
fn structures_shows_pack_structures_without_touching_the_world_database() {
    // This fixture's world has no pack of its own, so the shared copy of
    // Construct is what serves it — and `--source shared-pack` is legal under
    // `--world` precisely because a world listing does show those rows. Only
    // `export` and `delete` refuse the pair, since for them `--world` is the
    // grammar's way of staying out of the shared copy.
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
    // Needed for `discovery::enumerate` to see this as a world at all.
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

/// Like `fixture_world_with_extra_structures`, but extracts the real-leveldb
/// fixture world under a `--path` root's `minecraftWorlds/`, and
/// installs Construct beside it. This is what `structures` needs to see a name
/// collision between sources: `discovery::installation::for_world` looks an
/// installation up by name, and a bare-path reference (as used by
/// `fixture_world`) always carries the synthetic installation name `"path"`,
/// which never matches a real installation — so a path-referenced world can
/// never reach `pack::for_world`. Routing through `--path` instead gives
/// the world a real installation name and makes the pack reachable.
fn fixture_world_with_construct(
    extra_world_structures: &[(&str, &[u8])],
    pack_structures: &[(&str, &[u8])],
) -> (tempfile::TempDir, &'static str) {
    let root = tempfile::tempdir().unwrap();
    let worlds_dir = root.path().join("minecraftWorlds");
    std::fs::create_dir_all(&worlds_dir).unwrap();

    let gz = std::fs::File::open("../construct-core/tests/fixtures/world.tar.gz")
        .expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(&worlds_dir)
        .unwrap();
    let world = worlds_dir.join("test_level");

    {
        let db_dir = world.join("db");
        let db = construct_core::store::bedrock::retry_past_transient_locks(|| {
            bedrock_level::db::Database::open(db_dir.to_str().unwrap())
        })
        .unwrap();
        for (name, bytes) in extra_world_structures {
            let key = construct_core::store::key::encode(name);
            db.insert(&key, *bytes).unwrap();
        }
        // `db` drops here, releasing leveldb's LOCK before the CLI subprocess
        // opens the same database.
    }

    let bp = root.path().join("development_behavior_packs/Construct[BP]");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();
    for (name, bytes) in pack_structures {
        let p = bp.join("structures").join(format!("{name}.mcstructure"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    (root, "test_level")
}

/// Gives a world its **own** copy of Construct, holding `structures`.
///
/// `fixture_world_with_construct` puts pack structures in the *shared* copy,
/// which `delete --world` deliberately will not touch. A test about a world's
/// own pack therefore needs the pack to actually be the world's: `pack::serving`
/// reports a world's own Construct instead of the shared one when it finds it,
/// so creating this makes the shared copy invisible to that world.
fn add_world_construct(world_dir: &std::path::Path, structures: &[(&str, &[u8])]) {
    let bp = world_dir.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::write(
        bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();
    for (name, bytes) in structures {
        std::fs::write(
            bp.join("structures").join(format!("{name}.mcstructure")),
            bytes,
        )
        .unwrap();
    }
}

/// Extracts a *second* copy of the real-leveldb fixture world into `worlds_dir`
/// under `folder`, with its own copy of Construct (empty `structures/`).
/// Pairs with `fixture_world_with_construct`'s primary world to give `copy` a
/// destination that is a real, openable world — unlike `world_with_construct`,
/// whose `db/` is a stub directory that cannot be opened as a real LevelDB (see
/// the comments on the `copy_*` tests using `world_with_construct` above).
///
/// The archive always unpacks to a directory named `test_level`, so this
/// extracts into a throwaway staging directory first and renames the result
/// into place under the caller's `worlds_dir` — `std::fs::rename` is a same-
/// filesystem move here, since both directories come from `tempfile::tempdir`
/// under the same OS temp root.
fn add_destination_world(worlds_dir: &std::path::Path, folder: &str) -> std::path::PathBuf {
    let staging = tempfile::tempdir().unwrap();
    let gz = std::fs::File::open("../construct-core/tests/fixtures/world.tar.gz")
        .expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(staging.path())
        .unwrap();
    let world = worlds_dir.join(folder);
    std::fs::rename(staging.path().join("test_level"), &world).unwrap();
    std::fs::write(world.join("levelname.txt"), folder).unwrap();

    let bp = world.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::write(
        bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();

    world
}

#[test]
fn copy_reads_from_a_real_world_database_into_the_destinations_pack() {
    // The three `copy` tests above all pass `--source world-pack`, because
    // `world_with_construct`'s stub `db/` cannot be opened as real LevelDB
    // (see the comments there). None of them exercises `copy`'s primary use:
    // reading a structure out of an actual world database. This test does,
    // using the same real-leveldb fixture `export`'s collision test relies
    // on, with a second, independently named world as the destination.
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
    // Byte transparency: the file is the database value, untouched — same
    // check the real-leveldb `export` tests use.
    assert_eq!(std::fs::read(&written).unwrap()[0], 0x0a);
}

#[test]
fn a_name_in_both_world_and_pack_survives_unshadowed_in_the_structures_listing() {
    // §5's never-guess rule (Task 1) says a name colliding across sources must
    // never let one copy silently win — `catalog::unify` keeps both entries
    // rather than deduping, and `catalog::resolve` is what refuses to pick
    // between them. `structures` never calls `resolve` (it has no single name to
    // resolve), so the CLI-visible half of that rule *this* command can prove
    // is that both entries survive to the list, neither shadowing the other.
    // The other half — that a command asking for exactly one structure by
    // this name refuses with exit 2 naming both sources — is
    // `export_of_a_name_in_both_world_and_pack_is_refused_naming_both_sources`
    // below, via `export`'s `catalog::resolve` call.
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
    // This fixture installs Construct in the shared root, so the pack copy is
    // a `shared-pack` row: one axis now says both "a pack, not the database"
    // and "which pack", where it used to take two fields to say it.
    assert_eq!(sources, ["shared-pack", "world-db"].into_iter().collect());
}

#[test]
fn copy_writes_bytes_into_the_destination_worlds_own_construct() {
    // Two worlds under one root, so one --path covers both. `Other`
    // gets its own copy of Construct (see
    // copy_refuses_an_existing_target_unless_forced below) so the write
    // lands somewhere distinct from the source: a shared-copy version of
    // this test cannot prove `copy` writes into the *destination's* Construct
    // rather than just re-touching wherever it read from.
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("levelname.txt"), "Other").unwrap();
    // Needed for `discovery::enumerate` to see this as a world at all — see
    // the note on `world_with_construct` above.
    std::fs::write(other.join("level.dat"), b"x").unwrap();
    let bp = other.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();

    // `--source shared-pack` is required here, not merely convenient: `world_with_construct`'s
    // `db/` is an empty stub directory, never a real LevelDB, and this backend's FFI
    // hardcodes `create_if_missing = false` (confirmed against
    // third_party/checkouts/leveldb-sys/ffi/ffi.cpp and db_impl.cc) — so opening it
    // for real, as a plain `copy` with no `--source` would, fails with a database
    // error before resolution even runs. `barn` lives only in the pack in this
    // fixture, so filtering to it is also the honest description of the test.
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
    // Qualified, not display_name: `--path` roots with no config name
    // are numbered `flag1`, `flag2`, ... (see `main.rs`), and both worlds sit
    // under the one root this test passes, with no account segment (a single
    // world root carries none). §6 supports cross-root copies, where two
    // worlds can share a display name across installations — `qualified()`
    // is what disambiguates that case, so the payload must carry it.
    assert_eq!(v["from"], "flag1/Test");
    assert_eq!(v["to"], "flag1/Other");
    // Which pack took the write, in the payload as well as in the printed
    // line — `import` reports it the same way and `copy` writes into the same
    // two places. Spelled as a `--source` value, so the answer names a place
    // the reader can point the next command at.
    assert_eq!(v["target"], "world-pack");

    let written = bp.join("structures/barn.mcstructure");
    assert_eq!(std::fs::read(&written).unwrap(), b"barn-bytes");
}

#[test]
fn copy_creates_the_destination_worlds_structures_pack() {
    // `copy` always names a destination world, so it always writes somewhere
    // that world owns: with no structures pack yet it creates one, rather
    // than falling back to the shared Construct and putting the structure in
    // every world.
    //
    // The source keeps `barn` in its own copy of Construct so the two ends of
    // the copy are genuinely different packs.
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

    // A pack `--source` for the same reason as the test above: this fixture's
    // `db/` is a stub directory, not a real LevelDB. `world-pack` and not
    // `shared-pack` because `Test` was just given its own copy of Construct
    // above, which shadows the shared one — the distinction the merged flag
    // now carries on its own.
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
    // Give Other its own Construct copy holding a different `barn`.
    let bp = other.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();
    std::fs::write(bp.join("structures/barn.mcstructure"), b"theirs").unwrap();

    // `--source shared-pack`: see the comment in
    // copy_writes_bytes_into_the_destination_worlds_own_construct above —
    // `world_with_construct`'s `db/` cannot be opened as a real LevelDB.
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
    // `--source shared-pack`: see the comment in
    // copy_writes_bytes_into_the_destination_worlds_own_construct above —
    // `world_with_construct`'s `db/` cannot be opened as a real LevelDB.
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
fn export_of_a_name_in_both_world_and_pack_is_refused_naming_both_sources() {
    // The other half of §5's never-guess rule: a name present in both the
    // world's database and a pack must not let `export` silently pick one.
    //
    // The pack has to be the world's *own* copy of Construct. `--world` cannot
    // reach the shared copy at all now, so a name in the database and the
    // shared pack is no longer two things — it is one, and answering with it is
    // right rather than a guess. The ambiguity that survives is the one inside
    // a single scope, which is what this fixture builds.
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
    // One axis, so the hint is the matched places read back as flag values —
    // there is no second flag left to send the user to.
    assert!(
        err.contains("--source world-db") && err.contains("--source world-pack"),
        "the hint must offer each matched place as a --source value:\n{err}"
    );
}

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
    // The space becomes `_`; the capitals are the user's and are kept.
    assert_eq!(v["written"][0]["name"], "My_House");
    assert_eq!(v["written"][0]["id"], "mystructure:My_House");
    // Into the world's own structures pack, which this import created: the
    // world's Construct is the shared copy, so writing there would have put
    // the structure in every world using it.
    let written = root.path().join(
        "minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/My_House.mcstructure",
    );
    assert_eq!(std::fs::read(&written).unwrap(), b"structure-bytes");
}

#[test]
fn import_accepts_a_name_with_capitals() {
    // `--name` used to refuse any capital, which made half the structures a
    // world holds unaddressable: `construct structures` prints `10HzCounter`, and
    // nothing could then import or copy under that name.
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
    // The collision that matters is one inside the destination pack, so the
    // first import establishes it: it creates the world's structures pack and
    // puts `house` there. A `house` in the *shared* pack would not collide at
    // all now — different pack, different file — and only warns.
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
    // Without `--world` there is no per-world home to choose, so the shared
    // Construct is the deliberate destination — and the one case where a
    // single import reaches every world, which is why it is said out loud.
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
    // The same world, plus its own copy of Construct — which takes precedence
    // over the shared one, and confines the structure to that world.
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
    // Needed for `discovery::enumerate` to see this as a world at all — see
    // the note on `world_with_construct` above.
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
    // Removing from the shared copy takes the structure away from every world
    // using it, not just the one named on the command line — the same
    // distinction `import` and `copy` state on the way in.
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

    // And in the printed output, which is where a person reads it.
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
    // The world's own copy shadows the shared one, so this delete affects
    // that world alone — and the shared copy still holds its own `bomber`,
    // which this command must neither touch nor claim to have touched.
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
fn structures_with_no_world_shows_only_the_shared_copy() {
    // The installation's own view: what every world using this pack gets.
    // A world's own structures are not part of that answer, and no database
    // is opened to produce it.
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
    // Both `world-db` and `world-pack` name a place inside a world, and none
    // was named. `shared-pack` is the one value a bare `structures` can serve,
    // and it is what the command already does by default.
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
    // The question this whole split exists to answer: is this structure mine
    // alone, or does every world using the shared copy have it? Both packs
    // serve this world, so both appear in one listing, distinguished by the
    // single SOURCE column — `world-pack` against `shared-pack`, where it
    // used to take a `source` and a `scope` together to say the same thing.
    //
    // The real-leveldb fixture, so no `--source` is needed to keep the
    // command off an unopenable stub `db/`: the point here is the whole view,
    // and narrowing it to one pack would defeat that.
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
fn a_second_import_uses_the_structures_pack_the_first_one_created() {
    // The first import creates the home; the second has to find it and say
    // what it is, rather than creating a second one or falling back to the
    // shared Construct.
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
fn a_name_in_two_packs_serving_one_world_warns() {
    // Different packs, different files — so the file-level collision rule has
    // nothing to refuse. The game loads both and logs a conflict, which is
    // not something the tool can resolve, only report.
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

#[test]
fn install_world_gives_the_world_its_own_structures_pack() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0);
    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
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

    let shell = world.join("behavior_packs/ConstructStructures");
    assert!(shell.join("manifest.json").is_file(), "no manifest");
    assert!(shell.join("structures").is_dir(), "no structures folder");
    // Enabled, or the game would never load what is put in it.
    let enabled = std::fs::read_to_string(world.join("world_behavior_packs.json")).unwrap();
    assert!(
        enabled.contains("9f7d83af-309e-4997-840e-e9c350435e83"),
        "structures pack not enabled: {enabled}"
    );
}

#[test]
fn install_world_gives_no_structures_pack_to_a_world_that_has_its_own_construct() {
    // That world's Construct copy already holds a per-world `structures/`;
    // a second pack beside it would split one world's structures in two.
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0);
    let local = world.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(local.join("structures")).unwrap();
    std::fs::write(
        local.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();
    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
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
    assert!(
        !world.join("behavior_packs/ConstructStructures").exists(),
        "a world with its own Construct needs no shell pack"
    );
}

/// A world that sees `house` twice: once in the shared Construct, once in its
/// own structures pack. The state every user reaches by giving a world its own
/// copy of something the shared copy already had.
fn world_seeing_one_name_in_both_packs() -> tempfile::TempDir {
    let root = world_with_construct(&[("house", b"shared-copy")]);
    let src = root.path().join("house.mcstructure");
    std::fs::write(&src, b"world-copy").unwrap();
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
    root
}

fn shared_house(root: &std::path::Path) -> std::path::PathBuf {
    root.join("development_behavior_packs/Construct[BP]/structures/house.mcstructure")
}

fn world_house(root: &std::path::Path) -> std::path::PathBuf {
    root.join(
        "minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/house.mcstructure",
    )
}

#[test]
fn delete_with_a_world_removes_that_worlds_copy_and_leaves_the_shared_one() {
    // The scope guarantee, in the direction that protects other worlds:
    // --world means this world, and the shared copy serves worlds this command
    // never named, so it must come through untouched.
    let root = world_seeing_one_name_in_both_packs();
    // --source world-pack because this fixture's `db/` is a stub that cannot be
    // opened; the property under test is pack scoping, not the database.
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
    // The other half of the same grammar: absence means shared, exactly as it
    // does for `import` and `structures`.
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
    // The failure that must NOT silently succeed. Before the grammar change a
    // world-scoped delete would happily unlink the shared copy, taking the
    // structure from every other world using it. Now it reports a miss, and
    // the miss names the world so the user is not sent looking in the wrong
    // place.
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
    // `--pack` is gone: `--source` names the pack directly now, so the two
    // overlapping selectors became one. What survives is the contradiction
    // they used to make possible — `--world W` with the shared copy named —
    // and `delete` refuses it rather than guessing, because the shared copy is
    // exactly what `--world` exists to protect.
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
    // Refused means nothing was unlinked, in either copy.
    assert!(shared_house(root.path()).is_file());
    assert!(world_house(root.path()).is_file());
}

#[test]
fn a_read_only_command_refuses_the_force_flag() {
    // --force answers exactly one question — overwrite the thing already
    // there? — and `worlds` never writes anything, so it has nothing to
    // answer. Declared only on the four commands that read it, so clap
    // refuses it here instead of accepting it and doing nothing.
    let root = world_with_construct(&[]);
    let out = bin()
        .args(["worlds", "--force", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument '--force'"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn add_refuses_the_path_flag() {
    // --path widens the search, and `add` does not search: it writes the
    // path it was given into the config file. Declared only on the commands
    // that run discovery, so clap refuses it rather than letting it look like
    // a second way to name the root being added.
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "add",
            dir.path().to_str().unwrap(),
            "--path",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument '--path'"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn export_without_a_world_writes_the_shared_copy() {
    // `export` reads the grammar `import` writes: absence means the shared copy
    // of Construct, the same way it does for `import`, `delete` and `structures`. No
    // world is named, so no world's copy can be the answer.
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
    // The other half of the grammar. Both packs serving this world hold a
    // `house`; --world says which one is meant, which is why --pack no longer
    // has to.
    let root = world_seeing_one_name_in_both_packs();
    let dir = tempfile::tempdir().unwrap();
    // --source world-pack because this fixture's `db/` is a stub that cannot be
    // opened; the property under test is pack scoping, not the database.
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
    // The scope guarantee `delete --world` makes, made by `export` too: a
    // world-scoped read is about that world, so a name that exists only in the
    // shared copy reports a miss naming the world rather than quietly
    // answering with a structure the world does not have of its own.
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
    // Same story as `delete`: `--pack` folded into `--source`, and the
    // contradiction it enabled is now refused on the one axis that is left.
    // `--world W --source shared-pack` asks to read the copy `--world` just
    // excluded, so nothing is written and nothing is guessed.
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
    // Refused means nothing was written.
    assert!(!dir.path().join("house.mcstructure").exists());
}

#[test]
fn export_with_no_world_refuses_the_sources_that_need_one() {
    // The same refusal `structures` and `delete` make: a world's structures
    // live inside a world, and none was named.
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

#[test]
fn structures_takes_a_shared_source_under_a_world_where_export_and_delete_refuse_it() {
    // `--pack` is gone here too, but the contradiction `export` and `delete`
    // refuse is not one for a listing: a world's view genuinely *contains* the
    // shared copy's rows when that is what the world runs, so narrowing to
    // them answers a real question. It is also the only way to list a world's
    // structures without opening its database, which a read must avoid — so
    // refusing it here would cost a capability rather than prevent a mistake.
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
    // What `--pack` used to narrow, `--source` now does on its own: one name
    // in both packs serving this world is two rows, and each is reachable by
    // naming its pack. That is the whole reason the two flags collapsed into
    // one — `--source world-pack` could not tell these two apart, and needed
    // a second flag to finish the sentence.
    //
    // Two runs rather than one because this fixture's `db/` is a stub
    // directory (see the note on `world_with_construct`), so every run has to
    // name a pack source to stay off it.
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

/// Backdates every file directly inside a world's `db/`, so the in-use
/// heuristic reads the world as closed.
///
/// `inuse` calls a world in use when `db/` was written in the last ten
/// seconds, which is exactly what a fixture that just inserted keys through
/// the leveldb API looks like. Real worlds this tool is pointed at have been
/// closed for longer than that; a fixture has to say so.
fn close_world(world: &std::path::Path) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    for entry in std::fs::read_dir(world.join("db")).unwrap().flatten() {
        let f = std::fs::File::options()
            .write(true)
            .open(entry.path())
            .unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(when))
            .unwrap();
    }
}

/// The identity of a `db/` directory: every filename with its length.
///
/// Opening a leveldb rewrites its file set — that is the measured fact spec §8
/// rests on — so this is what proves a command did or did not open one.
fn db_fingerprint(world: &std::path::Path) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = std::fs::read_dir(world.join("db"))
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.metadata().map(|m| m.len()).unwrap_or(0),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn delete_removes_a_structure_from_a_world_database() {
    // The tool's only leveldb write. The fixture world really contains
    // `house`; after this it must not, and a fresh read of the world has to
    // agree — the removal is verified against the live handle before the
    // command reports it, and again here from a separate process.
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
    // The neighbours are still there: a delete removes what it was asked for
    // and nothing else.
    assert!(names.contains(&"barn"), "{v}");
    assert!(names.contains(&"understudy:players"), "{v}");
}

#[test]
fn delete_removes_the_database_copy_and_the_pack_copy_together() {
    // The case the delete-both rule exists for: one name, two places, and no
    // flag to say which was meant. Both go.
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);
    // The world's *own* pack: the shared copy is out of scope for --world, so
    // a shared-copy fixture would prove nothing here.
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
    // Two rows share a name, so `source` is the field that tells them apart.
    let sources: Vec<&str> = rows.iter().map(|r| r["source"].as_str().unwrap()).collect();
    assert!(sources.contains(&"world-db"), "{v}");
    assert!(sources.contains(&"world-pack"), "{v}");
}

#[test]
fn delete_source_world_leaves_the_pack_copy_alone() {
    // The flags still narrow what the bare form sweeps up.
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
    // The property that made `delete --source world-pack` shippable in stage 2, and
    // it has to survive stage 4 giving the command a write path. Opening a
    // leveldb rewrites its file set, so an unchanged fingerprint is proof the
    // database was never opened at all.
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
fn delete_refuses_a_world_that_looks_in_use_and_writes_nothing() {
    // The only guard on the only leveldb write. There is deliberately no
    // --force: a second writer is how databases get corrupted rather than
    // merely stale, so the refusal has to arrive before the open, which is
    // itself a write.
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    let before = db_fingerprint(&world_dir);
    // A world that keeps being written, the way a loaded one is. One write is
    // deliberately not enough — `delete` writes `db/` itself, so a single
    // recent write is as likely to be this tool's as the game's.
    let _live = live_world_at(&world_dir.join("db"));

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

    assert_eq!(out.status.code(), Some(1));
    // The simulator adds a log file of its own, so compare only the files
    // that were there to begin with: opening a leveldb rewrites *those*.
    let after = db_fingerprint(&world_dir);
    for entry in &before {
        assert!(
            after.contains(entry),
            "a refused delete must not have opened the database: {entry:?} changed"
        );
    }
    // The two write paths are refused for different reasons. Telling a
    // `delete` user their removal "would be silently discarded" describes
    // `level.dat`'s hazard, not this one.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is how a save gets corrupted"),
        "the refusal must give the database's reason: {stderr}"
    );
    assert!(
        !stderr.contains("silently discarded"),
        "that is level.dat's reason, not this one: {stderr}"
    );
}

#[test]
fn a_second_delete_straight_after_the_first_is_not_blocked_by_it() {
    // `delete` writes `db/`, and the in-use check is write-recency — so
    // without a record of our own write the second command here reads the
    // first one's as a running game and refuses. It did exactly that before
    // `writemark` existed.
    //
    // Timing is part of the assertion: falling back to the watch would still
    // reach the right answer, just `CONFIRM_WATCH` seconds later. The point of
    // the mark is that no wait happens at all.
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);

    let first = bin_isolated(root.path())
        .args([
            "delete",
            "barn",
            "--world",
            world,
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let started = std::time::Instant::now();
    let second = bin_isolated(root.path())
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
    let took = started.elapsed();

    assert!(
        second.status.success(),
        "a delete must not be blocked by this tool's own previous write; stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        took < construct_core::inuse::CONFIRM_WATCH,
        "answered from the mark, so it must not have watched: took {took:?}"
    );
    // And it must not have announced a wait it never took.
    let text = String::from_utf8_lossy(&second.stdout);
    assert!(!text.contains("watching up to"), "stdout:\n{text}");
}

#[test]
fn a_mark_from_our_own_write_does_not_wave_through_a_live_world() {
    // The failure mode `writemark` could introduce if it were wrong: a stale
    // "that write was ours" answer suppressing detection of a world the game
    // really does have open. It must not, because the game writing since
    // leaves a strictly newer mtime than the one recorded.
    //
    // This is the corrupting direction, so it gets its own test rather than
    // resting on the unit-level equality check.
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);

    // A real delete, which leaves a real mark behind.
    assert!(
        bin_isolated(root.path())
            .args([
                "delete",
                "barn",
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

    // Now the game opens the world and starts writing over the top of it.
    let _live = live_world_at(&world_dir.join("db"));

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

    assert_eq!(
        out.status.code(),
        Some(1),
        "a mark must never suppress detection of a live world; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn delete_of_a_batch_with_one_bad_name_removes_nothing_from_the_database() {
    // The plural contract, now that it spans two kinds of storage: every name
    // resolves before anything is removed, so an unknown name at the end of
    // the list leaves the database untouched.
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

/// All files under `dir`, recursively.
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// A world whose level.dat carries an `experiments` compound.
fn world_with_experiments(gametest: i8) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();

    let mut experiments = std::collections::HashMap::new();
    experiments.insert("gametest".to_string(), nbtx::Value::Byte(gametest));
    let mut level = std::collections::HashMap::new();
    level.insert(
        "experiments".to_string(),
        nbtx::Value::Compound(experiments),
    );
    level.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));

    let payload = nbtx::to_le_bytes(&nbtx::Value::Compound(level)).unwrap();
    let mut bytes = 10i32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&payload);
    std::fs::write(world.join("level.dat"), bytes).unwrap();
    root
}

#[test]
fn enable_beta_apis_turns_it_on_and_backs_the_file_up_first() {
    let root = world_with_experiments(0);
    let level = root.path().join("minecraftWorlds/Test/level.dat");
    let before = std::fs::read(&level).unwrap();
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[backups]\ndir = {:?}\nkeep = 5\n", backups),
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "enable-beta-apis",
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
    assert_eq!(v["beta_apis"], true);
    assert_eq!(v["changed"], true);
    assert!(v["backup"].is_string());

    // Verified by running it again: on a world that now has Beta APIs on,
    // a second run reports the no-op, and that report is the command's own
    // read of the file it just wrote.
    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "enable-beta-apis",
            "Test",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["beta_apis"], true);
    assert_eq!(v["changed"], false, "the flip must already have stuck");

    // The backup holds exactly the pre-flip file — the property that
    // actually matters, not merely that it differs from the post-flip one.
    let saved: Vec<_> = walk(&backups).into_iter().filter(|p| p.is_file()).collect();
    assert_eq!(saved.len(), 1, "one backup: {saved:?}");
    assert_eq!(std::fs::read(&saved[0]).unwrap(), before);
    assert_ne!(
        std::fs::read(&saved[0]).unwrap(),
        std::fs::read(&level).unwrap()
    );
}

#[test]
fn a_no_op_flip_takes_no_backup() {
    // Regression: `backup::file` used to run before `apply_beta_apis`'s own
    // "already in that state" short-circuit, so ten no-op `enable-beta-apis`
    // runs would evict every genuine pre-flip backup at the default `keep`.
    // A no-op must take none at all.
    let root = world_with_experiments(1); // beta apis already on
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[backups]\ndir = {:?}\nkeep = 5\n", backups),
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "enable-beta-apis",
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
    assert_eq!(v["changed"], false);
    assert!(v["backup"].is_null());

    let saved: Vec<_> = walk(&backups).into_iter().filter(|p| p.is_file()).collect();
    assert!(saved.is_empty(), "expected no backup taken: {saved:?}");
}

#[test]
fn enable_beta_apis_on_a_world_with_no_level_dat_fails_cleanly() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds/Test/db")).unwrap();
    let out = bin()
        .args([
            "enable-beta-apis",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
}

/// Writes a `level.dat` at `path`: version 10, an `experiments` compound
/// carrying just `gametest`. Like `world_with_experiments` above, but writes
/// straight to a caller-given path rather than building a whole com.mojang
/// layout, so `install` tests can lay a world out however they need to.
fn write_level_dat(path: &std::path::Path, gametest: i8) {
    let mut experiments = std::collections::HashMap::new();
    experiments.insert("gametest".to_string(), nbtx::Value::Byte(gametest));
    let mut level = std::collections::HashMap::new();
    level.insert(
        "experiments".to_string(),
        nbtx::Value::Compound(experiments),
    );
    level.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));

    let payload = nbtx::to_le_bytes(&nbtx::Value::Compound(level)).unwrap();
    let mut bytes = 10i32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&payload);
    std::fs::write(path, bytes).unwrap();
}

/// The same synthetic `.mcaddon` shape as construct-core's
/// `real_shaped_addon` (crates/construct-core/tests/install.rs), built in
/// memory instead of on disk. Carries Construct's real header UUIDs, since
/// `install` now verifies them before placing anything (§18's addition).
fn build_mcaddon_bytes() -> Vec<u8> {
    use std::io::Write;

    fn manifest(name: &str, uuid: &str, module: &str) -> Vec<u8> {
        format!(
            r#"{{"format_version":2,
                "header":{{"name":"{name}","uuid":"{uuid}","version":[1,2,0]}},
                "modules":[{{"type":"{module}","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}}]}}"#
        )
        .into_bytes()
    }

    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let bp = manifest(
        "Construct [BP] v1.2.0",
        "8c0c0153-d8b9-482a-889f-aef922b8fe58",
        "data",
    );
    let rp = manifest(
        "Construct [RP] v1.2.0",
        "375ec465-3dc1-429f-8b4c-a337889e1ed4",
        "resources",
    );

    zip.start_file("Construct[BP]/manifest.json", options)
        .unwrap();
    zip.write_all(&bp).unwrap();
    zip.start_file("Construct[BP]/scripts/main.js", options)
        .unwrap();
    zip.write_all(b"// code").unwrap();
    zip.start_file("Construct[BP]/structures/construct.mcstructure", options)
        .unwrap();
    zip.write_all(b"shipped").unwrap();
    zip.start_file("Construct[RP]/manifest.json", options)
        .unwrap();
    zip.write_all(&rp).unwrap();
    zip.finish().unwrap();

    cursor.into_inner()
}

/// A single-threaded HTTP server that answers exactly the two requests
/// `install` makes, then stops. Returns its base URL and a join handle.
///
/// This is what lets `install` be tested end to end — download included —
/// without the network or a mocking framework.
///
/// The handle is deliberately left unjoined by callers: `ureq` may serve both
/// requests over one connection, in which case the server's second `accept()`
/// never returns and joining would hang the whole suite forever. The thread
/// is daemon-like — it exits on its own once both requests land, or the test
/// process exits around it either way.
fn stub_github(addon: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"Construct-v1.2.0.mcaddon","size":{},"browser_download_url":"{base}/download"}}]}}"#,
        addon.len()
    );

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let body: Vec<u8> = if line.contains("/download") {
                addon.clone()
            } else {
                release.clone().into_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

/// Like `stub_github`, but the release advertises a larger asset size than
/// the bytes actually served for `/download` — simulating a connection that
/// drops mid-download without needing to actually sever one. Exists only for
/// the truncated-download test; not joined, for the same reason `stub_github`
/// isn't.
fn stub_github_wrong_size(
    addon: Vec<u8>,
    claimed_size: u64,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"Construct-v1.2.0.mcaddon","size":{claimed_size},"browser_download_url":"{base}/download"}}]}}"#
    );

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let body: Vec<u8> = if line.contains("/download") {
                addon.clone()
            } else {
                release.clone().into_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

/// A single-request stub answering only the release lookup, for commands like
/// `status` that check a version but never download anything. Kept separate
/// from `stub_github` rather than adding a parameter to it: that one's loop
/// is sized to `install`'s exact two-request sequence, and every existing
/// caller of it relies on that shape unchanged.
fn stub_github_release(tag: &str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(r#"{{"tag_name":"{tag}","assets":[]}}"#);

    let handle = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut line = String::new();
        let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
        let body = release.into_bytes();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    (base, handle)
}

/// A single-request stub answering with a chosen status, headers and body —
/// for the error paths GitHub's HTTP responses drive rather than its JSON
/// (rate limiting, a missing version). Never joined, for the same reason
/// `stub_github` isn't: a second `accept()` that never comes would hang the
/// suite.
fn stub_github_status(
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let body = body.as_bytes().to_vec();
    let extra_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    let reason = match status {
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };

    let handle = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut line = String::new();
        let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra_headers}\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    (base, handle)
}

#[test]
fn install_reports_rate_limiting_with_the_token_guidance_when_github_returns_403() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let (base, _server) = stub_github_status(
        403,
        &[("x-ratelimit-remaining", "0")],
        r#"{"message":"API rate limit exceeded"}"#,
    );

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("60") && stderr.contains("CONSTRUCT_GITHUB_TOKEN"),
        "expected the unauthenticated-limit guidance: {stderr}"
    );
    assert!(!root.path().join("development_behavior_packs").exists());
}

#[test]
fn install_reports_asset_not_found_and_lists_available_assets_when_the_version_is_missing() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let (base, _server) = stub_github_status(404, &[], r#"{"message":"Not Found"}"#);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
            "--version",
            "9.9.9",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!root.path().join("development_behavior_packs").exists());
}

#[test]
fn install_places_both_packs_and_enables_them_in_a_world() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0); // gametest = 0

    let addon = build_mcaddon_bytes(); // the same synthetic archive as the core tests
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
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
    assert_eq!(v["version"], "1.2.0");
    assert_eq!(v["world"], "Test");
    assert_eq!(v["beta_apis"], true);

    assert!(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json")
            .is_file()
    );
    assert!(
        root.path()
            .join("development_resource_packs/Construct[RP]/manifest.json")
            .is_file()
    );

    let enabled = std::fs::read_to_string(world.join("world_behavior_packs.json")).unwrap();
    assert!(enabled.contains("8c0c0153-d8b9-482a-889f-aef922b8fe58"));
    let enabled_rp = std::fs::read_to_string(world.join("world_resource_packs.json")).unwrap();
    assert!(enabled_rp.contains("375ec465-3dc1-429f-8b4c-a337889e1ed4"));
}

#[test]
fn install_reports_an_unreachable_github_without_touching_anything() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        // Port 1 refuses immediately on every platform we target.
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!root.path().join("development_behavior_packs").exists());
}

#[test]
fn install_names_both_byte_counts_when_the_download_is_truncated() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let addon = build_mcaddon_bytes();
    let actual = addon.len() as u64;
    let claimed = actual + 1000;
    let (base, _server) = stub_github_wrong_size(addon, claimed);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!root.path().join("development_behavior_packs").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&claimed.to_string()) && stderr.contains(&actual.to_string()),
        "expected both the claimed and actual byte counts in the error: {stderr}"
    );
}

#[test]
fn install_verifies_the_addon_is_actually_construct_before_placing_anything() {
    // A `.mcaddon` shaped correctly (one BP, one RP, matched by module type)
    // but carrying uuids that are not Construct's. `install` asked GitHub
    // specifically for Construct, so it must check the answer rather than
    // installing whatever it was handed under Construct's well-known uuids.
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("BP/manifest.json", options).unwrap();
    zip.write_all(
        br#"{"format_version":2,"header":{"name":"Not Construct","uuid":"11111111-1111-1111-1111-111111111111","version":[1,0,0]},"modules":[{"type":"data","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}]}"#,
    )
    .unwrap();
    zip.start_file("RP/manifest.json", options).unwrap();
    zip.write_all(
        br#"{"format_version":2,"header":{"name":"Not Construct RP","uuid":"33333333-3333-3333-3333-333333333333","version":[1,0,0]},"modules":[{"type":"resources","uuid":"44444444-4444-4444-4444-444444444444","version":[1,0,0]}]}"#,
    )
    .unwrap();
    zip.finish().unwrap();
    let addon = cursor.into_inner();

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(!root.path().join("development_behavior_packs").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("11111111-1111-1111-1111-111111111111"),
        "expected the found uuid to be named in the error: {stderr}"
    );
}

#[test]
fn install_reports_partial_with_the_packs_already_placed_when_level_dat_cannot_be_flipped() {
    // §11's distinguishing behaviour: the packs land, but the world's
    // level.dat cannot be read, so the Beta APIs flip never happens. That is
    // a partial install, not a plain failure — the downloaded packs are real
    // work already done and must not be thrown away just because the last step
    // failed. It exits 1 like any failure, but the payload survives and
    // `error.kind` is `partial-install`, which is what tells the two apart.
    //
    // A four-byte level.dat is shorter than the 8-byte header `leveldat::read`
    // requires, so `apply_beta_apis` fails deterministically at its first
    // read, on every platform — no permission games needed. `enumerate` only
    // requires level.dat to *exist* to find the world at all (it falls back
    // to directory mtime when the file can't be parsed for LastPlayed), so
    // the world is still discovered and `--world Test` still resolves.
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    std::fs::write(world.join("level.dat"), b"bad!").unwrap();

    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
            "--world",
            "Test",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The whole point of partial-install: the packs are really there.
    assert!(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json")
            .is_file()
    );
    assert!(
        root.path()
            .join("development_resource_packs/Construct[RP]/manifest.json")
            .is_file()
    );

    // Still exactly one JSON document on stdout, carrying the failure — and
    // carrying the payload *and* the error together, which is the property
    // that keeps "non-zero exit implies error.kind" true without an exception
    // for this command.
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["error"]["kind"], "partial-install");
    assert!(
        !v["version"].is_null() && !v["behavior"].is_null(),
        "the payload must survive beside the error, got {v}"
    );
    assert!(
        v["warnings"].as_array().is_some_and(|w| !w.is_empty()),
        "expected a non-empty warnings array, got {v}"
    );
    assert!(
        !v["level_dat_error"].is_null(),
        "expected the level.dat failure represented in the payload, got {v}"
    );
    assert!(v["beta_apis"].is_null());

    // stderr names the manual step.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("construct enable-beta-apis Test"),
        "expected the manual recovery command in stderr: {stderr}"
    );
}

#[test]
fn install_reports_partial_with_the_packs_already_placed_when_the_world_pack_list_is_malformed() {
    // Mirrors the level.dat partial-install case above, but for the other
    // partial-success path: the packs land, but a malformed world_behavior_packs.json
    // means Construct cannot be enabled in the world. That must not throw
    // away the already-downloaded packs by propagating a bare `?` failure.
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0); // gametest = 0, flips cleanly
    std::fs::write(world.join("world_behavior_packs.json"), "{ not an array").unwrap();

    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
            "--world",
            "Test",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The whole point of partial-install: the packs are really there.
    assert!(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json")
            .is_file()
    );
    assert!(
        root.path()
            .join("development_resource_packs/Construct[RP]/manifest.json")
            .is_file()
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "partial-install");
    assert!(
        !v["enable_error"].is_null(),
        "expected the enable failure represented in the payload, got {v}"
    );
    assert!(
        !v["version"].is_null() && !v["behavior"].is_null(),
        "the payload must survive beside the error, got {v}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("construct install --world Test"),
        "expected the manual recovery command in stderr: {stderr}"
    );
}

#[test]
fn install_moves_a_construct_found_in_behavior_packs_and_keeps_its_structures() {
    // The README's by-hand route puts Construct in `development_behavior_packs`,
    // but `behavior_packs` is the folder right next to it and an easy miss.
    // The game loads both, so the misplaced copy is not inert — and every
    // command here looks only in the development root, so the structures in
    // it are unreachable until install moves them.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let stray = root.path().join("behavior_packs/Construct[BP]");
    write_construct_bp(&stray, [1, 1, 0]);
    std::fs::create_dir_all(stray.join("structures")).unwrap();
    std::fs::write(
        stray.join("structures/house.mcstructure"),
        b"the user's house",
    )
    .unwrap();

    let (base, _server) = stub_github(build_mcaddon_bytes());
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let installed = root.path().join("development_behavior_packs/Construct[BP]");
    assert_eq!(
        std::fs::read(installed.join("structures/house.mcstructure")).unwrap(),
        b"the user's house",
        "the misplaced copy's structure survived both the move and the upgrade"
    );
    assert!(!stray.exists(), "the misplaced copy is gone");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("behavior_packs") && stdout.contains("development_behavior_packs"),
        "expected install to say the pack was moved: {stdout}"
    );
}

#[test]
fn install_merges_a_behavior_packs_copy_into_the_development_one_without_losing_structures() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();

    let installed = root.path().join("development_behavior_packs/Construct[BP]");
    write_construct_bp(&installed, [1, 1, 0]);
    std::fs::create_dir_all(installed.join("structures")).unwrap();
    std::fs::write(
        installed.join("structures/house.mcstructure"),
        b"the dev house",
    )
    .unwrap();

    let stray = root.path().join("behavior_packs/Construct[BP]");
    write_construct_bp(&stray, [1, 0, 0]);
    std::fs::create_dir_all(stray.join("structures")).unwrap();
    std::fs::write(
        stray.join("structures/house.mcstructure"),
        b"a different house",
    )
    .unwrap();
    std::fs::write(stray.join("structures/barn.mcstructure"), b"the barn").unwrap();

    let (base, _server) = stub_github(build_mcaddon_bytes());
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let structures = installed.join("structures");
    assert_eq!(
        std::fs::read(structures.join("house.mcstructure")).unwrap(),
        b"the dev house",
        "the development copy keeps its own structure at its own id"
    );
    assert_eq!(
        std::fs::read(structures.join("house-1.mcstructure")).unwrap(),
        b"a different house",
        "and the misplaced copy's clashing structure is rescued beside it"
    );
    assert_eq!(
        std::fs::read(structures.join("barn.mcstructure")).unwrap(),
        b"the barn"
    );
    assert!(!stray.exists(), "the misplaced copy is gone");

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["migrated"][0]["merged"], 2);
    assert_eq!(v["migrated"][0]["rescued"][0]["from"], "house.mcstructure");
    assert_eq!(v["migrated"][0]["rescued"][0]["to"], "house-1.mcstructure");
}

/// A Construct behaviour pack on disk at `dir`, the way a by-hand install
/// leaves one.
fn write_construct_bp(dir: &std::path::Path, version: [u32; 3]) {
    let [a, b, c] = version;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"format_version":2,
                "header":{{"name":"Construct [BP] v{a}.{b}.{c}","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[{a},{b},{c}]}},
                "modules":[{{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}}]}}"#
        ),
    )
    .unwrap();
}

#[test]
fn install_world_warns_when_a_world_construct_copy_shadows_the_shared_copy() {
    // `install --world` always places into the shared
    // dev-pack root, but `pack::for_world` (and every structure command
    // through it) prefers a world's own `behavior_packs/Construct[BP]` copy
    // when it has one. Without a warning, this world would keep silently
    // running the untouched 1.1.0 copy after `install` reports 1.2.0.
    let root = tempfile::tempdir().unwrap();
    // Joined a component at a time, not as "minecraftWorlds/Test". A `/` inside
    // a join argument survives verbatim on Windows, so `local_bp` would render
    // with mixed separators and never match the all-backslash path the warning
    // below prints — which is the only assertion here that compares a path as
    // text.
    let world = root.path().join("minecraftWorlds").join("Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0);

    let local_bp = world.join("behavior_packs").join("Construct[BP]");
    std::fs::create_dir_all(&local_bp).unwrap();
    std::fs::write(
        local_bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.1.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,1,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();

    let addon = build_mcaddon_bytes(); // ships v1.2.0, per stub_github
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args([
            "install",
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
        stderr.contains("shadows") && stderr.contains(&local_bp.display().to_string()),
        "expected a warning naming the shadowing world copy: {stderr}"
    );

    // The shared copy really was upgraded...
    let shared_manifest = std::fs::read_to_string(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
    )
    .unwrap();
    assert!(shared_manifest.contains("1, 2, 0") || shared_manifest.contains("[1,2,0]"));
    // ...but the world's own copy install never touches is still 1.1.0.
    let local_manifest = std::fs::read_to_string(local_bp.join("manifest.json")).unwrap();
    assert!(local_manifest.contains("1.1.0"));
}

#[test]
fn status_reports_the_installed_version_and_which_worlds_have_it() {
    let root = world_with_construct(&[]);
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::write(
        world.join("world_behavior_packs.json"),
        r#"[{"pack_id":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]}]"#,
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "offline must not fail: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["installed"], "1.2.0");
    assert_eq!(v["latest"], serde_json::Value::Null);
    assert_eq!(v["enabled_worlds"][0], "Test");
    assert!(
        v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("latest")),
        "the payload must say why latest is missing: {v}"
    );
}

#[test]
fn status_counts_the_structures_in_every_pack_it_can_see() {
    // The cross-world view: `structures` answers one world at a time, and a
    // structure in the shared copy is in every world using it. Only this
    // shows both at once.
    let root = world_with_construct(&[("shared_prefab", b"x")]);
    let src = root.path().join("mine.mcstructure");
    std::fs::write(&src, b"y").unwrap();
    for name in ["mine", "also_mine"] {
        assert!(
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
                .status
                .success()
        );
    }

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let shared = &v["structures"][0];
    assert_eq!(shared["world"], serde_json::Value::Null);
    assert_eq!(shared["source"], "shared-pack");
    assert_eq!(shared["count"], 1);
    let world = &v["structures"][1];
    assert_eq!(world["world"], "Test");
    assert_eq!(world["source"], "world-pack");
    assert_eq!(world["count"], 2);

    // And in the printed form, which is where a person reads it.
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("1 in the shared copy of Construct")
            && text.contains("2 in Test's structures pack"),
        "stdout:\n{text}"
    );
}

#[test]
fn status_without_construct_points_at_install() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}

#[test]
fn a_world_without_construct_enabled_is_not_listed() {
    let root = world_with_construct(&[]);
    // No world_behavior_packs.json at all.
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["enabled_worlds"].as_array().unwrap().len(), 0);
}

#[test]
fn status_reports_up_to_date_when_installed_matches_latest() {
    // world_with_construct installs 1.2.0; a release tagged v1.2.0 is the
    // same version, `v` prefix and all.
    let root = world_with_construct(&[]);

    let (base, _server) = stub_github_release("v1.2.0");
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["status", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["latest"], "v1.2.0");
    assert_eq!(
        v["warnings"].as_array().unwrap().len(),
        0,
        "a successful check leaves no warning: {v}"
    );

    // The human-readable line takes the same branch; needs its own server
    // since each stub answers exactly one request.
    let (base, _server) = stub_github_release("v1.2.0");
    let human = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["status", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("up to date"), "got: {text}");
}

#[test]
fn status_reports_an_update_is_available() {
    let root = world_with_construct(&[]);

    let (base, _server) = stub_github_release("v1.3.0");
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["status", "--json", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["latest"], "v1.3.0");
    assert_eq!(v["installed"], "1.2.0");

    let (base, _server) = stub_github_release("v1.3.0");
    let human = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["status", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(
        text.contains("construct install"),
        "expected the human output to point at construct install: {text}"
    );
}

/// Simulates a world Minecraft currently has loaded, until the guard drops.
///
/// A single write is no longer enough, and that is the point: `inuse`'s second
/// phase watches for a *further* write precisely so it can tell a live world
/// from a command that just finished writing one. Only something that keeps
/// writing looks live. Measured against a real session, Bedrock rewrites `db/`
/// every 5-10 seconds; this ticks faster so the check trips promptly.
#[must_use = "the world stops looking live as soon as this is dropped"]
struct LiveWorld {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    writer: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LiveWorld {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

fn mark_db_active(root: &std::path::Path) -> LiveWorld {
    let db = root.join("minecraftWorlds/Test/db");
    live_world_at(&db)
}

fn live_world_at(db: &std::path::Path) -> LiveWorld {
    let log = db.join("000021.log");
    std::fs::write(&log, b"chunk data").unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let writer = std::thread::spawn(move || {
        // Push the mtime forward explicitly rather than relying on the clock
        // to tick between writes, so the watcher always sees it advance.
        let mut when = std::time::SystemTime::now();
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            when += std::time::Duration::from_secs(1);
            if let Ok(f) = std::fs::File::options().write(true).open(&log) {
                let _ = f.set_times(std::fs::FileTimes::new().set_modified(when));
            }
        }
    });
    LiveWorld {
        stop,
        writer: Some(writer),
    }
}

#[test]
fn enable_beta_apis_refuses_as_in_use_when_minecraft_has_the_world_open() {
    // The bug this guards: Minecraft keeps level.dat in memory for the whole
    // session and rewrites it from memory on every save, so a flip written
    // under a live world verifies correctly and is then silently discarded.
    // Refusing is the only honest answer, and a refusal must leave no trace.
    let root = world_with_experiments(0);
    let level = root.path().join("minecraftWorlds/Test/level.dat");
    let before = std::fs::read(&level).unwrap();
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[backups]\ndir = {:?}\nkeep = 5\n", backups),
    )
    .unwrap();
    let _live = mark_db_active(root.path());

    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "enable-beta-apis",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(&level).unwrap(),
        before,
        "a refused flip must not modify level.dat"
    );
    assert!(
        !backups.exists(),
        "a refused flip must not take a backup either"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Close the world"),
        "the refusal must say what to do about it: {stderr}"
    );
}

#[test]
fn install_world_refuses_as_in_use_before_it_downloads_anything() {
    // `CONSTRUCT_GITHUB_API` points at a port nothing listens on, so reaching
    // the network would fail as `network`. Failing as `world-in-use` instead
    // is what proves the in-use check runs before the download — a refused
    // `--world` install must leave nothing half-done.
    //
    // The exit code cannot carry this: both are failures, so both exit 1. The
    // discriminator is `error.kind`, which is why this asks for `--json`.
    let root = world_with_experiments(0);
    let _live = mark_db_active(root.path());

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1/")
        .args([
            "install",
            "--world",
            "Test",
            "--json",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["error"]["kind"],
        "world-in-use",
        "the in-use check must preempt the download; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !root.path().join("development_behavior_packs").exists(),
        "a refused install must place no packs"
    );
}

#[test]
fn install_without_a_world_ignores_whether_any_world_is_in_use() {
    // The in-use check is scoped to `--world`. Installing the packs alone
    // touches no world at all, so a live world is none of its business.
    let root = world_with_experiments(0);
    let _live = mark_db_active(root.path());
    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json")
            .is_file()
    );
}

#[test]
fn a_flip_that_required_a_closed_world_does_not_ask_for_a_reload() {
    // `enable-beta-apis` refuses outright while the world is open,
    // so a flip that succeeded happened with the world closed. Telling the
    // user to reload a world they are not in is noise.
    let root = world_with_experiments(0);
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[backups]\ndir = {:?}\nkeep = 5\n", backups),
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "enable-beta-apis",
            "Test",
            "--path",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("Beta APIs: off \u{2192} on"),
        "the flip itself must still be reported: {text}"
    );
    assert!(!text.to_lowercase().contains("reload"), "stdout:\n{text}");
}

#[test]
fn a_world_install_does_not_ask_for_a_reload() {
    // Same reasoning: `--world` refuses while the world is open, so by the
    // time it succeeds there is no live session to reload.
    let root = world_with_experiments(0);
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[backups]\ndir = {:?}\nkeep = 5\n", backups),
    )
    .unwrap();
    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .env("CONSTRUCT_CONFIG", &config)
        .args([
            "install",
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
        text.contains("Beta APIs on"),
        "the --world half must have run: {text}"
    );
    assert!(!text.to_lowercase().contains("reload"), "stdout:\n{text}");
}

#[test]
fn an_install_without_a_world_still_asks_for_a_reload() {
    // Nothing on this path checks whether a world is open, and a world
    // already running Construct keeps the old version until it is reloaded.
    let root = world_with_experiments(0);
    let addon = build_mcaddon_bytes();
    let (base, _server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--path", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.to_lowercase().contains("reload"), "stdout:\n{text}");
}

#[test]
fn merge_without_o_is_a_usage_error() {
    // §5: "--merge requires -n". Without it there is no single name to derive.
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
            // `world_with_construct`'s `db/` is a stub directory, never a real
            // LevelDB (see `copy_writes_bytes_into_the_destination_worlds_own_construct`
            // above) — resolving without `--source shared-pack` would fail opening
            // it before merge logic ever runs.
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

    // The written file is a real .mcstructure covering both pieces.
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
    // `a`'s origin [0,0,0] also triggers the unrelated "origin may be unset"
    // warning, so asserting the array is merely non-empty is satisfied by
    // that alone. Pin the overlap warning itself.
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
    // The degenerate case is useful: it re-encodes a structure through the
    // codec, and the identical-origin refusal must not fire on a single piece.
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

/// A minimal `.mcstructure` builder for CLI tests. `construct-core`'s test
/// support module is not reachable from this crate, so the few fields these
/// tests need are built here rather than shared.
struct SupportBuild {
    size: [i32; 3],
    origin: [i32; 3],
    layer0: Vec<i32>,
    layer1: Vec<i32>,
    name: String,
}

impl SupportBuild {
    fn bytes(&self) -> Vec<u8> {
        use std::collections::HashMap;
        let c = |pairs: Vec<(&str, nbtx::Value)>| {
            nbtx::Value::Compound(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect::<HashMap<_, _>>(),
            )
        };
        let ints = |v: &[i32]| nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect());
        let palette = c(vec![
            (
                "block_palette",
                nbtx::Value::List(vec![c(vec![
                    ("name", nbtx::Value::String(self.name.clone())),
                    ("states", c(vec![])),
                    ("version", nbtx::Value::Int(18163713)),
                ])]),
            ),
            ("block_position_data", c(vec![])),
        ]);
        let structure = c(vec![
            (
                "block_indices",
                nbtx::Value::List(vec![ints(&self.layer0), ints(&self.layer1)]),
            ),
            ("entities", nbtx::Value::List(vec![])),
            ("palette", c(vec![("default", palette)])),
        ]);
        nbtx::to_le_bytes(&c(vec![
            ("format_version", nbtx::Value::Int(1)),
            ("size", ints(&self.size)),
            ("structure", structure),
            ("structure_world_origin", ints(&self.origin)),
        ]))
        .unwrap()
    }
}

fn support_build(size: [i32; 3], origin: [i32; 3], name: &str) -> SupportBuild {
    let volume = (size[0] * size[1] * size[2]) as usize;
    SupportBuild {
        size,
        origin,
        layer0: vec![0; volume],
        layer1: vec![-1; volume],
        name: name.to_string(),
    }
}

fn merge_fixture(size: [i32; 3], origin: [i32; 3], name: &str) -> Vec<u8> {
    support_build(size, origin, name).bytes()
}

// ---------------------------------------------------------------------------
// Plural arity: `delete`, `copy`, and `import` each take N structures, the way
// `export` always has. Every one of them resolves the whole batch before it
// touches anything, so a bad name in the middle leaves the job untouched
// rather than half done.
// ---------------------------------------------------------------------------

/// The shared Construct's `structures/` directory inside a `world_with_construct`.
fn shared_structures(root: &std::path::Path) -> std::path::PathBuf {
    root.join("development_behavior_packs/Construct[BP]/structures")
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
    // The whole point of resolving the batch up front: `barn` is real and
    // would have been unlinked already if this walked the list one at a time.
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
    // No world was named, so there is none to report — and with `world` null
    // meaning "the shared copy and nothing else", each row's `source` is the
    // only place statement left to make.
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
    // The shape does not depend on the count — that is the whole reason for
    // moving to an array rather than switching between two payloads.
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

/// A bare destination world under `world_with_construct`'s root, with a
/// Construct copy of its own so writes land in a predictable place.
fn destination_with_construct(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let world = root.join("minecraftWorlds").join(name);
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), name).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let bp = world.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();
    world
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
    // The collision check runs over the whole plan before the first write,
    // the way `export`'s does — so `barn` must not land just because `silo`
    // is the one that collides.
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
    // One destination home per invocation, so these describe the command,
    // not any one row.
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

/// Where an `import --world Test` lands under `world_with_construct`: the
/// world's own structures pack, created on demand by the first write.
fn imported_into_test(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    root.join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures")
        .join(format!("{name}.mcstructure"))
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
    // 1, not 2: the batch parses fine, the read is what fails.
    assert_eq!(out.status.code(), Some(1));

    assert!(
        !imported_into_test(root.path(), "barn").exists(),
        "nothing may be written when any file in the batch cannot be read"
    );
}

#[test]
fn import_refuses_two_files_that_would_derive_one_name() {
    // Distinct files collapsing onto a single structure name is silent data
    // loss — the second would land on the first. Caught before any write,
    // not discovered halfway through the batch.
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
    // `--name` renames a single import; it cannot name several, the same way
    // `export -n` cannot name several output files.
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
    // One destination home per invocation, so these describe the command.
    // `target` is spelled as a `--source` value, naming the place the files
    // landed in the same vocabulary `structures` and `export` read back.
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
fn completions_subcommand_generates_shell_scripts() {
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let out = bin().args(["completions", shell]).output().unwrap();
        assert!(
            out.status.success(),
            "completions {shell} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("construct"),
            "completions {shell} output missing binary name: {text}"
        );
    }
}

#[test]
fn tab_completion_completes_world_names() {
    // `export`'s world is a flag now, so this is where world names are asked
    // for. The value after `--world` is the one place `export` still wants one.
    let root = world_with_construct(&[("barn", b"x")]);
    let out = bin()
        .env("_CLAP_COMPLETE_INDEX", "5")
        .env("COMPLETE", "bash")
        .args([
            "--path",
            root.path().to_str().unwrap(),
            "--",
            "construct",
            "--path",
            root.path().to_str().unwrap(),
            "export",
            "--world",
            "",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Test"), "should suggest world Test:\n{text}");
}

#[test]
fn tab_completion_completes_structure_names_for_the_targeted_world() {
    // What `--world` can actually export: the world's database and the world's
    // own pack. The shared copy is out of scope, so offering its names here
    // would hand back a completion the command then refuses.
    //
    // The world must be one that genuinely *sees* both packs — a structures
    // pack of its own alongside the shared Construct. Give it a Construct of
    // its own instead and `pack::serving` hides the shared copy by itself,
    // which would let this pass without the scoping ever being exercised.
    let root = world_with_construct(&[("bomber", b"x")]);
    let src = root.path().join("hangar.mcstructure");
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
        .env("_CLAP_COMPLETE_INDEX", "6")
        .env("COMPLETE", "bash")
        .args([
            "--path",
            root.path().to_str().unwrap(),
            "--",
            "construct",
            "--path",
            root.path().to_str().unwrap(),
            "export",
            "--world",
            "Test",
            "",
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
        text.contains("hangar"),
        "should suggest the world's own pack structure hangar:\n{text}"
    );
    assert!(
        !text.contains("bomber"),
        "must not suggest a shared-copy structure --world cannot export:\n{text}"
    );
}

#[test]
fn tab_completion_completes_shared_structures_when_no_world_is_named() {
    // The other half of the grammar: with no `--world`, `export` reads the
    // shared copy, so that is what the names on offer must come from. A world's
    // database is not reachable from here and must not be suggested.
    let (root, _world_name) = fixture_world_with_construct(&[], &[("bomber", b"x")]);
    let out = bin()
        .env("_CLAP_COMPLETE_INDEX", "4")
        .env("COMPLETE", "bash")
        .args([
            "--path",
            root.path().to_str().unwrap(),
            "--",
            "construct",
            "--path",
            root.path().to_str().unwrap(),
            "export",
            "",
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
        text.contains("bomber"),
        "should suggest the shared copy's bomber:\n{text}"
    );
    assert!(
        !text.contains("house"),
        "must not suggest a world-database structure:\n{text}"
    );
}

#[test]
fn tab_completion_completes_source_world_structures_for_copy() {
    let root = world_with_construct(&[("barn", b"x")]);
    destination_with_construct(root.path(), "Other");

    let out = bin()
        .env("_CLAP_COMPLETE_INDEX", "6")
        .env("COMPLETE", "bash")
        .args([
            "--path",
            root.path().to_str().unwrap(),
            "--",
            "construct",
            "--path",
            root.path().to_str().unwrap(),
            "copy",
            "Test",
            "Other",
            "",
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
        text.contains("barn"),
        "should suggest source world structure barn:\n{text}"
    );
}

/// A path inside the structures folder `import --world Test` writes into,
/// given as the relative path the tree should mirror.
fn imported_tree_path(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
    root.join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures")
        .join(rel)
}

#[test]
fn import_of_a_directory_mirrors_its_tree_under_the_named_folder() {
    // The folder you name becomes a folder in the pack, so `Amelix/CF-Q1`
    // stays `Amelix/CF-Q1` — and nesting below it survives too.
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
    // A folder of structures routinely carries a README or a .DS_Store, and
    // naming those was never how they got into the batch.
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
    // Silently succeeding while writing nothing reads as "imported" — the
    // folder was almost certainly the wrong one.
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
    // `--name` renames a single import. A folder is a batch whose whole point
    // is the names it already carries, so there is nothing for one name to
    // rename — the same refusal `--name` gets for several files.
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

    // The loose file keeps the default namespace; the folder's file is nested.
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
    // The plan-then-write discipline has to survive expansion: a folder is
    // refused whole, never left half imported.
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
    // Folder names are not structure names: a space becomes `_`, exactly as a
    // file stem's does, so the tree is mirrored as closely as Construct's
    // naming rules allow.
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
    // Every file in a folder shares the folder's namespace, so the warning is
    // about the folder. Repeating it forty times buries the forty lines that
    // actually differ.
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
    // The per-structure lines are the ones worth repeating.
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("imported "))
            .count(),
        3,
        "{stdout}"
    );
}

#[test]
fn the_config_flag_names_the_file_that_is_read() {
    // The flag layer of "flag → env → file → discovery". Before it existed the
    // only way to name a config file was the environment.
    let tmp = tempfile::tempdir().unwrap();
    let com_mojang = tmp.path().join("games/com.mojang");
    std::fs::create_dir_all(com_mojang.join("minecraftWorlds")).unwrap();
    bare_world(&com_mojang.join("minecraftWorlds"), "AAAA", "Named By Flag");

    let config = tmp.path().join("elsewhere.toml");
    std::fs::write(
        &config,
        format!("[[roots]]\nname = \"cfg\"\npath = {:?}\n", com_mojang),
    )
    .unwrap();

    let out = bin()
        .args(["worlds", "--json", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let worlds = listed_worlds(&out);
    assert_eq!(worlds.len(), 1, "{worlds:?}");
    assert_eq!(worlds[0]["display_name"], "Named By Flag");
    assert_eq!(worlds[0]["qualified"], "cfg/AAAA");
}

#[test]
fn the_config_flag_beats_the_environment_variable() {
    // Precedence, checked in the one direction that can regress silently: both
    // are set, and each names a different root.
    let tmp = tempfile::tempdir().unwrap();

    let from_env = tmp.path().join("env/com.mojang");
    std::fs::create_dir_all(from_env.join("minecraftWorlds")).unwrap();
    bare_world(&from_env.join("minecraftWorlds"), "EEEE", "From The Env");
    let env_config = tmp.path().join("env.toml");
    std::fs::write(
        &env_config,
        format!("[[roots]]\nname = \"viaenv\"\npath = {:?}\n", from_env),
    )
    .unwrap();

    let from_flag = tmp.path().join("flag/com.mojang");
    std::fs::create_dir_all(from_flag.join("minecraftWorlds")).unwrap();
    bare_world(&from_flag.join("minecraftWorlds"), "FFFF", "From The Flag");
    let flag_config = tmp.path().join("flag.toml");
    std::fs::write(
        &flag_config,
        format!("[[roots]]\nname = \"viaflag\"\npath = {:?}\n", from_flag),
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_CONFIG", &env_config)
        .args([
            "worlds",
            "--json",
            "--config",
            flag_config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let worlds = listed_worlds(&out);
    assert_eq!(
        worlds.len(),
        1,
        "the env config must not also be read: {worlds:?}"
    );
    assert_eq!(worlds[0]["display_name"], "From The Flag");
    assert_eq!(worlds[0]["qualified"], "viaflag/FFFF");
}

#[test]
fn add_writes_to_the_file_the_config_flag_names() {
    // `add` writes the file every other command reads, so it has to resolve it
    // the same way. Writing to the default location under `--config` would
    // record a root nothing later reads.
    let tmp = tempfile::tempdir().unwrap();
    let com_mojang = tmp.path().join("games/com.mojang");
    std::fs::create_dir_all(&com_mojang).unwrap();
    let config = tmp.path().join("named.toml");

    let out = bin()
        .args([
            "add",
            com_mojang.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let settings: toml::Value = toml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        settings["roots"][0]["path"].as_str(),
        com_mojang.canonicalize().unwrap().to_str()
    );
}

#[test]
fn a_world_recorded_in_other_worlds_is_discovered() {
    // `add` files a lone save folder under `other_worlds`. That key is only
    // worth writing if discovery reads it back.
    let tmp = tempfile::tempdir().unwrap();
    let world = bare_world(tmp.path(), "Loose", "Loose World");
    let config = tmp.path().join("config.toml");
    std::fs::write(&config, format!("other_worlds = [{:?}]\n", world)).unwrap();

    let out = bin()
        .args(["worlds", "--json", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let worlds = listed_worlds(&out);
    assert_eq!(worlds.len(), 1, "{worlds:?}");
    assert_eq!(worlds[0]["display_name"], "Loose World");
    assert_eq!(worlds[0]["qualified"], "path/Loose");
}

#[test]
fn a_world_added_by_path_is_then_listed_without_repeating_the_path() {
    // The whole point of `construct add`: name the world once, and every later
    // command sees it. This is the round trip, `add` through to `worlds`.
    let tmp = tempfile::tempdir().unwrap();
    let world = bare_world(tmp.path(), "Saved", "Added Once");
    let config = tmp.path().join("config.toml");

    let out = bin()
        .args([
            "add",
            world.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = bin()
        .args(["worlds", "--json", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    let worlds = listed_worlds(&out);
    assert_eq!(
        worlds.len(),
        1,
        "add must make the world discoverable: {worlds:?}"
    );
    assert_eq!(worlds[0]["display_name"], "Added Once");
}

#[test]
fn no_environment_variable_can_choose_the_installation() {
    // `CONSTRUCT_INSTALLATION` used to settle this. It no longer does: with
    // two installations and no `default_installation`, the run is ambiguous
    // however the environment is set. `install` writes into the chosen
    // installation's pack root, so nothing invisible on the command line gets
    // to point it somewhere.
    let tmp = tempfile::tempdir().unwrap();
    for (dir, folder, name) in [("a", "W1", "Alpha"), ("b", "W2", "Beta")] {
        let root = tmp.path().join(dir).join("com.mojang");
        std::fs::create_dir_all(root.join("minecraftWorlds")).unwrap();
        bare_world(&root.join("minecraftWorlds"), folder, name);
    }
    let config = tmp.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[[roots]]\nname = \"alpha\"\npath = {:?}\n\n[[roots]]\nname = \"beta\"\npath = {:?}\n",
            tmp.path().join("a/com.mojang"),
            tmp.path().join("b/com.mojang"),
        ),
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_INSTALLATION", "beta")
        .args(["structures", "--json", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["error"]["kind"], "ambiguous-installation",
        "the environment must not settle it: {v}"
    );

    // The config file still does, and that is the only thing that does.
    std::fs::write(
        &config,
        format!(
            "default_installation = \"beta\"\n\n[[roots]]\nname = \"alpha\"\npath = {:?}\n\n[[roots]]\nname = \"beta\"\npath = {:?}\n",
            tmp.path().join("a/com.mojang"),
            tmp.path().join("b/com.mojang"),
        ),
    )
    .unwrap();
    let out = bin()
        .args(["structures", "--json", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_ne!(
        v["error"]["kind"], "ambiguous-installation",
        "default_installation must still settle it: {v}"
    );
}
