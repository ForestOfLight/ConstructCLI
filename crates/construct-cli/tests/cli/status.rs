use crate::support::github::*;
use crate::support::*;

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
