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
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("evil.mcaddon");
    make_addon(
        &addon,
        &[
            ("../escaped.txt", b"pwned"),
            ("BP/manifest.json", &manifest("x", "u", "data")),
        ],
    );

    assert!(mcaddon::extract(&addon).is_err());
    assert!(!tmp.path().join("escaped.txt").exists());
    assert!(!Path::new("../escaped.txt").exists());
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
