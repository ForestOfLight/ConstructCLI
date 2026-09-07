use crate::support::github::*;
use crate::support::*;

#[test]
fn delete_refuses_a_world_that_looks_in_use_and_writes_nothing() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    let before = db_fingerprint(&world_dir);
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
    let after = db_fingerprint(&world_dir);
    for entry in &before {
        assert!(
            after.contains(entry),
            "a refused delete must not have opened the database: {entry:?} changed"
        );
    }
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
    let text = String::from_utf8_lossy(&second.stdout);
    assert!(!text.contains("watching up to"), "stdout:\n{text}");
}

#[test]
fn a_mark_from_our_own_write_does_not_wave_through_a_live_world() {
    let (root, world) = fixture_world_with_construct(&[], &[]);
    let world_dir = root.path().join("minecraftWorlds").join(world);
    close_world(&world_dir);

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
fn enable_beta_apis_refuses_as_in_use_when_minecraft_has_the_world_open() {
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
