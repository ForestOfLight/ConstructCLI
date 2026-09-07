use crate::support::*;

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
    let root = world_with_experiments(1);
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
