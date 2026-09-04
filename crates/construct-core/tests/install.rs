use construct_core::install;
use construct_core::install::mcaddon;
use std::io::Write;
use std::path::Path;

/// Builds a synthetic `.mcaddon` from (path, contents) pairs.
fn make_addon(at: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(at).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn manifest(name: &str, uuid: &str, module: &str) -> Vec<u8> {
    format!(
        r#"{{"format_version":2,
            "header":{{"name":"{name}","uuid":"{uuid}","version":[1,2,0]}},
            "modules":[{{"type":"{module}","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}}]}}"#
    )
    .into_bytes()
}

fn real_shaped_addon(at: &Path) {
    make_addon(
        at,
        &[
            (
                "Construct[BP]/manifest.json",
                &manifest(
                    "Construct [BP] v1.2.0",
                    construct_core::pack::CONSTRUCT_BP_UUID,
                    "data",
                ),
            ),
            ("Construct[BP]/scripts/main.js", b"// code"),
            ("Construct[BP]/structures/construct.mcstructure", b"shipped"),
            (
                "Construct[RP]/manifest.json",
                &manifest(
                    "Construct [RP] v1.2.0",
                    construct_core::pack::CONSTRUCT_RP_UUID,
                    "resources",
                ),
            ),
        ],
    );
}

#[test]
fn extract_finds_the_behaviour_and_resource_packs_by_module_type() {
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("Construct-v1.2.0.mcaddon");
    real_shaped_addon(&addon);

    let extracted = mcaddon::extract(&addon).unwrap();
    assert_eq!(extracted.behavior.file_name().unwrap(), "Construct[BP]");
    assert_eq!(extracted.resource.file_name().unwrap(), "Construct[RP]");
    assert_eq!(
        std::fs::read(extracted.behavior.join("structures/construct.mcstructure")).unwrap(),
        b"shipped"
    );
    assert!(extracted.behavior.join("scripts/main.js").is_file());
}

#[test]
fn an_entry_that_would_escape_the_directory_refuses_the_whole_archive() {
    // A *complete* archive — both packs present — so the only possible reason
    // for failure is the traversal entry, not a missing resource pack wearing
    // the same `BadPack` variant.
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("evil.mcaddon");
    complete_addon_plus(&addon, "../escaped.txt");

    let err = mcaddon::extract(&addon).unwrap_err();
    let construct_core::CoreError::BadPack { reason, .. } = &err else {
        panic!("expected BadPack, got {err:?}");
    };
    assert!(
        reason.contains("escape"),
        "expected a traversal refusal, got {reason:?}"
    );
}

#[test]
fn an_entry_with_an_absolute_path_writes_nothing_to_that_path() {
    // The canary lives in a directory the test itself owns and can observe —
    // unlike the temp directory `extract()` creates internally, which the
    // test never sees the path of.
    let canary = tempfile::tempdir().unwrap();
    let target = canary.path().join("PWNED");

    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("evil.mcaddon");
    let absolute_entry = format!("{}/PWNED", canary.path().display());
    complete_addon_plus(&addon, &absolute_entry);

    assert!(mcaddon::extract(&addon).is_err());
    assert!(!target.exists());
}

/// A complete, otherwise-valid archive plus one extra entry — so a rejection
/// can only be attributed to that entry, never to a missing pack.
fn complete_addon_plus(at: &Path, extra_name: &str) {
    make_addon(
        at,
        &[
            (extra_name, b"pwned"),
            (
                "Construct[BP]/manifest.json",
                &manifest(
                    "Construct [BP] v1.2.0",
                    construct_core::pack::CONSTRUCT_BP_UUID,
                    "data",
                ),
            ),
            (
                "Construct[RP]/manifest.json",
                &manifest(
                    "Construct [RP] v1.2.0",
                    construct_core::pack::CONSTRUCT_RP_UUID,
                    "resources",
                ),
            ),
        ],
    );
}

#[test]
fn hostile_entry_shapes_are_all_refused() {
    let hostile = [
        "../escaped.txt",
        "/etc/escaped.txt",
        "//escaped.txt",
        "BP\\..\\..\\escaped.txt",
        "C:\\escaped.txt",
        "Construct[BP]/../../escaped.txt",
    ];

    for name in hostile {
        let tmp = tempfile::tempdir().unwrap();
        let addon = tmp.path().join("evil.mcaddon");
        complete_addon_plus(&addon, name);

        assert!(
            mcaddon::extract(&addon).is_err(),
            "expected {name:?} to be refused"
        );
    }
}

#[test]
fn an_archive_with_no_resource_pack_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("half.mcaddon");
    make_addon(&addon, &[("BP/manifest.json", &manifest("x", "u", "data"))]);

    let err = mcaddon::extract(&addon).unwrap_err();
    assert!(
        matches!(err, construct_core::CoreError::BadPack { .. }),
        "got {err:?}"
    );
}

#[test]
fn folder_names_do_not_decide_which_pack_is_which() {
    // Swapped names, correct module types.
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("swapped.mcaddon");
    make_addon(
        &addon,
        &[
            ("looks_like_rp/manifest.json", &manifest("a", "u1", "data")),
            (
                "looks_like_bp/manifest.json",
                &manifest("b", "u2", "resources"),
            ),
        ],
    );
    let extracted = mcaddon::extract(&addon).unwrap();
    assert_eq!(extracted.behavior.file_name().unwrap(), "looks_like_rp");
    assert_eq!(extracted.resource.file_name().unwrap(), "looks_like_bp");
}

#[test]
fn a_file_that_is_not_a_zip_is_a_bad_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let not_zip = tmp.path().join("x.mcaddon");
    std::fs::write(&not_zip, b"definitely not a zip").unwrap();
    assert!(mcaddon::extract(&not_zip).is_err());
}

/// A pack directory on disk, as if previously installed.
fn installed_pack(
    root: &Path,
    folder: &str,
    uuid: &str,
    version: [u32; 3],
    structures: &[(&str, &[u8])],
) -> std::path::PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"format_version":2,"header":{{"name":"{folder}","uuid":"{uuid}","version":[{},{},{}]}},
                "modules":[{{"type":"data","uuid":"33333333-3333-3333-3333-333333333333","version":[1,0,0]}}]}}"#,
            version[0], version[1], version[2]
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("structures")).unwrap();
    for (name, bytes) in structures {
        std::fs::write(
            dir.join("structures").join(format!("{name}.mcstructure")),
            bytes,
        )
        .unwrap();
    }
    dir
}

#[test]
fn an_upgrade_keeps_every_imported_structure() {
    // The highest-priority test in the spec: this folder holds the user's data.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    installed_pack(
        &root,
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        &[
            ("bomber", b"mine"),
            ("castle", b"also mine"),
            ("construct", b"old shipped"),
        ],
    );

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[("construct", b"new shipped")],
    );
    std::fs::write(new.join("scripts.js"), b"v1.2.0").unwrap();

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.from, Some([1, 1, 0]));
    assert_eq!(placed.to, [1, 2, 0]);
    assert_eq!(
        placed.preserved, 2,
        "bomber and castle, not the shipped one"
    );
    assert!(placed.changed);

    let structures = placed.dir.join("structures");
    assert_eq!(
        std::fs::read(structures.join("bomber.mcstructure")).unwrap(),
        b"mine"
    );
    assert_eq!(
        std::fs::read(structures.join("castle.mcstructure")).unwrap(),
        b"also mine"
    );
    // A file the new version ships wins over the copy already there.
    assert_eq!(
        std::fs::read(structures.join("construct.mcstructure")).unwrap(),
        b"new shipped"
    );
    // And the new version's own files arrived.
    assert_eq!(
        std::fs::read(placed.dir.join("scripts.js")).unwrap(),
        b"v1.2.0"
    );
}

#[test]
fn a_renamed_folder_is_upgraded_in_place_not_installed_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    installed_pack(
        &root,
        "my-construct-copy",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        &[],
    );

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[],
    );

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.dir.file_name().unwrap(), "my-construct-copy");
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        1,
        "no second copy"
    );
}

#[test]
fn installing_the_version_already_present_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let existing = installed_pack(
        &root,
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[("keep", b"x")],
    );
    std::fs::write(existing.join("marker"), b"untouched").unwrap();

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[],
    );

    let placed = install::place(&root, &new, false).unwrap();
    assert!(!placed.changed);
    assert_eq!(placed.from, Some([1, 2, 0]));
    assert_eq!(
        std::fs::read(existing.join("marker")).unwrap(),
        b"untouched"
    );

    // --force reinstalls the same version, and still keeps the structures.
    let placed = install::place(&root, &new, true).unwrap();
    assert!(placed.changed);
    assert_eq!(placed.preserved, 1);
}

#[test]
#[cfg(unix)]
fn a_copy_failure_partway_through_leaves_the_original_completely_intact() {
    // The Critical finding this test exists for: a naive delete-then-copy
    // destroys the original before the copy is known to succeed, so a copy
    // failure (disk full, most plausibly) leaves the user with neither the
    // old pack nor the new one. This provokes a real failure partway through
    // staging the new pack — before the original is ever touched — and
    // asserts the original pack is exactly as it was: same manifest, same
    // version, every structure present with its original bytes.
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let existing = installed_pack(
        &root,
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        &[("bomber", b"mine"), ("castle", b"also mine")],
    );

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[],
    );
    // A subdirectory of the new pack that the recursive copy cannot read
    // into, so `copy_dir(src, &staging)` errors out partway through.
    let unreadable = new.join("scripts");
    std::fs::create_dir_all(&unreadable).unwrap();
    std::fs::write(unreadable.join("main.js"), b"// code").unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = install::place(&root, &new, false);

    // Restore permissions unconditionally so the tempdir can clean itself up
    // regardless of what the assertions below find.
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        result.is_err(),
        "expected the unreadable subdirectory to fail the staged copy"
    );

    // The original must be completely untouched: same manifest, same
    // version, and every user structure still there with its original
    // bytes.
    let manifest = construct_core::pack::manifest::read(&existing).unwrap();
    assert_eq!(manifest.version, [1, 1, 0]);
    assert_eq!(manifest.uuid, construct_core::pack::CONSTRUCT_BP_UUID);
    let structures = existing.join("structures");
    assert_eq!(
        std::fs::read(structures.join("bomber.mcstructure")).unwrap(),
        b"mine"
    );
    assert_eq!(
        std::fs::read(structures.join("castle.mcstructure")).unwrap(),
        b"also mine"
    );

    // And nothing was left behind that would corrupt a later run.
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".constructcli-staging-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "expected the failed stage to be cleaned up, found {leftovers:?}"
    );
}

#[test]
fn a_crash_between_remove_and_rename_recovers_on_the_next_run() {
    // Reconstructs the point-of-no-return state by hand: `dest` has already
    // been removed, and a staging directory sits beside it holding the
    // complete, already-merged replacement -- exactly what a crash between
    // `remove_dir_all` and `rename` leaves on disk. The next `place()` call,
    // with no idea a crash happened, must recover it rather than throw it
    // away: at this point it is the only place the user's structures exist.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");

    let staging_name = format!(
        ".constructcli-staging-{}-{}-Construct[BP]",
        std::process::id(),
        123456789u64
    );
    installed_pack(
        &root,
        &staging_name,
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[("bomber", b"mine"), ("castle", b"also mine")],
    );
    // `dest` (Construct[BP]) intentionally does not exist -- it was already
    // removed by the run that crashed before it could rename the stage in.

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[],
    );

    let placed = install::place(&root, &new, false).unwrap();

    assert_eq!(placed.dir, root.join("Construct[BP]"));
    assert!(
        placed.dir.is_dir(),
        "the staging directory should have been recovered into place"
    );
    let structures = placed.dir.join("structures");
    assert_eq!(
        std::fs::read(structures.join("bomber.mcstructure")).unwrap(),
        b"mine"
    );
    assert_eq!(
        std::fs::read(structures.join("castle.mcstructure")).unwrap(),
        b"also mine"
    );

    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".constructcli-staging-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "the recovered staging directory should not remain, found {leftovers:?}"
    );
}

#[test]
fn a_foreign_staging_directory_is_never_deleted_or_mistaken_for_the_installed_pack() {
    // A staging directory this invocation did not create: either another
    // process's stage still being written, or a leftover whose destination
    // exists again. It carries a fully readable manifest with the *same*
    // UUID as the real pack -- the exact shape that would be mistaken for
    // the installed copy if staging directories were not explicitly
    // excluded from the UUID lookup, since `.` sorts before the real pack's
    // name and `find_by_uuid` returns the first match.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let existing = installed_pack(
        &root,
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 1, 0],
        &[("bomber", b"mine")],
    );

    let foreign_name = format!(
        ".constructcli-staging-{}-{}-Construct[BP]",
        999999u32, 42u64
    );
    installed_pack(
        &root,
        &foreign_name,
        construct_core::pack::CONSTRUCT_BP_UUID,
        [9, 9, 9],
        &[("someone-elses-structure", b"not yours")],
    );

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[],
    );

    let placed = install::place(&root, &new, false).unwrap();

    // The upgrade found and used the *real* pack, not the foreign staging
    // directory -- its prior version and preserved structure prove it.
    assert_eq!(placed.from, Some([1, 1, 0]));
    assert_eq!(placed.dir, existing);
    assert_eq!(
        std::fs::read(placed.dir.join("structures/bomber.mcstructure")).unwrap(),
        b"mine"
    );

    // The foreign staging directory -- something this call did not create,
    // and whose destination already existed -- must still be exactly as it
    // was: never deleted, never recovered into place.
    let foreign = root.join(&foreign_name);
    assert!(
        foreign.is_dir(),
        "a staging directory this run did not create must never be removed"
    );
    assert_eq!(
        std::fs::read(foreign.join("structures/someone-elses-structure.mcstructure")).unwrap(),
        b"not yours"
    );
}

#[test]
fn a_first_install_creates_the_root_and_copies_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(
        new.parent().unwrap(),
        "Construct[BP]",
        construct_core::pack::CONSTRUCT_BP_UUID,
        [1, 2, 0],
        &[("construct", b"shipped")],
    );

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.from, None);
    assert_eq!(placed.preserved, 0);
    assert_eq!(placed.dir, root.join("Construct[BP]"));
    assert_eq!(
        std::fs::read(placed.dir.join("structures/construct.mcstructure")).unwrap(),
        b"shipped"
    );
}
