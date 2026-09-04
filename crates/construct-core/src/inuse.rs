//! Detecting a world Minecraft currently has open.
//!
//! Writing `level.dat` while the game has the world loaded accomplishes
//! nothing. Minecraft reads the whole file into memory when the world loads
//! and rewrites it from memory on every save, so a write underneath it
//! succeeds, reads back correct, and is gone the next time the game saves.
//! That is not a hypothetical: it is the defect this module exists to stop —
//! `install --world` reported "Beta APIs on" against a world the game had
//! open, and the flag was gone minutes later.
//!
//! The obvious signal is not available. leveldb normally guards a database
//! with an flock'd `db/LOCK`, but the build shipped by mcpelauncher creates
//! no `LOCK` file at all, even with a world open — measured 2026-09-04
//! against a live session, where the only `LOCK` on the machine was a
//! leftover from an unclean exit a year earlier. Existence of the file is
//! therefore evidence of neither state.
//!
//! What *is* observable is the autosave. With a world loaded, Bedrock
//! rewrites `db/` metronomically: 19 write events in 90 seconds, longest gap
//! 5 seconds, measured against that same live session. Recent write activity
//! in `db/` is the signal, and [`ACTIVITY_WINDOW`] is twice the longest
//! measured gap.
//!
//! This is a heuristic, and it is deliberately one-sided. A false positive
//! costs a refused command that the user retries a few seconds later. A
//! false negative costs a silent revert of a change the tool said it made —
//! which is the bug. So the window errs long, and callers refuse rather than
//! warn.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// How recently `db/` must have been written for a world to count as in use.
///
/// Twice the longest gap measured between autosaves of a live world (5s).
pub const ACTIVITY_WINDOW: Duration = Duration::from_secs(10);

/// The most recent modification time of anything directly inside `db/`.
///
/// Not recursive: a leveldb directory is flat, and a nested directory there
/// would not be something the game writes on its autosave tick.
fn newest_write(db: &Path) -> Option<SystemTime> {
    std::fs::read_dir(db)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

/// Whether `db` was written recently enough that Minecraft is probably
/// holding this world open.
///
/// A database that does not exist, cannot be read, or is empty is not in
/// use: absence of evidence is treated as absence here, because the callers
/// that matter go on to fail with a clearer error of their own.
pub fn looks_in_use(db: &Path) -> bool {
    let Some(newest) = newest_write(db) else {
        return false;
    };
    match SystemTime::now().duration_since(newest) {
        Ok(age) => age < ACTIVITY_WINDOW,
        // A modification time in the future means a clock or filesystem we
        // cannot reason about. Treat it as active: the cost of being wrong
        // in this direction is a retry, not a lost change.
        Err(_) => true,
    }
}

/// Refuses when Minecraft appears to have `world` open.
///
/// Callers on a `level.dat` write path must go through this before taking a
/// backup, so that a refused command leaves no trace at all. Read paths must
/// *not* call it: reads work from a snapshot copy and are safe at any time.
pub fn refuse_if_in_use(world: &World) -> Result<()> {
    if looks_in_use(&world.db_path()) {
        return Err(CoreError::WorldInUse {
            world: world.path.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};

    /// Backdates every file in `dir` by `secs`, so a test can produce a world
    /// that was written a while ago without waiting for the clock.
    fn backdate(dir: &Path, secs: u64) {
        let when = SystemTime::now() - Duration::from_secs(secs);
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let f = File::options().write(true).open(entry.path()).unwrap();
            f.set_times(FileTimes::new().set_modified(when)).unwrap();
        }
    }

    fn db_with_a_file() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("000021.log"), b"chunk data").unwrap();
        tmp
    }

    #[test]
    fn a_database_written_just_now_looks_in_use() {
        let tmp = db_with_a_file();
        assert!(looks_in_use(tmp.path()));
    }

    #[test]
    fn a_database_untouched_for_longer_than_the_window_does_not() {
        let tmp = db_with_a_file();
        backdate(tmp.path(), ACTIVITY_WINDOW.as_secs() + 5);
        assert!(!looks_in_use(tmp.path()));
    }

    #[test]
    fn one_fresh_file_among_stale_ones_is_enough() {
        // Bedrock rewrites its log far more often than it compacts, so the
        // newest file is what matters, not the average.
        let tmp = db_with_a_file();
        std::fs::write(tmp.path().join("000022.ldb"), b"older").unwrap();
        backdate(tmp.path(), ACTIVITY_WINDOW.as_secs() + 5);
        std::fs::write(tmp.path().join("000021.log"), b"just written").unwrap();
        assert!(looks_in_use(tmp.path()));
    }

    #[test]
    fn a_missing_database_is_not_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!looks_in_use(&tmp.path().join("db")));
    }

    #[test]
    fn an_empty_database_directory_is_not_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!looks_in_use(tmp.path()));
    }

    #[test]
    fn a_modification_time_in_the_future_counts_as_in_use() {
        let tmp = db_with_a_file();
        let ahead = SystemTime::now() + Duration::from_secs(3600);
        let f = File::options()
            .write(true)
            .open(tmp.path().join("000021.log"))
            .unwrap();
        f.set_times(FileTimes::new().set_modified(ahead)).unwrap();
        assert!(looks_in_use(tmp.path()));
    }
}
