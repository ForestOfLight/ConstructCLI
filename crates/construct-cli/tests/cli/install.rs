use crate::support::github::*;
use crate::support::*;

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
    write_level_dat(&world.join("level.dat"), 0);

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

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("construct enable-beta-apis Test"),
        "expected the manual recovery command in stderr: {stderr}"
    );
}

#[test]
fn install_reports_partial_with_the_packs_already_placed_when_the_world_pack_list_is_malformed() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0);
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

#[test]
fn install_world_warns_when_a_world_construct_copy_shadows_the_shared_copy() {
    let root = tempfile::tempdir().unwrap();
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

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("shadows") && stderr.contains(&local_bp.display().to_string()),
        "expected a warning naming the shadowing world copy: {stderr}"
    );

    let shared_manifest = std::fs::read_to_string(
        root.path()
            .join("development_behavior_packs/Construct[BP]/manifest.json"),
    )
    .unwrap();
    assert!(shared_manifest.contains("1, 2, 0") || shared_manifest.contains("[1,2,0]"));
    let local_manifest = std::fs::read_to_string(local_bp.join("manifest.json")).unwrap();
    assert!(local_manifest.contains("1.1.0"));
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
    let enabled = std::fs::read_to_string(world.join("world_behavior_packs.json")).unwrap();
    assert!(
        enabled.contains("9f7d83af-309e-4997-840e-e9c350435e83"),
        "structures pack not enabled: {enabled}"
    );
}

#[test]
fn install_world_gives_no_structures_pack_to_a_world_that_has_its_own_construct() {
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
