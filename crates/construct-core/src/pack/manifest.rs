//! `manifest.json`, read with a small local serde struct rather than
//! `bedrock_addon` — only the header UUID, the version, and whether a module is
//! `resources` matter here, and this keeps the git-dependency surface to
//! `bedrock_level`.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Behavior,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub uuid: String,
    pub version: [u32; 3],
    pub kind: PackKind,
}

#[derive(Deserialize)]
struct Raw {
    header: Header,
    #[serde(default)]
    modules: Vec<Module>,
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    name: String,
    uuid: String,
    version: Vec<u32>,
}

#[derive(Deserialize)]
struct Module {
    #[serde(default)]
    r#type: String,
}

/// `[1, 2, 0]` as `"1.2.0"`.
pub fn version_string(v: [u32; 3]) -> String {
    format!("{}.{}.{}", v[0], v[1], v[2])
}

pub fn parse(text: &str, path: &Path) -> Result<Manifest> {
    let bad = |reason: String| CoreError::BadPack {
        path: path.to_path_buf(),
        reason,
    };
    let raw: Raw = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
    let version: [u32; 3] = raw.header.version.as_slice().try_into().map_err(|_| {
        bad(format!(
            "header version is not three numbers: {:?}",
            raw.header.version
        ))
    })?;

    let kind = if raw.modules.iter().any(|m| m.r#type == "resources") {
        PackKind::Resource
    } else {
        PackKind::Behavior
    };

    Ok(Manifest {
        name: raw.header.name,
        uuid: raw.header.uuid,
        version,
        kind,
    })
}

pub fn read(pack_dir: &Path) -> Result<Manifest> {
    let path: PathBuf = pack_dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| CoreError::BadPack {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    parse(&text, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const BP: &str = r#"{
        "format_version": 2,
        "header": {
            "name": "Construct [BP] v1.2.0",
            "uuid": "8c0c0153-d8b9-482a-889f-aef922b8fe58",
            "min_engine_version": [1, 26, 40],
            "version": [1, 2, 0]
        },
        "modules": [
            { "type": "data", "uuid": "f4d52ae1-2c26-4938-b8c2-7e455d495620", "version": [1, 0, 0] },
            { "type": "script", "language": "javascript", "entry": "scripts/main.js",
              "uuid": "8bb9a1e4-0531-4b93-8ade-3880bfe1e2fc", "version": [1, 0, 0] }
        ],
        "dependencies": [ { "module_name": "@minecraft/server", "version": "2.10.0-beta" } ]
    }"#;

    const RP: &str = r#"{
        "format_version": 2,
        "header": {
            "name": "Construct [RP] v1.2.0",
            "uuid": "375ec465-3dc1-429f-8b4c-a337889e1ed4",
            "version": [1, 2, 0],
            "min_engine_version": [1, 26, 40]
        },
        "modules": [ { "type": "resources", "uuid": "7f6b23df-a583-476b-b0e4-87457e65f7c0", "version": [1, 0, 0] } ],
        "capabilities": [ "pbr" ]
    }"#;

    #[test]
    fn reads_the_real_behaviour_pack_manifest() {
        let m = parse(BP, Path::new("manifest.json")).unwrap();
        assert_eq!(m.uuid, "8c0c0153-d8b9-482a-889f-aef922b8fe58");
        assert_eq!(m.version, [1, 2, 0]);
        assert_eq!(m.kind, PackKind::Behavior);
        assert_eq!(m.name, "Construct [BP] v1.2.0");
    }

    #[test]
    fn a_module_of_type_resources_makes_it_a_resource_pack() {
        assert_eq!(
            parse(RP, Path::new("m.json")).unwrap().kind,
            PackKind::Resource
        );
    }

    #[test]
    fn a_dependency_module_name_does_not_break_parsing() {
        assert!(parse(BP, Path::new("m.json")).is_ok());
    }

    #[test]
    fn a_missing_header_is_a_bad_pack_not_a_panic() {
        let err = parse(r#"{"format_version": 2}"#, Path::new("m.json")).unwrap_err();
        assert!(matches!(err, CoreError::BadPack { .. }), "got {err:?}");
    }

    #[test]
    fn malformed_json_names_the_file() {
        let err = parse("{ not json", Path::new("/packs/Thing/manifest.json")).unwrap_err();
        let CoreError::BadPack { path, .. } = err else {
            panic!("expected BadPack")
        };
        assert_eq!(path, Path::new("/packs/Thing/manifest.json"));
    }

    #[test]
    fn a_two_part_version_is_rejected_rather_than_padded() {
        let text =
            r#"{"header":{"name":"x","uuid":"u","version":[1,2]},"modules":[{"type":"data"}]}"#;
        assert!(parse(text, Path::new("m.json")).is_err());
    }

    #[test]
    fn version_string_is_dotted() {
        assert_eq!(version_string([1, 2, 0]), "1.2.0");
    }
}
