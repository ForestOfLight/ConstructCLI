//! Copies taken before anything is modified.
//!
//! Backups live outside the world folder: a `db.backup-*` inside a world would
//! confuse Minecraft, bloat the world, and ride along into any world export.
//! Retention is keyed on the qualified reference rather than the folder name,
//! which is not unique across roots.

use crate::config::Backups;
use crate::error::Result;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The directory backups go in: configured, else the platform data directory.
pub fn root(backups: &Backups) -> Result<PathBuf> {
    if let Some(dir) = &backups.dir {
        return Ok(dir.clone());
    }
    directories::ProjectDirs::from("", "", "constructcli")
        .map(|d| d.data_dir().join("backups"))
        .ok_or_else(|| {
            crate::error::CoreError::Io(std::io::Error::other(
                "no platform data directory for backups; set [backups] dir in config.toml",
            ))
        })
}

/// One directory name per world, from its qualified reference.
pub fn sanitize(reference: &str) -> String {
    reference
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Copies `src` into the backup directory for `world_reference` and prunes.
pub fn file(src: &Path, world_reference: &str, backups: &Backups) -> Result<PathBuf> {
    let dir = root(backups)?.join(sanitize(world_reference));
    std::fs::create_dir_all(&dir)?;

    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("backup")
        .to_string();
    // Epoch seconds rather than a formatted date: a date needs a dependency,
    // and the full path is printed to the user anyway.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Find the highest existing n for this timestamp to avoid gaps
    // when old backups are deleted by pruning.
    let prefix = format!("{name}.{stamp}");
    let mut next_n = 0u64;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if let Some(suffix) = name_str.strip_prefix(&format!("{prefix}-")) {
                if let Ok(n) = suffix.parse::<u64>() {
                    next_n = next_n.max(n + 1);
                }
            } else if name_str == prefix {
                next_n = next_n.max(1);
            }
        }
    }

    let at = if next_n == 0 {
        dir.join(format!("{name}.{stamp}"))
    } else {
        dir.join(format!("{name}.{stamp}-{next_n}"))
    };

    std::fs::copy(src, &at)?;
    prune(&dir, &name, backups.keep);
    Ok(at)
}

/// Keeps the newest `keep` backups of one file, by numeric suffix.
///
/// Sorts by the numeric suffix in the filename (e.g., the epoch seconds and
/// potential `-n` disambiguator), which is monotonic by construction. This
/// is more reliable than sorting by modification time, which may be coarse
/// on some filesystems.
///
/// Best-effort: a backup that cannot be removed is not worth failing a write
/// that already succeeded.
fn prune(dir: &Path, name: &str, keep: usize) {
    let prefix = format!("{name}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<(u64, u64, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|e| {
            let file_name = e.file_name();
            let name_str = file_name.to_string_lossy();
            // Parse: name.{stamp} or name.{stamp}-{n}
            let suffix = name_str.strip_prefix(&prefix)?;
            let (stamp_str, n_str) = if let Some(dash_idx) = suffix.rfind('-') {
                let (s, n) = suffix.split_at(dash_idx);
                (s, n.trim_start_matches('-'))
            } else {
                (suffix, "0")
            };
            let stamp = stamp_str.parse::<u64>().ok()?;
            let n = n_str.parse::<u64>().ok()?;
            Some((stamp, n, e.path()))
        })
        .collect();
    if found.len() <= keep {
        return;
    }
    // Sort by (stamp, n) in descending order (newest first)
    found.sort_by_key(|item| Reverse((item.0, item.1)));
    for (_, _, path) in found.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Backups;

    fn backups(dir: &Path, keep: usize) -> Backups {
        Backups {
            dir: Some(dir.to_path_buf()),
            keep,
        }
    }

    #[test]
    fn a_qualified_reference_becomes_one_safe_directory_name() {
        assert_eq!(
            sanitize("release/Shared/Ssu8ww1SFbM="),
            "release_Shared_Ssu8ww1SFbM_"
        );
        assert_eq!(
            sanitize("mcpelauncher/Amelix CMP"),
            "mcpelauncher_Amelix_CMP"
        );
        // Nothing that could climb out of the backup directory survives.
        assert_eq!(sanitize("../../etc"), "______etc");
    }

    #[test]
    fn a_backup_is_a_copy_outside_the_world() {
        let tmp = tempfile::tempdir().unwrap();
        let world = tmp.path().join("world");
        std::fs::create_dir_all(&world).unwrap();
        let level = world.join("level.dat");
        std::fs::write(&level, b"original").unwrap();

        let store = tmp.path().join("backups");
        let at = file(&level, "test/World", &backups(&store, 10)).unwrap();

        assert_eq!(std::fs::read(&at).unwrap(), b"original");
        assert!(
            at.starts_with(&store),
            "backup must live outside the world: {}",
            at.display()
        );
        assert!(
            at.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("level.dat.")
        );
    }

    #[test]
    fn retention_keeps_the_newest_and_drops_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        let store = tmp.path().join("backups");
        let cfg = backups(&store, 3);

        let mut made = Vec::new();
        for i in 0..5 {
            std::fs::write(&level, format!("v{i}")).unwrap();
            made.push(file(&level, "test/World", &cfg).unwrap());
        }
        let kept: Vec<_> = std::fs::read_dir(store.join("test_World"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(kept.len(), 3, "keep = 3");
        // The newest survives, whatever the clock did.
        assert!(made.last().unwrap().exists());
        assert!(!made[0].exists());
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_collide() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        std::fs::write(&level, b"x").unwrap();
        let cfg = backups(&tmp.path().join("b"), 10);

        let a = file(&level, "w", &cfg).unwrap();
        let b = file(&level, "w", &cfg).unwrap();
        assert_ne!(a, b);
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn backups_of_different_worlds_do_not_share_retention() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        std::fs::write(&level, b"x").unwrap();
        let cfg = backups(&tmp.path().join("b"), 1);

        // The folder name is the same in both; the qualified reference is not.
        let a = file(&level, "release/Survival", &cfg).unwrap();
        let b = file(&level, "preview/Survival", &cfg).unwrap();
        assert!(
            a.exists() && b.exists(),
            "one world's backup must not evict another's"
        );
    }
}
