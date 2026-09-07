use std::process::Command;

pub mod github;

pub fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_construct"));
    let empty = std::env::temp_dir().join("construct-empty-home");
    std::fs::create_dir_all(&empty).unwrap();
    c.env("HOME", &empty).env("USERPROFILE", &empty);
    c.env_remove("APPDATA").env_remove("LOCALAPPDATA");
    c.env("CONSTRUCT_DATA_DIR", &empty);
    c.env("CONSTRUCT_CONFIG", empty.join("no-such-config.toml"));
    c
}

pub fn bin_isolated(root: &std::path::Path) -> Command {
    let mut c = bin();
    c.env("CONSTRUCT_DATA_DIR", root.join("state"));
    c
}

pub fn bin_with_config(config: &std::path::Path) -> Command {
    let mut command = bin();
    command.env("CONSTRUCT_CONFIG", config);
    command
}

pub fn bare_world(parent: &std::path::Path, folder: &str, name: &str) -> std::path::PathBuf {
    let dir = parent.join(folder);
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(dir.join("level.dat"), b"x").unwrap();
    std::fs::write(dir.join("levelname.txt"), name).unwrap();
    dir
}

pub fn listed_worlds(out: &std::process::Output) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not JSON: {e}\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    v["worlds"].as_array().cloned().unwrap_or_default()
}

pub fn fixture_world() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let gz = std::fs::File::open("../construct-core/tests/fixtures/world.tar.gz")
        .expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(tmp.path())
        .unwrap();
    let world = tmp.path().join("test_level");
    (tmp, world)
}

pub fn fixture_world_with_extra_structures(
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
    }
    (tmp, world)
}

pub fn world_with_construct(structures: &[(&str, &[u8])]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
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

pub fn fixture_world_with_construct(
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
        let db = bedrock_level::db::Database::open(db_dir.to_str().unwrap()).unwrap();
        for (name, bytes) in extra_world_structures {
            let key = construct_core::store::key::encode(name);
            db.insert(&key, *bytes).unwrap();
        }
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

pub fn add_world_construct(world_dir: &std::path::Path, structures: &[(&str, &[u8])]) {
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

pub fn add_destination_world(worlds_dir: &std::path::Path, folder: &str) -> std::path::PathBuf {
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

pub fn world_seeing_one_name_in_both_packs() -> tempfile::TempDir {
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

pub fn shared_house(root: &std::path::Path) -> std::path::PathBuf {
    root.join("development_behavior_packs/Construct[BP]/structures/house.mcstructure")
}

pub fn world_house(root: &std::path::Path) -> std::path::PathBuf {
    root.join(
        "minecraftWorlds/Test/behavior_packs/ConstructStructures/structures/house.mcstructure",
    )
}

pub fn close_world(world: &std::path::Path) {
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

pub fn db_fingerprint(world: &std::path::Path) -> Vec<(String, u64)> {
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

pub fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
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

pub fn world_with_experiments(gametest: i8) -> tempfile::TempDir {
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

pub fn write_level_dat(path: &std::path::Path, gametest: i8) {
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

pub fn write_construct_bp(dir: &std::path::Path, version: [u32; 3]) {
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

#[must_use = "the world stops looking live as soon as this is dropped"]
pub struct LiveWorld {
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub writer: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LiveWorld {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

pub fn mark_db_active(root: &std::path::Path) -> LiveWorld {
    let db = root.join("minecraftWorlds/Test/db");
    live_world_at(&db)
}

pub fn live_world_at(db: &std::path::Path) -> LiveWorld {
    let log = db.join("000021.log");
    std::fs::write(&log, b"chunk data").unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let writer = std::thread::spawn(move || {
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

pub struct SupportBuild {
    pub size: [i32; 3],
    pub origin: [i32; 3],
    pub layer0: Vec<i32>,
    pub layer1: Vec<i32>,
    pub name: String,
}

impl SupportBuild {
    pub fn bytes(&self) -> Vec<u8> {
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

pub fn support_build(size: [i32; 3], origin: [i32; 3], name: &str) -> SupportBuild {
    let volume = (size[0] * size[1] * size[2]) as usize;
    SupportBuild {
        size,
        origin,
        layer0: vec![0; volume],
        layer1: vec![-1; volume],
        name: name.to_string(),
    }
}

pub fn merge_fixture(size: [i32; 3], origin: [i32; 3], name: &str) -> Vec<u8> {
    support_build(size, origin, name).bytes()
}

pub fn shared_structures(root: &std::path::Path) -> std::path::PathBuf {
    root.join("development_behavior_packs/Construct[BP]/structures")
}

pub fn destination_with_construct(root: &std::path::Path, name: &str) -> std::path::PathBuf {
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

pub fn imported_into_test(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    root.join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures")
        .join(format!("{name}.mcstructure"))
}

pub fn imported_tree_path(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
    root.join("minecraftWorlds/Test/behavior_packs/ConstructStructures/structures")
        .join(rel)
}
