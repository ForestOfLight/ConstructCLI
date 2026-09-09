//! The `bedrock_level` backend: FFI to the leveldb fork Minecraft itself uses.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use bedrock_level::db::Database;
use std::path::{Path, PathBuf};

pub struct BedrockStore {
    db: Database,
}

impl BedrockStore {
    /// Opens a **copy** of a world's `db` directory.
    ///
    /// Every read goes through here. Opening a leveldb runs recovery and
    /// rewrites the file set, so a read may only open a copy. The guard below
    /// enforces that at the open rather than in the caller, so no refactor can
    /// route a world's own path down this function.
    ///
    /// Writes go through [`BedrockStore::open_live`], the one entry point that
    /// touches a real world.
    pub fn open_copy(db_dir: &Path) -> Result<Self> {
        guard_copy_path(db_dir);
        Self::open_unguarded(db_dir)
    }

    /// Opens a world's **own** database, for writing.
    ///
    /// The only function here that opens a database Minecraft owns, and
    /// opening it is itself a write (§8). Takes a [`World`] rather than a path
    /// so it cannot be reached by handing the wrong path to a general-purpose
    /// opener. The sole caller is `delete`, which refuses first when the world
    /// looks in use.
    pub fn open_live(world: &World) -> Result<Self> {
        Self::open_unguarded(&world.db_path())
    }

    fn open_unguarded(db_dir: &Path) -> Result<Self> {
        let path = db_dir.to_str().ok_or_else(|| {
            CoreError::Db(format!("non-UTF-8 database path: {}", db_dir.display()))
        })?;
        let db = retry_past_transient_locks(|| {
            Database::open(path).map_err(|e| CoreError::Db(e.to_string()))
        })?;
        Ok(Self { db })
    }

    /// Removes one structure key, reporting whether it was there to remove.
    ///
    /// leveldb's `Delete` succeeds for a key that was never present, so the
    /// `get` beforehand is what makes "deleted" mean something. The `get`
    /// afterwards is the §15 verification, confirming the removal against the
    /// same handle before the caller is told it happened.
    pub fn remove(&self, id: &str) -> Result<bool> {
        let mut found = None;
        for k in key::candidates(id) {
            if self
                .db
                .get(&k)
                .map_err(|e| CoreError::Db(e.to_string()))?
                .is_some()
            {
                found = Some(k);
                break;
            }
        }
        let Some(key) = found else {
            return Ok(false);
        };
        self.db
            .remove(&key)
            .map_err(|e| CoreError::Db(e.to_string()))?;
        let still_there = self
            .db
            .get(&key)
            .map_err(|e| CoreError::Db(e.to_string()))?
            .is_some();
        if still_there {
            return Err(CoreError::Db(format!(
                "{id} was still present after being removed"
            )));
        }
        Ok(true)
    }
}

impl StructureStore for BedrockStore {
    fn ids(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut keys = self.db.keys();
        for kv in &mut keys {
            if let Some(id) = key::decode(&kv.key()) {
                out.push(id);
            }
        }
        Ok(out)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        for k in key::candidates(id) {
            if let Some(buf) = self.db.get(&k).map_err(|e| CoreError::Db(e.to_string()))? {
                return Ok(Some(buf.to_vec()));
            }
        }
        Ok(None)
    }

    fn sizes(&self) -> Result<Vec<(String, u64)>> {
        let mut out = Vec::new();
        let mut keys = self.db.keys();
        for kv in &mut keys {
            if let Some(id) = key::decode(&kv.key()) {
                out.push((id, kv.value().len() as u64));
            }
        }
        Ok(out)
    }
}

/// Retries `open` past a transient Windows sharing violation.
///
/// Opening a leveldb runs recovery, and recovery finishes by writing
/// `<db>/NNNNNN.dbtmp` and renaming it over `CURRENT`. Every open this tool
/// performs is of a directory whose files were written moments earlier — the
/// read path copies `CURRENT`, the log and the MANIFEST into a temp directory
/// and opens the copy immediately (see [`crate::store::snapshot`]). On Windows
/// a file just written is still open by whatever scanned it on close, so that
/// rename can lose a race and fail with `ERROR_SHARING_VIOLATION`, which the
/// leveldb fork surfaces as "The process cannot access the file because it is
/// being used by another process". Nothing about the database is wrong; the
/// next attempt milliseconds later succeeds. Unix has no such window, so this
/// never retries there — the messages simply do not match.
///
/// Only that violation and the access-denied a delete-pending `CURRENT`
/// reports are retried. Every other failure — a missing database, a corrupt
/// one, leveldb's own "already in use" when another handle holds `LOCK` —
/// returns on the first attempt, because waiting would not change it.
pub fn retry_past_transient_locks<T, E: std::fmt::Display>(
    open: impl FnMut() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    retry_with(open, &mut |ms| {
        std::thread::sleep(std::time::Duration::from_millis(ms))
    })
}

/// Milliseconds to wait before each retry: six attempts over ~310 ms.
///
/// Long enough for a scanner to let go of a file it has just read, short
/// enough that a genuine access-denied still fails promptly.
const RETRY_DELAYS_MS: [u64; 5] = [10, 20, 40, 80, 160];

/// [`retry_past_transient_locks`] with the waiting injected, so the tests below
/// can assert the schedule without spending it.
fn retry_with<T, E: std::fmt::Display>(
    mut open: impl FnMut() -> std::result::Result<T, E>,
    sleep: &mut dyn FnMut(u64),
) -> std::result::Result<T, E> {
    for delay in RETRY_DELAYS_MS {
        match open() {
            Err(e) if is_transient_lock(&e.to_string()) => sleep(delay),
            outcome => return outcome,
        }
    }
    open()
}

/// Whether a leveldb error message is one of the transient Windows locks.
fn is_transient_lock(message: &str) -> bool {
    // Matched on the message because the FFI reduces every leveldb status to
    // one, with no error code left to test. "already in use" is leveldb's own
    // `LOCK` refusal and deliberately does not match.
    message.contains("being used by another process") || message.contains("Access is denied")
}

/// Refuses any database path that is not under a temp directory.
///
/// Enforces the copy-before-open invariant of §8 at the open rather than in
/// the caller, so a refactor routing a world's own path down the read path
/// aborts loudly instead of rewriting somebody's save.
///
/// # Panics
///
/// On any path outside the temp directory. The cost of the wrong path is a
/// corrupted save, so this panics rather than warns. The write path does not
/// come through here — [`BedrockStore::open_live`] opens a world's own
/// database on purpose.
pub fn guard_copy_path(path: &Path) {
    let canonical = resolve_existing(path);
    let tmp = resolve_existing(&std::env::temp_dir());
    assert!(
        canonical.starts_with(&tmp),
        "refusing to open a database outside a temp directory: {}",
        canonical.display()
    );
}

fn resolve_existing(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cursor = path;
    let resolved = loop {
        match cursor.canonicalize() {
            Ok(resolved) => break resolved,
            Err(_) => match (cursor.parent(), cursor.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name);
                    cursor = parent;
                }
                _ => return path.to_path_buf(),
            },
        }
    };
    let mut out = resolved;
    out.extend(tail.iter().rev());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message the Windows runner produced, verbatim: trailing CRLF and
    /// all, since that is what leveldb's `Status::ToString` hands back.
    const SHARING_VIOLATION: &str = "IO error: C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\.tmpdyGXl4\\test_level\\db/000007.dbtmp: The process cannot access the file because it is being used by another process.\r\n";

    /// Runs `retry_with` over a canned sequence of outcomes, reporting how many
    /// attempts it made and how long it asked to wait between them.
    fn run(outcomes: &[&'static str]) -> (std::result::Result<(), String>, usize, Vec<u64>) {
        let mut attempts = 0;
        let mut waits = Vec::new();
        let outcome = retry_with(
            || {
                let message = outcomes.get(attempts).copied();
                attempts += 1;
                match message {
                    Some("") | None => Ok(()),
                    Some(m) => Err(m.to_string()),
                }
            },
            &mut |ms| waits.push(ms),
        );
        (outcome, attempts, waits)
    }

    #[test]
    fn a_sharing_violation_is_retried_until_the_open_succeeds() {
        let (outcome, attempts, waits) = run(&[SHARING_VIOLATION, SHARING_VIOLATION, ""]);
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(attempts, 3);
        assert_eq!(waits, vec![10, 20]);
    }

    #[test]
    fn a_sharing_violation_that_never_clears_gives_up_and_reports_it() {
        let (outcome, attempts, waits) = run(&[SHARING_VIOLATION; 20]);
        assert_eq!(outcome.unwrap_err(), SHARING_VIOLATION);
        // One attempt per delay, plus the final one after the last wait.
        assert_eq!(attempts, RETRY_DELAYS_MS.len() + 1);
        assert_eq!(waits, RETRY_DELAYS_MS.to_vec());
    }

    /// Waiting cannot make a missing database appear, and the caller is left
    /// holding the original error either way.
    #[test]
    fn an_ordinary_failure_is_not_retried() {
        let (outcome, attempts, waits) = run(&["IO error: db/CURRENT: No such file or directory"]);
        assert!(outcome.is_err());
        assert_eq!(attempts, 1);
        assert!(waits.is_empty());
    }

    /// leveldb's `LOCK` refusal reads as "in use" but is not this race: another
    /// handle holds the database open, and it will still hold it in 300 ms.
    #[test]
    fn the_lock_refusal_is_not_retried() {
        let (outcome, attempts, _) = run(&["IO error: This LevelDB database is already in use"]);
        assert!(outcome.is_err());
        assert_eq!(attempts, 1);
    }

    #[cfg(unix)]
    #[test]
    fn resolves_a_missing_path_reached_through_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let link = tmp.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert_eq!(
            resolve_existing(&link.join("no-db")),
            real.canonicalize().unwrap().join("no-db")
        );
    }

    #[test]
    fn resolves_a_path_that_exists() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_existing(tmp.path()),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn leaves_a_path_with_no_existing_ancestor_alone() {
        let path = Path::new("construct-no-such-ancestor/db");
        assert_eq!(resolve_existing(path), path);
    }
}
