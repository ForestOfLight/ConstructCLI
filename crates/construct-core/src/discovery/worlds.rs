//! Turning installations into the flat list of worlds the CLI shows.

use crate::discovery::platform::Installation;
use crate::leveldat;
use std::path::{Path, PathBuf};

/// Where a world's last-played time came from. The two sources disagree often
/// enough that `--json` reports which was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastPlayedSource {
    LevelDat,
    DirMtime,
}

/// One discovered world.
#[derive(Debug, Clone)]
pub struct World {
    pub installation: String,
    pub account: Option<String>,
    /// The directory name, e.g. `Ssu8ww1SFbM=`.
    pub folder: String,
    /// From `levelname.txt`, falling back to the folder name.
    pub display_name: String,
    pub path: PathBuf,
    /// Unix seconds.
    pub last_played: Option<i64>,
    pub last_played_source: LastPlayedSource,
    /// Size of `db/`, which is what dominates a world and what a snapshot copies.
    pub size_bytes: u64,
}

impl World {
    /// `<installation>/<account>/<folder>`, with the account segment omitted
    /// when the installation has only one world root.
    pub fn qualified(&self) -> String {
        match &self.account {
            Some(a) => format!("{}/{}/{}", self.installation, a, self.folder),
            None => format!("{}/{}", self.installation, self.folder),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.path.join("db")
    }
}

/// Every world under every installation. Unreadable entries are skipped rather
/// than failing the whole enumeration.
pub fn enumerate(installations: &[Installation]) -> Vec<World> {
    let mut out = Vec::new();

    for installation in installations {
        for root in &installation.world_roots {
            let Ok(entries) = std::fs::read_dir(&root.path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // A world is a directory containing level.dat. Nothing else counts.
                if !path.join("level.dat").is_file() {
                    continue;
                }
                let folder = entry.file_name().to_string_lossy().into_owned();
                let display_name = std::fs::read_to_string(path.join("levelname.txt"))
                    .map(|s| s.trim().to_string())
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| folder.clone());

                let (last_played, last_played_source) = last_played(&path);

                out.push(World {
                    installation: installation.name.clone(),
                    account: root.account.clone(),
                    folder,
                    display_name,
                    size_bytes: dir_size(&path.join("db")),
                    path,
                    last_played,
                    last_played_source,
                });
            }
        }
    }

    // Newest first — the world you want is almost always the one you just played.
    out.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then(a.folder.cmp(&b.folder))
    });
    out
}

fn last_played(world: &Path) -> (Option<i64>, LastPlayedSource) {
    if let Ok(dat) = leveldat::read(&world.join("level.dat"))
        && let Some(v) = dat.last_played()
    {
        return (Some(v), LastPlayedSource::LevelDat);
    }
    let mtime = std::fs::metadata(world)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    (mtime, LastPlayedSource::DirMtime)
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::platform::{Installation, WorldRoot};
    use std::fs;

    fn world_at(root: &Path, folder: &str, name: &str) -> PathBuf {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("db/000001.ldb"), vec![0u8; 1024]).unwrap();
        fs::write(dir.join("levelname.txt"), name).unwrap();
        dir
    }

    fn level_dat_with(dir: &Path, last_played: i64) {
        let root = nbtx::Value::Compound(
            [("LastPlayed".to_string(), nbtx::Value::Long(last_played))]
                .into_iter()
                .collect(),
        );
        let payload = nbtx::to_le_bytes(&root).unwrap();
        let mut bytes = 10i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        fs::write(dir.join("level.dat"), bytes).unwrap();
    }

    fn single_root(tmp: &Path) -> Vec<Installation> {
        vec![Installation {
            name: "mcpelauncher".to_string(),
            dev_pack_root: tmp.to_path_buf(),
            world_roots: vec![WorldRoot {
                account: None,
                path: tmp.join("minecraftWorlds"),
            }],
        }]
    }

    #[test]
    fn reads_display_name_from_levelname_txt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("minecraftWorlds");
        let dir = world_at(&root, "Ssu8ww1SFbM=", "construct show");
        level_dat_with(&dir, 1741501762);

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].display_name, "construct show");
        assert_eq!(worlds[0].folder, "Ssu8ww1SFbM=");
    }

    #[test]
    fn prefers_level_dat_last_played_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1741501762);

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].last_played, Some(1741501762));
        assert_eq!(worlds[0].last_played_source, LastPlayedSource::LevelDat);
    }

    #[test]
    fn falls_back_to_dir_mtime_when_level_dat_is_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        fs::write(dir.join("level.dat"), b"garbage").unwrap();

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].last_played_source, LastPlayedSource::DirMtime);
        assert!(worlds[0].last_played.is_some());
    }

    #[test]
    fn display_name_falls_back_to_the_folder_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("minecraftWorlds/NoName=");
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("level.dat"), b"x").unwrap();

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].display_name, "NoName=");
    }

    #[test]
    fn a_directory_without_level_dat_is_not_a_world() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("minecraftWorlds/not_a_world")).unwrap();
        assert!(enumerate(&single_root(tmp.path())).is_empty());
    }

    #[test]
    fn qualified_omits_the_account_when_there_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1);
        assert_eq!(
            enumerate(&single_root(tmp.path()))[0].qualified(),
            "mcpelauncher/A="
        );
    }

    #[test]
    fn qualified_includes_the_account_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("minecraftWorlds");
        let dir = world_at(&root, "A=", "W");
        level_dat_with(&dir, 1);
        let installs = vec![Installation {
            name: "release".to_string(),
            dev_pack_root: tmp.path().to_path_buf(),
            world_roots: vec![WorldRoot {
                account: Some("Shared".into()),
                path: root,
            }],
        }];
        assert_eq!(enumerate(&installs)[0].qualified(), "release/Shared/A=");
    }

    #[test]
    fn size_counts_the_db_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1);
        assert!(enumerate(&single_root(tmp.path()))[0].size_bytes >= 1024);
    }

    #[test]
    fn a_folder_name_containing_a_space_is_handled() {
        // "Amelix CMP" is a real world folder on the target machine.
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(
            &tmp.path().join("minecraftWorlds"),
            "Amelix CMP",
            "Amelix CMP",
        );
        level_dat_with(&dir, 1);
        assert_eq!(enumerate(&single_root(tmp.path()))[0].folder, "Amelix CMP");
    }
}
