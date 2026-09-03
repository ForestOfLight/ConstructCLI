//! Reading `level.dat`, which is little-endian NBT behind an 8-byte header.

use crate::error::{CoreError, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// The payload length this file was read with.
    payload_len: usize,
    /// Whether re-serializing the untouched value reproduces a payload of the
    /// same length. See §9: `nbtx` turns array tags into lists (one byte longer
    /// each) and writes empty lists five bytes short and unparseable. Both
    /// change the length, so this is a reliable refusal rather than a guess.
    faithful: bool,
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
    let version = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let declared = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let declared = usize::try_from(declared).map_err(|_| bad("negative payload length"))?;

    let payload = bytes
        .get(8..8 + declared)
        .ok_or_else(|| bad("declared payload length runs past the end of the file"))?;

    let mut cursor: &[u8] = payload;
    let root: nbtx::Value =
        nbtx::from_le_bytes(&mut cursor).map_err(|e| bad(&format!("invalid NBT: {e}")))?;

    // The fidelity gate (§9, §10): re-serialize the untouched value and check
    // its length against what we just read. `nbtx` cannot round-trip array
    // tags or empty lists, and both defects change the payload's length, so a
    // mismatch here reliably means a write would corrupt this file.
    let faithful = nbtx::to_le_bytes(&root)
        .map(|re| re.len() == payload.len())
        .unwrap_or(false);

    Ok(LevelDat {
        version,
        root,
        payload_len: payload.len(),
        faithful,
    })
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
    ///
    /// # Warning
    ///
    /// This returns a read-only projection filtered to `Byte` values. Any non-`Byte`
    /// sibling entries in the `experiments` compound are excluded from the result.
    /// This is safe for display and inspection today because `self.root` retains
    /// every original entry unchanged.
    ///
    /// **Do NOT use this projection to reconstruct the compound for writing.** Stage 2
    /// will rewrite `experiments` in place by mutating `self.root`. If you reconstruct
    /// the compound from this projection, you will silently drop any non-`Byte` sibling,
    /// disabling whatever else the world had enabled. Stage 2 mutates in place instead.
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

    /// The payload length this file was read with, and what [`Self::is_faithful`]
    /// was measured against.
    pub fn payload_len(&self) -> usize {
        self.payload_len
    }

    /// False when this file's NBT cannot be re-serialized without changing —
    /// an array tag or an empty list (§9). [`Self::to_bytes`] refuses to write
    /// such a file rather than silently corrupt it.
    pub fn is_faithful(&self) -> bool {
        self.faithful
    }

    /// Whether Beta APIs are on, or `None` when the world has no `experiments`.
    pub fn beta_apis(&self) -> Option<bool> {
        Some(self.experiments()?.get(GAMETEST).copied().unwrap_or(0) != 0)
    }

    /// Sets the Beta APIs state, creating the compound if the world has none.
    ///
    /// Read-modify-write, never a replacement: worlds carry other experiment
    /// keys and rewriting the compound wholesale would silently disable them.
    /// Disabling leaves the two companion flags at 1 — they record that the
    /// world once used experiments rather than mirroring the current state.
    pub fn set_beta_apis(&mut self, on: bool) {
        let nbtx::Value::Compound(root) = &mut self.root else {
            return;
        };
        let entry = root
            .entry("experiments".to_string())
            .or_insert_with(|| nbtx::Value::Compound(Default::default()));
        let nbtx::Value::Compound(experiments) = entry else {
            return;
        };
        experiments.insert(GAMETEST.to_string(), nbtx::Value::Byte(on as i8));
        if on {
            experiments.insert(EVER_USED.to_string(), nbtx::Value::Byte(1));
            experiments.insert(TOGGLED.to_string(), nbtx::Value::Byte(1));
        }
    }

    /// The complete file: the 8-byte header, then the payload.
    ///
    /// Refuses when [`Self::is_faithful`] is false — see §9/§10 for why a
    /// length-preserving round-trip is the property that stands between this
    /// tool and a corrupted save.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if !self.faithful {
            return Err(CoreError::UnwritableLevelDat {
                path: PathBuf::new(),
                reason: "this level.dat uses NBT this tool cannot rewrite without changing it \
                         (an array tag or an empty list); the world was not modified"
                    .to_string(),
            });
        }
        let payload = nbtx::to_le_bytes(&self.root).map_err(|e| CoreError::UnwritableLevelDat {
            path: PathBuf::new(),
            reason: format!("could not encode NBT: {e}"),
        })?;
        let mut out = Vec::with_capacity(payload.len() + 8);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }
}

/// The three `Byte` entries that make up the Beta APIs state (§10).
const GAMETEST: &str = "gametest";
const EVER_USED: &str = "experiments_ever_used";
const TOGGLED: &str = "saved_with_toggled_experiments";

/// Writes a `level.dat` atomically: a temporary file beside it, then a rename.
///
/// `to_bytes` (and therefore the fidelity gate) runs before anything on disk
/// is touched, so a refusal here leaves `path` exactly as it was.
pub fn write(dat: &LevelDat, path: &Path) -> Result<()> {
    let bytes = dat.to_bytes().map_err(|e| match e {
        // `to_bytes` has no path to name; supply it here.
        CoreError::UnwritableLevelDat { reason, .. } => CoreError::UnwritableLevelDat {
            path: path.to_path_buf(),
            reason,
        },
        other => other,
    })?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.construct-tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("level.dat")
    ));
    std::fs::write(&tmp, &bytes)?;
    // Same directory, so the rename is atomic on every platform we target.
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetaApisChange {
    pub before: Option<bool>,
    pub after: bool,
    pub changed: bool,
}

/// Reads, flips, writes, then re-reads and verifies.
///
/// §10: treat "write succeeded, value unchanged" as a failure. Backing the file
/// up first is the caller's job — it needs configuration this module does not
/// have.
pub fn apply_beta_apis(path: &Path, on: bool) -> Result<BetaApisChange> {
    let mut dat = read(path)?;
    let before = dat.beta_apis();
    if before == Some(on) {
        return Ok(BetaApisChange {
            before,
            after: on,
            changed: false,
        });
    }
    dat.set_beta_apis(on);
    write(&dat, path)?;

    let verified = read(path)?;
    if verified.beta_apis() != Some(on) {
        return Err(CoreError::UnwritableLevelDat {
            path: path.to_path_buf(),
            reason: format!(
                "wrote the Beta APIs flag but read back {:?}",
                verified.beta_apis()
            ),
        });
    }
    Ok(BetaApisChange {
        before,
        after: on,
        changed: true,
    })
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

    #[test]
    fn rejects_garbage_payload_bytes() {
        // Build a valid header but with random garbage as the NBT payload.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&10i32.to_le_bytes()); // version
        bytes.extend_from_slice(&64i32.to_le_bytes()); // payload length
        bytes.extend_from_slice(&[0xff; 64]); // garbage payload
        let err = parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    #[test]
    fn rejects_truncated_tag_compound_in_mid_name() {
        // Build a valid header with a TAG_Compound (0x0a) opener but truncate mid-name.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&10i32.to_le_bytes()); // version
        bytes.extend_from_slice(&3i32.to_le_bytes()); // payload length = 3 bytes
        bytes.push(0x0a); // TAG_Compound
        bytes.push(0x00); // name length (high byte of u16)
        bytes.push(0x05); // name length (low byte), so 5 bytes expected but not provided
        // truncated here — only 3 bytes total
        let err = parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    #[test]
    fn rejects_invalid_tag_id() {
        // Build a valid header with an invalid/unknown tag ID.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&10i32.to_le_bytes()); // version
        bytes.extend_from_slice(&2i32.to_le_bytes()); // payload length
        bytes.push(0xFF); // invalid tag ID
        bytes.push(0x00); // 1 more byte to fill the declared length
        let err = parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    fn experiments(entries: &[(&str, i8)]) -> nbtx::Value {
        let mut inner = HashMap::new();
        for (k, v) in entries {
            inner.insert(k.to_string(), nbtx::Value::Byte(*v));
        }
        let mut root = HashMap::new();
        root.insert("experiments".to_string(), nbtx::Value::Compound(inner));
        root.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));
        nbtx::Value::Compound(root)
    }

    #[test]
    fn enabling_creates_the_compound_when_a_world_has_none() {
        let mut root = HashMap::new();
        root.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));
        let bytes = build(10, nbtx::Value::Compound(root));
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();

        assert_eq!(dat.beta_apis(), None);
        dat.set_beta_apis(true);
        assert_eq!(dat.experiments().unwrap().get("gametest"), Some(&1));
        assert_eq!(
            dat.experiments().unwrap().get("experiments_ever_used"),
            Some(&1)
        );
        assert_eq!(
            dat.experiments()
                .unwrap()
                .get("saved_with_toggled_experiments"),
            Some(&1)
        );
    }

    #[test]
    fn enabling_a_world_that_already_has_it_changes_nothing() {
        let bytes = build(
            10,
            experiments(&[
                ("experiments_ever_used", 1),
                ("gametest", 1),
                ("saved_with_toggled_experiments", 1),
            ]),
        );
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        let before = dat.experiments().unwrap();
        dat.set_beta_apis(true);
        assert_eq!(dat.experiments().unwrap(), before);
    }

    #[test]
    fn disabling_clears_only_gametest() {
        let bytes = build(
            10,
            experiments(&[
                ("experiments_ever_used", 1),
                ("gametest", 1),
                ("saved_with_toggled_experiments", 1),
            ]),
        );
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        dat.set_beta_apis(false);

        let after = dat.experiments().unwrap();
        assert_eq!(after.get("gametest"), Some(&0));
        // Historical records of the world having used experiments, not mirrors
        // of the current state.
        assert_eq!(after.get("experiments_ever_used"), Some(&1));
        assert_eq!(after.get("saved_with_toggled_experiments"), Some(&1));
    }

    #[test]
    fn unrelated_experiments_survive_the_flip() {
        // A wholesale rewrite of the compound would silently disable these.
        let bytes = build(
            10,
            experiments(&[
                ("data_driven_biomes", 1),
                ("upcoming_creator_features", 1),
                ("gametest", 0),
            ]),
        );
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        dat.set_beta_apis(true);

        let after = dat.experiments().unwrap();
        assert_eq!(after.get("data_driven_biomes"), Some(&1));
        assert_eq!(after.get("upcoming_creator_features"), Some(&1));
        assert_eq!(after.get("gametest"), Some(&1));
    }

    #[test]
    fn a_file_that_cannot_round_trip_refuses_to_be_written() {
        // nbtx 3.0.1 drops the element type and length of an empty list, five
        // bytes short, producing NBT it cannot parse back (§9). The `build`
        // helper cannot construct this fixture: it round-trips through
        // `nbtx::to_le_bytes`, which is exactly what can't serialize an empty
        // list. So these bytes are hand-written, not built. Verified by a
        // scratch probe: they parse to Compound({"gaps": List([])}) and
        // re-serialize to 11 bytes against an original of 16.
        //
        // { "gaps": List([]) }
        let payload: Vec<u8> = vec![
            0x0a, 0x00, 0x00, // TAG_Compound, root name ""
            0x09, // TAG_List
            0x04, 0x00, b'g', b'a', b'p', b's', // name "gaps"
            0x00, // element type TAG_End
            0x00, 0x00, 0x00, 0x00, // length 0
            0x00, // TAG_End of compound
        ];
        let mut bytes = 10i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        let dat = parse(&bytes, Path::new("level.dat")).unwrap();

        assert!(!dat.is_faithful());
        assert!(matches!(
            dat.to_bytes(),
            Err(CoreError::UnwritableLevelDat { .. })
        ));

        // A gate that returns an error but writes anyway is as dangerous as no
        // gate at all: the write must refuse, and the file on disk must be
        // untouched, byte for byte.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let err = write(&dat, &path).unwrap_err();
        assert!(matches!(err, CoreError::UnwritableLevelDat { .. }));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn an_ordinary_file_round_trips_and_reports_the_new_state() {
        let bytes = build(10, experiments(&[("gametest", 0)]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let change = apply_beta_apis(&path, true).unwrap();
        assert_eq!(change.before, Some(false));
        assert!(change.after);
        assert!(change.changed);

        // The written file is a real level.dat: it parses, and the value took.
        let reread = read(&path).unwrap();
        assert_eq!(reread.beta_apis(), Some(true));
        assert_eq!(reread.version, 10);
    }

    #[test]
    fn applying_the_state_a_world_already_has_reports_no_change() {
        let bytes = build(
            10,
            experiments(&[
                ("experiments_ever_used", 1),
                ("gametest", 1),
                ("saved_with_toggled_experiments", 1),
            ]),
        );
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let change = apply_beta_apis(&path, true).unwrap();
        assert!(!change.changed);
        assert_eq!(change.before, Some(true));
    }
}
