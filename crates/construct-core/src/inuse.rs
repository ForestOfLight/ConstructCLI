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

/// How recently `db/` must have been written for a world to be *suspected* of
/// being in use.
///
/// Twice the longest gap measured between autosaves of a live world. That
/// measurement is per-build and has moved once already:
///
/// | Build | Longest gap | Window |
/// | --- | --- | --- |
/// | mcpelauncher / macOS 1.26.45.1 | 5s | 10s |
/// | mcpelauncher flatpak / Linux 1.26.45.1 | **10s** | **20s** |
///
/// The Linux figure (14 writes in 90s, gaps `5 5 5 10 5 5 5 10 5 5 5 5 10 5`,
/// measured 2026-09-05 against a live session) is why this is 20 and not 10.
/// At 10 it was equal to the longest real gap, and `looks_in_use` compares
/// with `<` — so every time the game left a full gap, the world it definitely
/// had open read as free. That is the false negative this whole module exists
/// to prevent, and it was live.
///
/// Widening is cheap now in a way it was not before: a positive here no longer
/// refuses anything on its own, it only sends the caller to
/// [`confirm_in_use`]. Over-suspicion costs a few seconds of watching;
/// under-suspicion costs a corrupted save.
pub const ACTIVITY_WINDOW: Duration = Duration::from_secs(20);

/// How long [`confirm_in_use`] watches for a *new* write before concluding the
/// recent one was a one-off.
///
/// Must exceed the longest autosave gap, or a live world could sit quiet for
/// the whole watch and be waved through — the same false negative by another
/// route. Same 2x rule, same measurement.
pub const CONFIRM_WATCH: Duration = Duration::from_secs(20);

/// How often [`confirm_in_use`] re-stats `db/` while watching.
const CONFIRM_TICK: Duration = Duration::from_millis(500);

/// The most recent modification time of anything directly inside `db/`.
///
/// Not recursive: a leveldb directory is flat, and a nested directory there
/// would not be something the game writes on its autosave tick.
///
/// Public because [`crate::writemark`] records and compares exactly this
/// value; the two must agree on what "the newest write" means or the equality
/// test between them is meaningless.
pub fn newest_write(db: &Path) -> Option<SystemTime> {
    std::fs::read_dir(db)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

/// Whether `db` was written recently enough to *suspect* Minecraft is holding
/// this world open.
///
/// This is the instant first phase and it is deliberately trigger-happy. A
/// positive is not an answer — [`confirm_in_use`] decides. The common case is
/// a negative, which is free.
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

/// What a write to an open world would put at risk.
///
/// The two callers face genuinely different dangers, and telling a user their
/// deletion "would be silently discarded" when the real hazard is a corrupted
/// database misdescribes the thing they are being protected from. The caller
/// knows which it is; the error carries it so `report` can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtRisk {
    /// `level.dat`. The game reads it into memory when the world loads and
    /// rewrites it from memory on every save, so a write underneath it
    /// succeeds, reads back correct, and is gone at the next save.
    LevelDat,
    /// The world's database. Opening it is itself a write, and a second writer
    /// against a live leveldb is how a save gets corrupted rather than merely
    /// stale — a worse failure than the silent revert, and not a recoverable
    /// one.
    Database,
}

/// Watches `db` for a *new* write, to tell a live world from a finished one.
///
/// [`looks_in_use`] cannot make this distinction, and that is a real problem
/// rather than a theoretical one: `delete` writes `db/`, so its own write made
/// the next `construct delete` against the same world look exactly like a
/// running game. The user was told Minecraft had the world open when the
/// recent write was this tool's.
///
/// What separates them is that a live world keeps writing and a finished
/// command does not. So: sample until either a new write appears — in which
/// case something is actively writing and the answer is yes, immediately — or
/// [`CONFIRM_WATCH`] elapses with the file set unchanged, in which case the
/// recent write was a one-off and the answer is no.
///
/// The early bail matters for the cost: a genuinely live world is caught in
/// about one autosave gap, not the full watch. Only the "it was us" case pays
/// the whole [`CONFIRM_WATCH`], and only when a write path runs twice in quick
/// succession.
///
/// Deliberately portable. There are exact answers available per-platform — a
/// held `flock` where the game takes one, `/proc/*/fd` on Linux — but each is
/// one platform's, and a check that is precise on Linux and absent on Windows
/// is worse than one that behaves the same everywhere.
///
/// Only meaningful after [`looks_in_use`] returns true; call it on a quiet
/// database and it just waits.
pub fn confirm_in_use(db: &Path) -> bool {
    watch_for_writes(db, CONFIRM_WATCH, CONFIRM_TICK)
}

/// The body of [`confirm_in_use`], with its timings exposed so tests need not
/// take [`CONFIRM_WATCH`] seconds each.
fn watch_for_writes(db: &Path, watch: Duration, tick: Duration) -> bool {
    let Some(baseline) = newest_write(db) else {
        // Nothing to watch. `looks_in_use` would not have sent us here.
        return false;
    };
    let deadline = std::time::Instant::now() + watch;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(tick);
        match newest_write(db) {
            // A write landed while we watched. Something is driving this
            // database right now, and it is not us — we are asleep.
            Some(now) if now > baseline => return true,
            // The newest file went backwards or vanished: a compaction
            // replaced it. That is still an active writer.
            None => return true,
            _ => {}
        }
    }
    false
}

/// Refuses when Minecraft appears to have `world` open.
///
/// Callers on a write path must go through this *before* taking a backup or
/// opening anything, so that a refused command leaves no trace at all. Read
/// paths must *not* call it: reads work from a snapshot copy and are safe at
/// any time.
pub fn refuse_if_in_use(world: &World, at_risk: AtRisk) -> Result<()> {
    let db = world.db_path();

    // Phase one: instant, and almost always the answer. A world nobody has
    // touched in `ACTIVITY_WINDOW` is free, and that is the common case.
    if !looks_in_use(&db) {
        return Ok(());
    }

    // Phase two: was that recent write ours? `delete` writes `db/`, so a
    // second db-touching command inside the window used to be indistinguishable
    // from a running game and paid a full `CONFIRM_WATCH` to find out
    // otherwise. The mark answers it instantly. A non-match proves nothing on
    // its own — no mark, an unreadable one, a world someone else touched — so
    // it falls through rather than refusing.
    if crate::writemark::left_by_us(world) {
        return Ok(());
    }

    // Phase three: watch. The only phase that can refuse, and the fallback
    // whenever the mark could not answer.
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

    /// Millisecond timings, so a test costs milliseconds rather than
    /// `CONFIRM_WATCH` seconds. The logic under test is the comparison, not
    /// the constants.
    const FAST_WATCH: Duration = Duration::from_millis(300);
    const FAST_TICK: Duration = Duration::from_millis(20);

    #[test]
    fn a_database_nobody_is_writing_is_not_confirmed() {
        // The case that was broken: `delete` writes `db/`, so its own write
        // made the next run look like a live game. Nothing writes during the
        // watch, so this must come back false however recent that write was.
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
        // A stand-in for the autosave tick: one write, partway through.
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
        // Bedrock compacts as well as appending. A newest-mtime that goes
        // backwards or vanishes is still evidence of a writer, not of quiet.
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
        // The property that keeps phase two from reintroducing the false
        // negative: a live world must be unable to sit quiet for the whole
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

    /// Against a world Minecraft actually has open, named by
    /// `CONSTRUCT_LIVE_WORLD_DB`. Ignored by default: it needs a running game.
    /// Calls only the detection functions — it never opens the database.
    #[test]
    #[ignore = "needs a world open in Minecraft"]
    fn a_real_live_world_is_detected() {
        let db = std::path::PathBuf::from(std::env::var("CONSTRUCT_LIVE_WORLD_DB").unwrap());
        assert!(db.is_dir(), "no such db: {}", db.display());
        let t0 = std::time::Instant::now();
        assert!(looks_in_use(&db), "phase one must suspect a live world");
        assert!(confirm_in_use(&db), "phase two must confirm a live world");
        println!(
            "confirmed live in {:?} (watch cap {CONFIRM_WATCH:?})",
            t0.elapsed()
        );
    }

    /// A planted mark must not suppress detection of a world the game really
    /// has open. This is the corrupting direction, checked against the real
    /// thing rather than a simulator. Calls only detection functions; it never
    /// opens the database.
    #[test]
    #[ignore = "needs a world open in Minecraft"]
    fn a_mark_cannot_mask_a_real_live_world() {
        let db = std::path::PathBuf::from(std::env::var("CONSTRUCT_LIVE_WORLD_DB").unwrap());
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

        // Plant a mark claiming the newest write right now is ours.
        crate::writemark::record_in(marks.path(), &world);

        // The game writes every 5-10s, so within one gap the mark is stale.
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

    /// The inverse, against a world that is closed.
    #[test]
    #[ignore = "needs a closed world"]
    fn a_real_closed_world_is_not_detected() {
        let db = std::path::PathBuf::from(std::env::var("CONSTRUCT_IDLE_WORLD_DB").unwrap());
        assert!(db.is_dir(), "no such db: {}", db.display());
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
