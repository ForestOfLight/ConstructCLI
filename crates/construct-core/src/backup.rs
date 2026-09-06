//! Copies taken before anything is modified.
//!
//! Backups live outside the world folder: a `db.backup-*` inside a world would
//! confuse Minecraft, bloat the world, and ride along into any world export.
//! Retention is keyed on the qualified reference rather than the folder name,
//! which is not unique across roots.

use crate::config::Backups;
use crate::error::{CoreError, Result};
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The directory backups go in: configured, else `CONSTRUCT_BACKUPS_DIR`, else
/// the platform data directory.
///
/// The environment override sits *below* `[backups] dir` because that is the
/// documented user knob; this one stands in for the platform lookup, so that
/// tests never write into the real user's data directory. It is not a
/// documented user knob, and it is separate from `CONSTRUCT_STATE_DIR` for the
/// reason given on [`crate::writemark::root`]: a mark is not a backup.
///
/// Tests need it because a harness that points `HOME`/`USERPROFILE` at a
/// scratch directory has no platform data directory at all on Windows, where
/// the lookup resolves the *real* known folder and fails when it is missing —
/// unlike Unix, where it is built from the environment and always yields a
/// path. Without the override the failure surfaces as a spurious "could not
/// turn Beta APIs on" and a partial-success exit.
pub fn root(backups: &Backups) -> Result<PathBuf> {
    root_from(backups, &|k| std::env::var(k).ok())
}

/// `root`, with the environment injected — the same seam [`crate::config::load`]
/// uses, so precedence is testable without mutating the process environment.
fn root_from(backups: &Backups, env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf> {
    if let Some(dir) = &backups.dir {
        return Ok(dir.clone());
    }
    if let Some(dir) = env("CONSTRUCT_BACKUPS_DIR") {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("", "", "constructcli")
        .map(|d| d.data_dir().join("backups"))
        .ok_or(CoreError::NoBackupDir)
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

/// Parse a backup filename suffix into (epoch_seconds, disambiguator).
///
/// Expected format: `{stamp}` or `{stamp}-{n}`.
/// Returns `None` if the suffix cannot be parsed.
fn parse_suffix(suffix: &str) -> Option<(u64, u64)> {
    if let Some(dash_idx) = suffix.rfind('-') {
        let (stamp_str, n_str) = suffix.split_at(dash_idx);
        let stamp = stamp_str.parse::<u64>().ok()?;
        let n = n_str.trim_start_matches('-').parse::<u64>().ok()?;
        Some((stamp, n))
    } else {
        let stamp = suffix.parse::<u64>().ok()?;
        Some((stamp, 0))
    }
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
    let prefix = format!("{name}.");
    let mut next_n = 0u64;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if let Some(suffix) = name_str.strip_prefix(&prefix)
                && let Some((entry_stamp, entry_n)) = parse_suffix(suffix)
                && entry_stamp == stamp
            {
                next_n = next_n.max(entry_n + 1);
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
            let suffix = name_str.strip_prefix(&prefix)?;
            let (stamp, n) = parse_suffix(suffix)?;
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
    fn the_configured_directory_wins_over_the_environment() {
        let configured = backups(Path::new("/configured"), 5);
        let env = |k: &str| (k == "CONSTRUCT_BACKUPS_DIR").then(|| "/from-env".to_string());
        assert_eq!(
            root_from(&configured, &env).unwrap(),
            PathBuf::from("/configured")
        );
    }

    #[test]
    fn the_environment_stands_in_for_the_platform_directory() {
        let unset = Backups { dir: None, keep: 5 };
        let env = |k: &str| (k == "CONSTRUCT_BACKUPS_DIR").then(|| "/from-env".to_string());
        assert_eq!(root_from(&unset, &env).unwrap(), PathBuf::from("/from-env"));
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

    #[test]
    fn retention_across_multiple_timestamps() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("backups");
        let world_ref = "test/World";
        let world_dir = store.join(sanitize(world_ref));
        std::fs::create_dir_all(&world_dir).unwrap();

        // Manually create backups at different timestamps
        std::fs::write(world_dir.join("level.dat.1000"), b"old").unwrap();
        std::fs::write(world_dir.join("level.dat.1000-1"), b"old").unwrap();
        std::fs::write(world_dir.join("level.dat.1000-2"), b"old").unwrap();
        std::fs::write(world_dir.join("level.dat.1000-3"), b"old").unwrap();
        std::fs::write(world_dir.join("level.dat.2000"), b"newer").unwrap();

        // Prune to keep only 2 backups
        prune(&world_dir, "level.dat", 2);

        // The newer timestamp's entry should survive, even though older timestamp has higher n values
        assert!(
            world_dir.join("level.dat.2000").exists(),
            "newer timestamp should survive"
        );
        // At least one more from the older timestamp may survive, but the pattern is that
        // we keep by (timestamp, n) ordering
        let kept: Vec<_> = std::fs::read_dir(&world_dir).unwrap().flatten().collect();
        assert_eq!(kept.len(), 2, "should keep exactly 2 backups");
    }

    #[test]
    fn source_filename_with_dash_parses_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("level-legacy.dat");
        std::fs::write(&source, b"content").unwrap();

        let store = tmp.path().join("backups");
        let cfg = backups(&store, 10);

        // Create multiple backups of a file with a dash in its name
        let b1 = file(&source, "test/World", &cfg).unwrap();
        let b2 = file(&source, "test/World", &cfg).unwrap();
        let b3 = file(&source, "test/World", &cfg).unwrap();

        // All should exist and have correct content
        assert_eq!(std::fs::read(&b1).unwrap(), b"content");
        assert_eq!(std::fs::read(&b2).unwrap(), b"content");
        assert_eq!(std::fs::read(&b3).unwrap(), b"content");
        assert_ne!(b1, b2);
        assert_ne!(b2, b3);

        // Verify filenames are correctly parsed (contain the source name)
        let b1_name = b1.file_name().unwrap().to_string_lossy();
        assert!(b1_name.starts_with("level-legacy.dat."));
    }

    #[test]
    fn unrelated_files_survive_pruning() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("backups");
        let world_dir = store.join(sanitize("test/World"));
        std::fs::create_dir_all(&world_dir).unwrap();

        // Create backups of our source file
        std::fs::write(world_dir.join("level.dat.1000"), b"1").unwrap();
        std::fs::write(world_dir.join("level.dat.1000-1"), b"2").unwrap();
        std::fs::write(world_dir.join("level.dat.1000-2"), b"3").unwrap();

        // Create unrelated files in the same directory
        std::fs::write(world_dir.join("not-a-backup.txt"), b"unrelated").unwrap();
        std::fs::write(world_dir.join("options.txt.1000"), b"another-world").unwrap();

        // Prune level.dat backups to keep only 1
        prune(&world_dir, "level.dat", 1);

        // Our unrelated files should survive
        assert!(
            world_dir.join("not-a-backup.txt").exists(),
            "unrelated file should survive"
        );
        assert!(
            world_dir.join("options.txt.1000").exists(),
            "another world's backup should survive"
        );

        // We should have 1 level.dat backup + 2 unrelated = 3 files total
        let kept: Vec<_> = std::fs::read_dir(&world_dir).unwrap().flatten().collect();
        assert_eq!(kept.len(), 3, "should keep 1 level.dat + 2 unrelated");
    }
}
