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
        let db = Database::open(path).map_err(|e| CoreError::Db(e.to_string()))?;
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
