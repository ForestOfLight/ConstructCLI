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
