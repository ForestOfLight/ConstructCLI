//! Detecting a world Minecraft currently has open.
//!
//! Writes under a live world do not survive: `level.dat` is rewritten from the
//! game's memory on every save, and a second writer against a live `db/` can
//! corrupt it. `db/LOCK` cannot detect this — the mcpelauncher build creates no
//! `LOCK` file even with a world open (measured 2026-09-04).
//!
//! Recent write activity in `db/` is the signal instead, in two phases:
//! [`looks_in_use`] suspects, [`confirm_in_use`] decides.
//!
//! Deliberately one-sided. A false positive costs a retry; a false negative
//! costs a silent revert or a damaged save. Timings err long, and callers
//! refuse rather than warn.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// How recently `db/` must have been written for a world to be *suspected* of
/// being in use.
///
/// Twice the longest autosave gap measured against a live world: 5s on
/// mcpelauncher/macOS, 10s on mcpelauncher flatpak/Linux, both 1.26.45.1.
/// Twice, not equal — [`looks_in_use`] compares with `<`, so a window equal to
/// the worst case reads a live world as free whenever the game leaves a full
/// gap.
pub const ACTIVITY_WINDOW: Duration = Duration::from_secs(20);

/// How long [`confirm_in_use`] watches for a new write before concluding the
/// recent one was a one-off.
///
/// Must exceed the longest autosave gap, or a live world could stay quiet for
/// the whole watch and be waved through.
pub const CONFIRM_WATCH: Duration = Duration::from_secs(20);

const CONFIRM_TICK: Duration = Duration::from_millis(500);

/// The most recent modification time of anything directly inside `db/`.
///
/// Not recursive: a leveldb directory is flat.
///
/// Public because [`crate::writemark`] stores and compares this exact value.
/// The two must agree on what "the newest write" means.
pub fn newest_write(db: &Path) -> Option<SystemTime> {
    std::fs::read_dir(db)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

/// Whether `db` was written recently enough to suspect Minecraft has this
/// world open.
///
/// Instant and deliberately trigger-happy; not an answer on its own, since
/// [`confirm_in_use`] decides. A missing, unreadable, or empty database
/// reports `false`.
pub fn looks_in_use(db: &Path) -> bool {
    let Some(newest) = newest_write(db) else {
        return false;
    };
    match SystemTime::now().duration_since(newest) {
        Ok(age) => age < ACTIVITY_WINDOW,
        Err(_) => true,
    }
}

/// What a write to an open world would put at risk.
///
/// The two dangers are different, and the advice for them differs. The caller
/// knows which applies; [`CoreError::WorldInUse`] carries it so the message
/// can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtRisk {
    /// `level.dat`: a write succeeds, reads back correct, and is discarded at
    /// the next save.
    LevelDat,
    /// The world's database: opening it is itself a write, and a second
    /// writer against a live leveldb can corrupt the save unrecoverably.
    Database,
}

/// Watches `db` for a new write, distinguishing a live world from a command
/// that just finished writing one.
///
/// A live world keeps writing; a finished command does not. Returns `true` as
/// soon as a new write appears, or `false` once [`CONFIRM_WATCH`] elapses
/// unchanged — so a live world costs about one autosave gap, not the full
/// watch.
///
/// Portable by choice: exact per-platform answers exist, but a check precise on
/// one platform and absent on another is worse than one that behaves the same
/// everywhere.
///
/// Only meaningful after [`looks_in_use`] returns `true`.
pub fn confirm_in_use(db: &Path) -> bool {
    watch_for_writes(db, CONFIRM_WATCH, CONFIRM_TICK)
}

fn watch_for_writes(db: &Path, watch: Duration, tick: Duration) -> bool {
    let Some(baseline) = newest_write(db) else {
        return false;
    };
    let deadline = std::time::Instant::now() + watch;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(tick);
        match newest_write(db) {
            Some(now) if now > baseline => return true,
            None => return true,
            _ => {}
        }
    }
    false
}

/// Refuses when Minecraft appears to have `world` open.
///
/// Write paths must call this before taking a backup or opening anything, so
/// a refusal leaves no trace. Read paths must not: reads work from a snapshot
/// and are safe at any time.
pub fn refuse_if_in_use(world: &World, at_risk: AtRisk) -> Result<()> {
    let db = world.db_path();

    if !looks_in_use(&db) {
        return Ok(());
    }

    if crate::writemark::left_by_us(world) {
        return Ok(());
    }

    if confirm_in_use(&db) {
        return Err(CoreError::WorldInUse {
            world: world.path.clone(),
            at_risk,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};

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

    const FAST_WATCH: Duration = Duration::from_millis(300);
    const FAST_TICK: Duration = Duration::from_millis(20);

    #[test]
    fn a_database_nobody_is_writing_is_not_confirmed() {
        let tmp = db_with_a_file();
        assert!(looks_in_use(tmp.path()), "phase one suspects it");
        assert!(
            !watch_for_writes(tmp.path(), FAST_WATCH, FAST_TICK),
            "phase two must clear a database that has stopped being written"
        );
    }

    #[test]
    fn a_database_being_written_during_the_watch_is_confirmed() {
        let tmp = db_with_a_file();
        let path = tmp.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            let f = File::options()
                .write(true)
                .open(path.join("000021.log"))
                .unwrap();
            f.set_times(FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(1)))
                .unwrap();
        });
        assert!(watch_for_writes(tmp.path(), FAST_WATCH, FAST_TICK));
        writer.join().unwrap();
    }

    #[test]
    fn a_compaction_that_removes_the_newest_file_counts_as_active() {
        let tmp = db_with_a_file();
        let path = tmp.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            std::fs::remove_file(path.join("000021.log")).unwrap();
        });
        assert!(watch_for_writes(tmp.path(), FAST_WATCH, FAST_TICK));
        writer.join().unwrap();
    }

    #[test]
    fn the_watch_outlasts_the_longest_measured_autosave_gap() {
        // watch. Measured longest gap is 10s (Linux flatpak 1.26.45.1).
        assert!(
            CONFIRM_WATCH >= Duration::from_secs(20),
            "the watch must exceed the longest real autosave gap, with margin"
        );
        assert!(
            ACTIVITY_WINDOW >= Duration::from_secs(20),
            "a window equal to the longest gap lets a live world read as free"
        );
    }

    /// The database named by `var`, or `None` after saying why it is skipping.
    ///
    /// The three tests below are `#[ignore]`d because they need a real world in
    /// a known state — one open in Minecraft, one closed for hours:
    ///
    /// ```
    /// CONSTRUCT_LIVE_WORLD_DB=<com.mojang>/minecraftWorlds/<world>/db \
    /// CONSTRUCT_IDLE_WORLD_DB=<com.mojang>/minecraftWorlds/<other>/db \
    /// cargo test -p construct-core --lib inuse -- --ignored --nocapture
    /// ```
    ///
    /// Running the ignored set without them is the normal case for anyone who
    /// reaches for `cargo test -- --ignored`, so it skips with the variable's
    /// name rather than panicking on an `unwrap`.
    fn world_db_from_env(var: &str) -> Option<std::path::PathBuf> {
        let Ok(value) = std::env::var(var) else {
            eprintln!("skipping: {var} is not set");
            return None;
        };
        let db = std::path::PathBuf::from(value);
        assert!(
            db.is_dir(),
            "{var} is set but is no directory: {}",
            db.display()
        );
        Some(db)
    }

    #[test]
    #[ignore = "needs a world open in Minecraft"]
    fn a_real_live_world_is_detected() {
        let Some(db) = world_db_from_env("CONSTRUCT_LIVE_WORLD_DB") else {
            return;
        };
        let t0 = std::time::Instant::now();
        assert!(looks_in_use(&db), "phase one must suspect a live world");
        assert!(confirm_in_use(&db), "phase two must confirm a live world");
        println!(
            "confirmed live in {:?} (watch cap {CONFIRM_WATCH:?})",
            t0.elapsed()
        );
    }

    #[test]
    #[ignore = "needs a world open in Minecraft"]
    fn a_mark_cannot_mask_a_real_live_world() {
        let Some(db) = world_db_from_env("CONSTRUCT_LIVE_WORLD_DB") else {
            return;
        };
        let marks = tempfile::tempdir().unwrap();
        let world = World {
            installation: "test".into(),
            account: None,
            folder: "live".into(),
            display_name: "live".into(),
            path: db.parent().unwrap().to_path_buf(),
            last_played: None,
            last_played_source: crate::discovery::LastPlayedSource::DirMtime,
            size_bytes: 0,
        };

        crate::writemark::record_in(marks.path(), &world);

        std::thread::sleep(CONFIRM_WATCH.min(Duration::from_secs(12)));

        assert!(
            !crate::writemark::left_by_us_in(marks.path(), &world),
            "the game wrote since; the mark must have gone stale"
        );
        assert!(
            looks_in_use(&db) && confirm_in_use(&db),
            "still detected live"
        );
    }

    #[test]
    #[ignore = "needs a closed world"]
    fn a_real_closed_world_is_not_detected() {
        let Some(db) = world_db_from_env("CONSTRUCT_IDLE_WORLD_DB") else {
            return;
        };
        assert!(!looks_in_use(&db), "a world closed for hours is not in use");
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
