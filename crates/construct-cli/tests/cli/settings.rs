use crate::support::*;

#[test]
fn the_config_flag_names_the_file_that_is_read() {
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
