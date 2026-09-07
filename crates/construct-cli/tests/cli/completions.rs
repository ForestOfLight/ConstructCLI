use crate::support::*;

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
