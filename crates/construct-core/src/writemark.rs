//! A record of the last write this tool made to a world's database.
//!
//! [`crate::inuse`] cannot tell the game's write from ours, so `delete`'s own
//! write made the next run look like a running game and pay a full
//! [`crate::inuse::CONFIRM_WATCH`]. Recording the newest mtime after a write
//! answers that case instantly: a later mtime equal to the mark is ours, since
//! any write by the game since would be strictly newer.
//!
//! Every failure falls the safe way. A missing mark, a first run, a moved
//! clock — all fail to match, which only sends the caller to the watch it
//! would have done anyway. Nothing here can make a world look free when it is
//! not.

use crate::discovery::World;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where marks live: `writemarks/` under [`crate::config::data_dir`].
///
/// Not `[backups] dir`. A mark is disposable bookkeeping, not a backup, and a
/// user redirecting backups to external storage must not silently move this
/// too. Both sit under the platform data directory, which the user does not
/// redirect, in separate subdirectories.
pub fn root() -> Option<PathBuf> {
    crate::config::data_dir(&|k| std::env::var(k).ok()).map(|d| d.join("writemarks"))
}

fn mark_path(dir: &Path, world: &World) -> PathBuf {
    dir.join(crate::backup::sanitize(&world.qualified()))
}

fn stamp(at: SystemTime) -> Option<String> {
    let d = at.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}.{:09}", d.as_secs(), d.subsec_nanos()))
}

/// Records the newest write currently in the world's `db/` as this tool's own.
///
/// Call *after* dropping the database handle, so leveldb has flushed and the
/// mtimes are final. Best-effort and silent: an unwritable mark costs the next
/// run a watch, nothing more.
pub fn record_in(dir: &Path, world: &World) {
    let Some(newest) = crate::inuse::newest_write(&world.db_path()) else {
        return;
    };
    let Some(text) = stamp(newest) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let final_path = mark_path(dir, world);
    let staging = final_path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&staging, text).is_ok() && std::fs::rename(&staging, &final_path).is_err() {
        let _ = std::fs::remove_file(&staging);
    }
}

/// Whether the newest write to the world's `db/` is the one recorded here.
///
/// `true` means the activity that made [`crate::inuse::looks_in_use`]
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

pub fn record(world: &World) {
    if let Some(dir) = root() {
        record_in(&dir, world);
    }
}

pub fn left_by_us(world: &World) -> bool {
    root().is_some_and(|dir| left_by_us_in(&dir, world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{LastPlayedSource, World};

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
