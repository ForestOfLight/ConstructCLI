//! `world_behavior_packs.json` and `world_resource_packs.json`.
//!
//! Flat arrays of `{ pack_id, version }`. The game matches development packs by
//! UUID and leaves a stale version in place across an upgrade, so the upsert
//! keys on `pack_id` alone. `world_behavior_pack_history.json` beside these is
//! Minecraft's own record and is never written.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRef {
    pub pack_id: String,
    pub version: [u32; 3],
}

pub fn behavior_path(world: &World) -> PathBuf {
    world.path.join("world_behavior_packs.json")
}

pub fn resource_path(world: &World) -> PathBuf {
    world.path.join("world_resource_packs.json")
}

pub fn read(path: &Path) -> Result<Vec<PackRef>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| CoreError::BadPack {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

/// Adds or replaces one entry, keyed on `pack_id`. Returns whether anything changed.
pub fn upsert(path: &Path, entry: PackRef) -> Result<bool> {
    let mut packs = read(path)?;
    match packs.iter_mut().find(|p| p.pack_id == entry.pack_id) {
        Some(existing) if *existing == entry => return Ok(false),
        Some(existing) => *existing = entry,
        None => packs.push(entry),
    }
    let text = serde_json::to_string_pretty(&packs).map_err(|e| CoreError::BadPack {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.construct-tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("world_packs.json")
    ));
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "\n[\n\t\n\t{\n\t\t\"pack_id\" : \"8c0c0153-d8b9-482a-889f-aef922b8fe58\",\n\t\t\"version\" : [ 1, 0, 0 ]\n\t}\n]";

    #[test]
    fn reads_a_file_as_minecraft_actually_writes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();

        let packs = read(&path).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_id, "8c0c0153-d8b9-482a-889f-aef922b8fe58");
        assert_eq!(packs[0].version, [1, 0, 0]);
    }

    #[test]
    fn a_missing_file_is_an_empty_list_not_an_error() {
        assert!(
            read(Path::new("/no/such/world_behavior_packs.json"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn upsert_adds_an_entry_to_a_world_that_had_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");

        assert!(
            upsert(
                &path,
                PackRef {
                    pack_id: "abc".into(),
                    version: [1, 2, 0]
                }
            )
            .unwrap()
        );
        assert_eq!(read(&path).unwrap()[0].pack_id, "abc");
    }

    #[test]
    fn upsert_replaces_a_stale_version_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();

        let changed = upsert(
            &path,
            PackRef {
                pack_id: "8c0c0153-d8b9-482a-889f-aef922b8fe58".into(),
                version: [1, 2, 0],
            },
        )
        .unwrap();
        assert!(changed);

        let packs = read(&path).unwrap();
        assert_eq!(packs.len(), 1, "replaced, not appended");
        assert_eq!(packs[0].version, [1, 2, 0]);
    }

    #[test]
    fn upsert_of_an_identical_entry_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();
        let before = std::fs::read(&path).unwrap();

        let changed = upsert(
            &path,
            PackRef {
                pack_id: "8c0c0153-d8b9-482a-889f-aef922b8fe58".into(),
                version: [1, 0, 0],
            },
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "an unchanged upsert must not rewrite the file"
        );
    }

    #[test]
    fn other_packs_survive_an_upsert() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(
            &path,
            r#"[{"pack_id":"other","version":[0,9,0]},{"pack_id":"another","version":[1,0,1]}]"#,
        )
        .unwrap();

        upsert(
            &path,
            PackRef {
                pack_id: "new".into(),
                version: [1, 2, 0],
            },
        )
        .unwrap();
        let ids: Vec<String> = read(&path)
            .unwrap()
            .into_iter()
            .map(|p| p.pack_id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "other".to_string(),
                "another".to_string(),
                "new".to_string()
            ]
        );
    }

    #[test]
    fn a_malformed_file_is_an_error_rather_than_being_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, "{ not an array").unwrap();
        assert!(read(&path).is_err());
        assert!(
            upsert(
                &path,
                PackRef {
                    pack_id: "x".into(),
                    version: [1, 0, 0]
                }
            )
            .is_err()
        );
    }
}
