//! Reading `level.dat`, which is little-endian NBT behind an 8-byte header.

use crate::error::{CoreError, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// A parsed `level.dat`.
///
/// The root is kept as a generic [`nbtx::Value`] rather than a typed struct so
/// that keys this tool does not understand survive a read. Stage 2 rewrites the
/// `experiments` compound in place, and a wholesale replacement would silently
/// disable whatever else the world had enabled.
#[derive(Debug, Clone)]
pub struct LevelDat {
    pub version: i32,
    pub root: nbtx::Value,
}

/// Reads and parses the `level.dat` at `path`.
pub fn read(path: &Path) -> Result<LevelDat> {
    let bytes = std::fs::read(path)?;
    parse(&bytes, path)
}

/// Parses `level.dat` bytes. Separated from [`read`] so tests need no files.
pub fn parse(bytes: &[u8], path: &Path) -> Result<LevelDat> {
    let bad = |reason: &str| CoreError::BadLevelDat {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };

    if bytes.len() < 8 {
        return Err(bad("shorter than the 8-byte header"));
    }
    let version = i32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes"));
    let declared = i32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
    let declared = usize::try_from(declared).map_err(|_| bad("negative payload length"))?;

    let payload = bytes
        .get(8..8 + declared)
        .ok_or_else(|| bad("declared payload length runs past the end of the file"))?;

    let mut cursor: &[u8] = payload;
    let root: nbtx::Value =
        nbtx::from_le_bytes(&mut cursor).map_err(|e| bad(&format!("invalid NBT: {e}")))?;

    Ok(LevelDat { version, root })
}

impl LevelDat {
    fn field(&self, name: &str) -> Option<&nbtx::Value> {
        match &self.root {
            nbtx::Value::Compound(map) => map.get(name),
            _ => None,
        }
    }

    /// Unix seconds of the last session, if the field is present and a Long.
    pub fn last_played(&self) -> Option<i64> {
        match self.field("LastPlayed") {
            Some(nbtx::Value::Long(v)) => Some(*v),
            _ => None,
        }
    }

    /// The `experiments` compound's byte flags, or `None` when the world has no
    /// such compound. Ordered so callers render it deterministically.
    pub fn experiments(&self) -> Option<BTreeMap<String, i8>> {
        let nbtx::Value::Compound(map) = self.field("experiments")? else {
            return None;
        };
        Some(
            map.iter()
                .filter_map(|(k, v)| match v {
                    nbtx::Value::Byte(b) => Some((k.clone(), *b)),
                    _ => None,
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CoreError;
    use std::collections::HashMap;
    use std::path::Path;

    /// Build a level.dat: 8-byte header then a little-endian NBT compound.
    fn build(version: i32, root: nbtx::Value) -> Vec<u8> {
        let payload = nbtx::to_le_bytes(&root).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn compound(pairs: Vec<(&str, nbtx::Value)>) -> nbtx::Value {
        nbtx::Value::Compound(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<HashMap<_, _>>(),
        )
    }

    #[test]
    fn reads_last_played() {
        let bytes = build(
            10,
            compound(vec![("LastPlayed", nbtx::Value::Long(1741501762))]),
        );
        let dat = parse(&bytes, Path::new("x")).unwrap();
        assert_eq!(dat.version, 10);
        assert_eq!(dat.last_played(), Some(1741501762));
    }

    #[test]
    fn missing_last_played_is_none() {
        let bytes = build(10, compound(vec![("SomethingElse", nbtx::Value::Int(1))]));
        assert_eq!(parse(&bytes, Path::new("x")).unwrap().last_played(), None);
    }

    #[test]
    fn reads_all_three_experiment_flags() {
        // These are the exact keys and values measured in Amelix CMP/level.dat.
        let experiments = compound(vec![
            ("experiments_ever_used", nbtx::Value::Byte(1)),
            ("gametest", nbtx::Value::Byte(1)),
            ("saved_with_toggled_experiments", nbtx::Value::Byte(1)),
        ]);
        let bytes = build(10, compound(vec![("experiments", experiments)]));
        let got = parse(&bytes, Path::new("x"))
            .unwrap()
            .experiments()
            .unwrap();
        assert_eq!(got.get("gametest"), Some(&1));
        assert_eq!(got.get("experiments_ever_used"), Some(&1));
        assert_eq!(got.get("saved_with_toggled_experiments"), Some(&1));
    }

    #[test]
    fn preserves_unrelated_experiment_siblings() {
        // Stage 2 rewrites this compound. Anything it cannot see, it will destroy.
        let experiments = compound(vec![
            ("gametest", nbtx::Value::Byte(1)),
            ("data_driven_biomes", nbtx::Value::Byte(1)),
        ]);
        let bytes = build(10, compound(vec![("experiments", experiments)]));
        let got = parse(&bytes, Path::new("x"))
            .unwrap()
            .experiments()
            .unwrap();
        assert_eq!(got.get("data_driven_biomes"), Some(&1));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn no_experiments_compound_is_none() {
        let bytes = build(10, compound(vec![("LastPlayed", nbtx::Value::Long(1))]));
        assert_eq!(parse(&bytes, Path::new("x")).unwrap().experiments(), None);
    }

    #[test]
    fn rejects_a_file_too_short_for_the_header() {
        let err = parse(&[0u8; 4], Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    #[test]
    fn rejects_a_declared_length_longer_than_the_file() {
        let mut bytes = build(10, compound(vec![("LastPlayed", nbtx::Value::Long(1))]));
        bytes[4..8].copy_from_slice(&9999i32.to_le_bytes());
        let err = parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    #[test]
    fn parses_a_real_world_level_dat() {
        let path = dirs_next_to_home(
            "Library/Application Support/mcpelauncher/games/com.mojang/minecraftWorlds/Amelix CMP/level.dat",
        );
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skipping: no local world at {}", path.display());
            return;
        };
        let dat = parse(&bytes, &path).unwrap();
        assert_eq!(dat.version, 10);
        assert_eq!(dat.last_played(), Some(1741501762));
        let exp = dat
            .experiments()
            .expect("Amelix CMP has an experiments compound");
        assert_eq!(exp.get("gametest"), Some(&1));
        assert_eq!(exp.get("experiments_ever_used"), Some(&1));
        assert_eq!(exp.get("saved_with_toggled_experiments"), Some(&1));
    }

    fn dirs_next_to_home(rel: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rel)
    }
}
