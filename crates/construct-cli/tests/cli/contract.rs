use crate::support::*;

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
    assert_eq!(v["schema"], 1);
    assert!(v["warnings"].is_array());
}

#[test]
fn a_failure_without_json_still_prints_nothing_on_stdout() {
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
    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}

#[test]
fn a_read_only_command_refuses_the_force_flag() {
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
