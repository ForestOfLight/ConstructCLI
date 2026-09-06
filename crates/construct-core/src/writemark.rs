//! A record of the last write this tool made to a world's database.
//!
//! [`crate::inuse`] answers "is Minecraft holding this world open?" by looking
//! at how recently `db/` was written, which cannot by itself tell the game's
//! write from ours — and `delete` writes `db/`, so its own write made the next
//! run look exactly like a running game. The second phase resolves that by
//! watching for a *further* write, which is correct but costs up to
//! [`crate::inuse::CONFIRM_WATCH`] in precisely the case that is not a problem
//! at all.
//!
//! So: remember. After a write, record the newest mtime left in `db/`. On the
//! next run, a recent write whose mtime is exactly the one recorded is ours,
//! answered instantly with no watching. Equality is a sound test — same file,
//! same filesystem, and any write by the game since is strictly newer, which
//! no mtime granularity can blur.
//!
//! Every failure falls the safe way. A missing mark, an unreadable one, a
//! first run, a world someone else touched, a clock that moved — all fail to
//! match, and a non-match means only that the caller falls through to the
//! watch it would have done anyway. Nothing here can make a world look free
//! when it is not; the worst it can do is fail to make one look free when it
//! is.

use crate::discovery::World;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where marks live: `writemarks/` under [`crate::config::data_dir`].
///
/// Deliberately not `[backups] dir`, and deliberately not the backup
/// directory either. A mark is not a backup — it is disposable bookkeeping,
/// and a user who redirects backups to external storage must not silently
/// move this too. The shared root is the platform data directory, which the
/// user does not redirect; the two subdirectories under it stay distinct.
pub fn root() -> Option<PathBuf> {
    crate::config::data_dir(&|k| std::env::var(k).ok()).map(|d| d.join("writemarks"))
}

/// One file per world, keyed the way backups are: on the qualified reference
/// rather than the folder name, which is not unique across roots.
fn mark_path(dir: &Path, world: &World) -> PathBuf {
    dir.join(crate::backup::sanitize(&world.qualified()))
}

/// Renders an mtime exactly, so the comparison is not lossy.
fn stamp(at: SystemTime) -> Option<String> {
    let d = at.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}.{:09}", d.as_secs(), d.subsec_nanos()))
}

/// Records the newest write currently in the world's `db/` as this tool's own.
///
/// Call *after* the database handle has been dropped, so leveldb has flushed
/// and the mtimes are final. Best-effort and silent: if the mark cannot be
/// written, the next run watches instead of answering instantly, which is the
/// behaviour it had before this module existed.
pub fn record_in(dir: &Path, world: &World) {
    let Some(newest) = crate::inuse::newest_write(&world.db_path()) else {
        return;
    };
    let Some(text) = stamp(newest) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Write-then-rename rather than a plain write: two processes marking the
    // same world at once would otherwise let a reader see a half-written
    // stamp, which reads as "not ours" and costs a watch nobody needed. The
    // temporary carries the pid so the two writers cannot collide on it
    // either. A rename within one directory is atomic on every platform this
    // runs on.
    let final_path = mark_path(dir, world);
    let staging = final_path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&staging, text).is_ok() && std::fs::rename(&staging, &final_path).is_err() {
        let _ = std::fs::remove_file(&staging);
    }
}

/// Whether the newest write to the world's `db/` is the one we recorded.
///
/// A `true` here means the recent activity that made [`crate::inuse::looks_in_use`]
/// suspicious was this tool's own, and there is nothing to wait for.
pub fn left_by_us_in(dir: &Path, world: &World) -> bool {
    let Some(newest) = crate::inuse::newest_write(&world.db_path()) else {
        return false;
    };
    let Some(text) = stamp(newest) else {
        return false;
    };
    match std::fs::read_to_string(mark_path(dir, world)) {
        Ok(recorded) => recorded.trim() == text,
        Err(_) => false,
    }
}

/// [`record_in`] against the default [`root`].
pub fn record(world: &World) {
    if let Some(dir) = root() {
        record_in(&dir, world);
    }
}

/// [`left_by_us_in`] against the default [`root`].
///
/// No mark directory at all means no mark, which means not ours.
pub fn left_by_us(world: &World) -> bool {
    root().is_some_and(|dir| left_by_us_in(&dir, world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{LastPlayedSource, World};

    /// A world with a `db/` holding one file, plus the mark directory.
    fn world_with_db() -> (tempfile::TempDir, World, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test_level");
        std::fs::create_dir_all(path.join("db")).unwrap();
        std::fs::write(path.join("db/000021.log"), b"chunk data").unwrap();
        let world = World {
            installation: "test".into(),
            account: None,
            folder: "test_level".into(),
            display_name: "test_level".into(),
            path,
            last_played: None,
            last_played_source: LastPlayedSource::DirMtime,
            size_bytes: 0,
        };
        let marks = tmp.path().join("marks");
        (tmp, world, marks)
    }

    #[test]
    fn a_write_we_recorded_is_recognised_as_ours() {
        // The whole point: `delete` writes `db/`, so without this the next run
        // cannot tell its own write from a running game and has to watch.
        let (_tmp, world, marks) = world_with_db();
        record_in(&marks, &world);
        assert!(left_by_us_in(&marks, &world));
    }

    #[test]
    fn a_world_we_never_wrote_is_not_ours() {
        let (_tmp, world, marks) = world_with_db();
        assert!(!left_by_us_in(&marks, &world));
    }

    #[test]
    fn a_write_by_somebody_else_after_ours_is_not_ours() {
        // The case that must never be waved through: the game wrote since.
        let (_tmp, world, marks) = world_with_db();
        record_in(&marks, &world);

        let log = world.db_path().join("000021.log");
        let f = std::fs::File::options().write(true).open(&log).unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::now() + std::time::Duration::from_secs(30)),
        )
        .unwrap();

        assert!(
            !left_by_us_in(&marks, &world),
            "a later write is the game's, not ours"
        );
    }

    #[test]
    fn a_new_file_appearing_after_ours_is_not_ours() {
        // leveldb adds files as well as touching them; the newest is what counts.
        let (_tmp, world, marks) = world_with_db();
        record_in(&marks, &world);

        let newer = world.db_path().join("000022.log");
        std::fs::write(&newer, b"later").unwrap();
        let f = std::fs::File::options().write(true).open(&newer).unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::now() + std::time::Duration::from_secs(30)),
        )
        .unwrap();

        assert!(!left_by_us_in(&marks, &world));
    }

    #[test]
    fn an_unreadable_mark_directory_is_simply_not_a_match() {
        // Failing to answer must never mean "not in use" on its own — the
        // caller falls through to the watch, which is where it started.
        let (_tmp, world, _marks) = world_with_db();
        assert!(!left_by_us_in(Path::new("/nonexistent/marks"), &world));
    }

    #[test]
    fn recording_a_world_with_no_database_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let world = World {
            installation: "test".into(),
            account: None,
            folder: "gone".into(),
            display_name: "gone".into(),
            path: tmp.path().join("gone"),
            last_played: None,
            last_played_source: LastPlayedSource::DirMtime,
            size_bytes: 0,
        };
        let marks = tmp.path().join("marks");
        record_in(&marks, &world);
        assert!(!left_by_us_in(&marks, &world));
    }

    #[test]
    fn two_worlds_do_not_share_a_mark() {
        // Keyed on the qualified reference, because a folder name is not
        // unique across installations.
        let (_tmp, mut a, marks) = world_with_db();
        let mut b = a.clone();
        a.installation = "release".into();
        b.installation = "preview".into();

        record_in(&marks, &a);
        assert!(left_by_us_in(&marks, &a));
        assert!(
            !left_by_us_in(&marks, &b),
            "one world's mark must not answer for another"
        );
    }
}
