use construct_core::discovery::{Installation, LastPlayedSource, World};
use construct_core::pack;
use std::path::Path;

fn test_world(dir: &Path) -> World {
    World {
        installation: "test".into(),
        account: None,
        folder: dir.file_name().unwrap().to_string_lossy().into_owned(),
        display_name: "Test".into(),
        path: dir.to_path_buf(),
        last_played: None,
        last_played_source: LastPlayedSource::DirMtime,
        size_bytes: 0,
    }
}

fn test_installation(com_mojang: &Path) -> Installation {
    Installation {
        name: "test".into(),
        dev_pack_root: com_mojang.to_path_buf(),
        world_roots: Vec::new(),
    }
}

/// Writes a minimal pack directory and returns its path.
fn make_pack(
    root: &Path,
    folder: &str,
    uuid: &str,
    version: [u32; 3],
    resources: bool,
) -> std::path::PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let module = if resources { "resources" } else { "data" };
    let manifest = format!(
        r#"{{"format_version":2,
            "header":{{"name":"{folder}","uuid":"{uuid}","version":[{},{},{}]}},
            "modules":[{{"type":"{module}","uuid":"11111111-1111-1111-1111-111111111111","version":[1,0,0]}}]}}"#,
        version[0], version[1], version[2]
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    dir
}

#[test]
fn construct_is_found_by_uuid_under_any_folder_name() {
    let root = tempfile::tempdir().unwrap();
    make_pack(
        root.path(),
        "SomethingElse",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    make_pack(
        root.path(),
        "Canopy[BP]",
        "aaaaaaaa-0000-0000-0000-000000000000",
        [1, 0, 0],
        false,
    );

    let found =
        pack::find_by_uuid(root.path(), pack::CONSTRUCT_BP_UUID).expect("should find Construct");
    assert_eq!(found.dir.file_name().unwrap(), "SomethingElse");
    assert_eq!(found.manifest.version, [1, 2, 0]);
}

#[test]
fn a_directory_without_a_manifest_is_skipped_not_an_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("junk")).unwrap();
    std::fs::write(root.path().join("loose-file.txt"), "x").unwrap();
    make_pack(
        root.path(),
        "Real",
        pack::CONSTRUCT_BP_UUID,
        [1, 0, 0],
        false,
    );

    let packs = pack::packs_in(root.path());
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].manifest.uuid, pack::CONSTRUCT_BP_UUID);
}

#[test]
fn a_pack_with_a_broken_manifest_is_skipped_rather_than_failing_the_scan() {
    // One corrupt pack must not hide every other pack on the machine.
    let root = tempfile::tempdir().unwrap();
    let broken = root.path().join("Broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("manifest.json"), "{ not json").unwrap();
    make_pack(
        root.path(),
        "Good",
        pack::CONSTRUCT_BP_UUID,
        [1, 0, 0],
        false,
    );

    assert_eq!(pack::packs_in(root.path()).len(), 1);
}

#[test]
fn a_dotted_directory_is_never_seen_as_an_installed_pack() {
    // `install::place` stages a pack under a dotted directory name while
    // swapping it in, and that directory can carry a fully valid manifest
    // -- with the same header UUID as the pack it is staging -- for as
    // long as the swap is in flight or before the next run recovers or
    // abandons it. Every caller of `packs_in`/`find_by_uuid`, not just
    // `install`, must never mistake it for the installed copy.
    let root = tempfile::tempdir().unwrap();
    make_pack(
        root.path(),
        ".constructcli-staging-1234-5678-Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [9, 9, 9],
        false,
    );

    assert!(pack::packs_in(root.path()).is_empty());
    assert!(pack::find_by_uuid(root.path(), pack::CONSTRUCT_BP_UUID).is_none());
}

#[test]
fn a_missing_root_yields_no_packs() {
    assert!(pack::packs_in(Path::new("/no/such/root")).is_empty());
}

#[test]
fn the_pack_roots_are_the_documented_folder_names() {
    let base = Path::new("/com.mojang");
    assert_eq!(
        pack::shared_behavior_root(base),
        Path::new("/com.mojang/development_behavior_packs")
    );
    assert_eq!(
        pack::shared_resource_root(base),
        Path::new("/com.mojang/development_resource_packs")
    );
}

#[test]
fn the_stray_roots_are_the_non_development_siblings() {
    // Where a hand-installed Construct lands when it is dropped in the wrong
    // folder — the pair `install::adopt` rescues it from.
    let base = Path::new("/com.mojang");
    assert_eq!(
        pack::stray_behavior_root(base),
        Path::new("/com.mojang/behavior_packs")
    );
    assert_eq!(
        pack::stray_resource_root(base),
        Path::new("/com.mojang/resource_packs")
    );
}

// --- where a world's structures live ---

/// A world directory under `com_mojang/minecraftWorlds/`, so that
/// `world_behavior_root` and the shared root are the real two places.
fn world_in(com_mojang: &Path, folder: &str) -> World {
    let dir = com_mojang.join("minecraftWorlds").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    test_world(&dir)
}

#[test]
fn a_world_with_no_pack_of_its_own_has_no_home_yet() {
    // Not an error and not the shared copy: the shared copy serves every
    // world, so it can never be where one world's structures are written.
    let root = tempfile::tempdir().unwrap();
    make_pack(
        &root.path().join("development_behavior_packs"),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    let world = world_in(root.path(), "Test");
    assert!(pack::home(&world).is_none());
}

#[test]
fn the_structures_pack_is_the_home_when_there_is_one() {
    let root = tempfile::tempdir().unwrap();
    let world = world_in(root.path(), "Test");
    let created = pack::shell::create(&world, None).unwrap();

    let home = pack::home(&world).expect("the shell pack is a home");
    assert_eq!(home.kind, pack::HomeKind::WorldStructuresPack);
    assert_eq!(home.dir, created.dir);
    assert_eq!(
        home.kind.source(),
        construct_core::catalog::Source::WorldPack
    );
}

#[test]
fn a_worlds_own_construct_outranks_a_structures_pack() {
    // A world whose Construct is its own already keeps structures per-world;
    // writing into a second pack beside it would split them in two.
    let root = tempfile::tempdir().unwrap();
    let world = world_in(root.path(), "Test");
    pack::shell::create(&world, None).unwrap();
    let local = make_pack(
        &pack::world_behavior_root(&world),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );

    let home = pack::home(&world).expect("home");
    assert_eq!(home.kind, pack::HomeKind::WorldConstruct);
    assert_eq!(home.dir, local);
}

#[test]
fn a_world_on_the_shared_construct_is_served_by_it_and_by_its_own_pack() {
    let root = tempfile::tempdir().unwrap();
    let shared = make_pack(
        &root.path().join("development_behavior_packs"),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    let world = world_in(root.path(), "Test");
    let shell = pack::shell::create(&world, None).unwrap();

    let serving = pack::serving(&world, &test_installation(root.path()));
    assert_eq!(
        serving.iter().map(|h| h.kind).collect::<Vec<_>>(),
        vec![
            pack::HomeKind::SharedConstruct,
            pack::HomeKind::WorldStructuresPack
        ]
    );
    assert_eq!(serving[0].dir, shared);
    assert_eq!(serving[1].dir, shell.dir);
}

#[test]
fn a_worlds_own_construct_hides_the_shared_one_from_that_world() {
    // Both copies carry Construct's header UUID, so the game loads the
    // world's and never the shared one. Listing the shared copy's structures
    // for this world would name structures it cannot see.
    let root = tempfile::tempdir().unwrap();
    make_pack(
        &root.path().join("development_behavior_packs"),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    let world = world_in(root.path(), "Test");
    let local = make_pack(
        &pack::world_behavior_root(&world),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        false,
    );

    let serving = pack::serving(&world, &test_installation(root.path()));
    assert_eq!(
        serving.iter().map(|h| h.kind).collect::<Vec<_>>(),
        vec![pack::HomeKind::WorldConstruct]
    );
    assert_eq!(serving[0].dir, local);
}

#[test]
fn the_structures_pack_is_a_readable_behaviour_pack_with_a_structures_folder() {
    let root = tempfile::tempdir().unwrap();
    let world = world_in(root.path(), "Test");
    let icon_src = root.path().join("Construct[BP]");
    std::fs::create_dir_all(&icon_src).unwrap();
    std::fs::write(icon_src.join("pack_icon.png"), b"PNG-BYTES").unwrap();

    let created = pack::shell::create(&world, Some(&icon_src)).unwrap();
    assert_eq!(created.manifest.uuid, pack::shell::UUID);
    assert_eq!(created.manifest.name, pack::shell::NAME);
    assert!(structures::dir(&created.dir).is_dir());
    // The icon is copied from Construct so the two read as a pair in the
    // game's pack list.
    assert_eq!(
        std::fs::read(created.dir.join("pack_icon.png")).unwrap(),
        b"PNG-BYTES"
    );
    // And it is a *behaviour* pack: a resource pack here would be enabled in
    // the wrong list and load nothing.
    assert_eq!(
        created.manifest.kind,
        construct_core::pack::manifest::PackKind::Behavior
    );
}

#[test]
fn creating_a_structures_pack_twice_keeps_what_is_in_it() {
    // An interrupted run leaves a half-made pack; the next command repairs it
    // rather than needing a reinstall, and must not drop structures doing so.
    let root = tempfile::tempdir().unwrap();
    let world = world_in(root.path(), "Test");
    let first = pack::shell::create(&world, None).unwrap();
    std::fs::write(
        structures::dir(&first.dir).join("house.mcstructure"),
        b"bytes",
    )
    .unwrap();

    let again = pack::shell::create(&world, None).unwrap();
    assert_eq!(again.dir, first.dir);
    assert_eq!(
        std::fs::read(structures::dir(&first.dir).join("house.mcstructure")).unwrap(),
        b"bytes"
    );
}

use construct_core::pack::structures;

fn touch(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_file_directly_in_structures_is_mystructure_namespaced() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Construct[BP]");
    touch(&pack.join("structures/bomber.mcstructure"), b"12345");

    let found = structures::list(&pack);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "mystructure:bomber");
    assert_eq!(found[0].name, "bomber");
    assert_eq!(found[0].size_bytes, 5);
}

#[test]
fn a_subdirectory_supplies_the_namespace_with_its_case_intact() {
    // This folder name used to be lowercased on the way out, on the
    // assumption that Minecraft namespaces are lowercase. Nothing measured
    // supports that: the game stored `CanopyPlayers:players` in a local
    // world's database unaltered. Reporting an id that differs from the one
    // on disk would break `delete`, which addresses the file by that id.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Understudy");
    touch(
        &pack.join("structures/Understudy/players.mcstructure"),
        b"x",
    );

    let found = structures::list(&pack);
    assert_eq!(found[0].id, "Understudy:players");
    // A non-default namespace stays visible in the display name.
    assert_eq!(found[0].name, "Understudy:players");
}

#[test]
fn non_mcstructure_files_are_not_listed() {
    // This test used to also assert that `structures/a/b/deep.mcstructure` was
    // ignored as "too deep". Task 21's ruling: that half was retired, not edited
    // around, because `docs/bedrock-mcstructure-files.md` -- a local, untracked copy
    // of tryashtar's third-party `.mcstructure` documentation on GitHub -- documents
    // that exact shape as `a:b/deep` -- listing it is the point of the task, not a
    // regression to paper over. The flat and one-level rules this test also
    // used to brush against are now covered by
    // `depth_does_not_change_the_flat_or_one_level_rules`.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/readme.txt"), b"x");
    touch(&pack.join("structures/ok.mcstructure"), b"x");

    let found = structures::list(&pack);
    assert_eq!(
        found.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["mystructure:ok"]
    );
}

#[test]
fn a_pack_with_no_structures_folder_lists_nothing() {
    let root = tempfile::tempdir().unwrap();
    assert!(structures::list(&root.path().join("Empty")).is_empty());
}

#[test]
fn path_for_puts_the_default_namespace_flat_and_others_in_a_subdirectory() {
    let pack = Path::new("/p");
    assert_eq!(
        structures::path_for(pack, "house").unwrap(),
        Path::new("/p/structures/house.mcstructure")
    );
    assert_eq!(
        structures::path_for(pack, "mystructure:house").unwrap(),
        Path::new("/p/structures/house.mcstructure")
    );
    assert_eq!(
        structures::path_for(pack, "understudy:players").unwrap(),
        Path::new("/p/structures/understudy/players.mcstructure")
    );
}

#[test]
fn path_for_refuses_an_id_that_would_escape_the_pack() {
    for evil in [
        "../../etc/passwd",
        "a/b",
        "..",
        "ns:../x",
        "ns:",
        ":name",
        "C:\\x",
    ] {
        assert!(
            structures::path_for(Path::new("/p"), evil).is_err(),
            "{evil} should be refused"
        );
    }
}

#[test]
fn write_refuses_an_existing_file_unless_forced() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    structures::write(&pack, "house", b"first", false).unwrap();

    let err = structures::write(&pack, "house", b"second", false).unwrap_err();
    assert!(matches!(
        err,
        construct_core::CoreError::TargetExists { .. }
    ));
    // Untouched by the refusal.
    assert_eq!(
        std::fs::read(pack.join("structures/house.mcstructure")).unwrap(),
        b"first"
    );

    structures::write(&pack, "house", b"second", true).unwrap();
    assert_eq!(
        std::fs::read(pack.join("structures/house.mcstructure")).unwrap(),
        b"second"
    );
}

#[test]
fn write_creates_the_structures_folder_and_any_namespace_directory() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    let at = structures::write(&pack, "understudy:players", b"x", false).unwrap();
    assert_eq!(at, pack.join("structures/understudy/players.mcstructure"));
    assert!(at.is_file());
}

#[test]
fn a_name_with_capitals_is_accepted() {
    // Capitals are ordinary in real structure names: `10HzCounter` and
    // `CanopyPlayers:players` are both measured in local worlds. A pack write
    // that refused them could not take a copy of either.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    let at = structures::write(&pack, "10HzCounter", b"x", false).unwrap();
    assert_eq!(at, pack.join("structures/10HzCounter.mcstructure"));
    assert!(at.is_file());
}

#[test]
fn a_namespace_with_capitals_survives_the_round_trip() {
    // The namespace is a directory name on the way in and is read back off
    // the filesystem on the way out, so anything normalising one side and not
    // the other shows up here as an id that does not match what was written.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    structures::write(&pack, "CanopyPlayers:players", b"x", false).unwrap();

    let listed = structures::list(&pack);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].id, "CanopyPlayers:players");
    assert_eq!(listed[0].name, "CanopyPlayers:players");
}

#[test]
fn derive_name_keeps_case_and_maps_spaces() {
    assert_eq!(structures::derive_name("My House").unwrap(), "My_House");
    assert_eq!(
        structures::derive_name("tower-2.v1_a").unwrap(),
        "tower-2.v1_a"
    );
}

#[test]
fn derive_name_rejects_rather_than_mangles() {
    // A mangled name is one Construct will not list, so the user is told to
    // pass --name instead of being handed something silently different.
    for bad in ["café", "a/b", "what?", "", "  ", "..", "."] {
        assert!(
            structures::derive_name(bad).is_err(),
            "{bad:?} should be rejected"
        );
    }
}

use construct_core::catalog::{self, Source};

#[test]
fn pack_entries_carry_their_file_path() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Construct[BP]");
    touch(&pack.join("structures/bomber.mcstructure"), b"12345");

    let entries = catalog::from_pack(&pack, Source::WorldPack);
    assert_eq!(entries[0].source, Source::WorldPack);
    assert_eq!(entries[0].id, "mystructure:bomber");
    assert_eq!(
        entries[0].path.as_deref(),
        Some(pack.join("structures/bomber.mcstructure").as_path())
    );
}

#[test]
fn unify_interleaves_both_sources_by_name() {
    let world = vec![
        catalog::Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::WorldDb,
            size_bytes: 1,
            path: None,
        },
        catalog::Entry {
            name: "zebra".into(),
            id: "mystructure:zebra".into(),
            source: Source::WorldDb,
            size_bytes: 1,
            path: None,
        },
    ];
    let pack = vec![catalog::Entry {
        name: "barn".into(),
        id: "mystructure:barn".into(),
        source: Source::WorldPack,
        size_bytes: 1,
        path: Some(std::path::PathBuf::from("/p/structures/barn.mcstructure")),
    }];
    let all = catalog::unify(world, pack);
    assert_eq!(
        all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["barn", "house", "zebra"]
    );
}

#[test]
fn a_worlds_own_copy_of_construct_wins_over_the_shared_one() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    let shared = com_mojang.join("development_behavior_packs");
    make_pack(
        &shared,
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );

    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(world_dir.join("db")).unwrap();
    make_pack(
        &world_dir.join("behavior_packs"),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        false,
    );

    let world = test_world(&world_dir);
    let installation = test_installation(&com_mojang);

    let target = pack::for_world(&world, &installation).unwrap();
    assert_eq!(target.scope, pack::Scope::World);
    assert_eq!(target.pack.manifest.version, [1, 1, 0]);
    assert_eq!(target.also_at, Some(shared.join("Construct[BP]")));
}

#[test]
fn without_a_world_copy_the_shared_copy_is_used() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    make_pack(
        &com_mojang.join("development_behavior_packs"),
        "Construct[BP]",
        pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        false,
    );
    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world_dir).unwrap();

    let target = pack::for_world(&test_world(&world_dir), &test_installation(&com_mojang)).unwrap();
    assert_eq!(target.scope, pack::Scope::Shared);
    assert_eq!(target.also_at, None);
}

#[test]
fn no_construct_anywhere_says_where_it_looked() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world_dir).unwrap();

    let err =
        pack::for_world(&test_world(&world_dir), &test_installation(&com_mojang)).unwrap_err();
    let construct_core::CoreError::ConstructNotInstalled { searched } = err else {
        panic!("expected ConstructNotInstalled");
    };
    assert_eq!(
        searched.len(),
        2,
        "both the world copy and the shared root: {searched:?}"
    );
}

#[test]
fn a_structure_nested_below_the_namespace_folder_is_addressable() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(
        &pack.join("structures/stuff/towers/diamond.mcstructure"),
        b"x",
    );

    let found = structures::list(&pack);
    assert_eq!(found.len(), 1);
    // First subfolder is the namespace; everything after it is part of the name.
    assert_eq!(found[0].id, "stuff:towers/diamond");
    assert_eq!(found[0].name, "stuff:towers/diamond");
}

#[test]
fn depth_does_not_change_the_flat_or_one_level_rules() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/house.mcstructure"), b"x");
    touch(
        &pack.join("structures/Understudy/players.mcstructure"),
        b"x",
    );
    touch(&pack.join("structures/a/b/c/d.mcstructure"), b"x");

    let ids: Vec<String> = structures::list(&pack).into_iter().map(|s| s.id).collect();
    assert!(ids.contains(&"mystructure:house".to_string()));
    assert!(ids.contains(&"Understudy:players".to_string()));
    assert!(ids.contains(&"a:b/c/d".to_string()));
}

#[test]
fn every_segment_is_left_exactly_as_it_sits_on_disk() {
    // Namespace, intermediate folders, and stem alike: the id is what the
    // filesystem says, so what `structures` prints is what `delete` can address.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(
        &pack.join("structures/Stuff/Towers/Diamond.mcstructure"),
        b"x",
    );
    assert_eq!(structures::list(&pack)[0].id, "Stuff:Towers/Diamond");
}

#[test]
fn a_derived_name_is_still_a_single_segment() {
    // `path_for` now writes the depth `list` reads, but depth comes from real
    // directories, never from a string someone typed: a file stem or a
    // `--name` is one segment, so the separator that would make traversal
    // possible cannot enter that way.
    assert!(structures::derive_name("towers/diamond").is_err());
    assert!(structures::derive_name("../x").is_err());
}

#[test]
fn path_for_nests_a_name_that_carries_separators() {
    // Symmetry with `list`: a pack holding `structures/Stuff/Towers/Diamond`
    // reports `Stuff:Towers/Diamond`, so that id has to be one this tool can
    // write back — otherwise `import` and `copy` cannot round-trip a tree
    // `structures` just printed.
    assert_eq!(
        structures::path_for(Path::new("/p"), "stuff:towers/diamond").unwrap(),
        Path::new("/p/structures/stuff/towers/diamond.mcstructure")
    );
    assert_eq!(
        structures::path_for(Path::new("/p"), "a:b/c/d").unwrap(),
        Path::new("/p/structures/a/b/c/d.mcstructure")
    );
}

#[test]
fn path_for_validates_every_segment_of_a_nested_name() {
    // Depth is not an escape hatch: each segment faces the same check the
    // single-segment name always did, so traversal is refused at any depth.
    for evil in [
        "ns:a/../b",
        "ns:a/./b",
        "ns:a//b",
        "ns:a/",
        "ns:/a",
        "ns:a/b c/d",
    ] {
        assert!(
            structures::path_for(Path::new("/p"), evil).is_err(),
            "{evil} should be refused"
        );
    }
}

#[test]
fn path_for_refuses_a_nested_name_in_the_default_namespace() {
    // `mystructure` is the one namespace with no folder of its own, so
    // `mystructure:a/b` would write `structures/a/b` — which `list` reads back
    // as `a:b`. Refused rather than silently filed under another namespace.
    assert!(structures::path_for(Path::new("/p"), "mystructure:a/b").is_err());
    assert!(structures::path_for(Path::new("/p"), "a/b").is_err());
}
