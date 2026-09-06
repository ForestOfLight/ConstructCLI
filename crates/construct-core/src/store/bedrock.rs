//! The `bedrock_level` backend: FFI to the leveldb fork Minecraft itself uses.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use bedrock_level::db::Database;
use std::path::Path;

pub struct BedrockStore {
    db: Database,
}

impl BedrockStore {
    /// Opens a **copy** of a world's `db` directory.
    ///
    /// Every read goes through here. Opening a leveldb runs recovery and
    /// rewrites the file set, so a read may only ever open a copy — the guard
    /// below enforces that at the point of the open rather than in the caller,
    /// so no future refactor can route a world's own path down this function.
    ///
    /// Writes do not come this way. [`BedrockStore::open_live`] is the one
    /// entry point that touches a real world.
    pub fn open_copy(db_dir: &Path) -> Result<Self> {
        guard_copy_path(db_dir);
        Self::open_unguarded(db_dir)
    }

    /// Opens a world's **own** database, for writing.
    ///
    /// This is the only function in the tool that opens a database Minecraft
    /// owns, and opening it is itself a write (spec §8). It takes a [`World`]
    /// rather than a path so it cannot be reached by handing the wrong path to
    /// a general-purpose opener; the sole caller is the `delete` write path,
    /// which refuses first when the world looks in use.
    pub fn open_live(world: &World) -> Result<Self> {
        Self::open_unguarded(&world.db_path())
    }

    fn open_unguarded(db_dir: &Path) -> Result<Self> {
        // `Database::open` takes `AsRef<str>`, not a path.
        let path = db_dir.to_str().ok_or_else(|| {
            CoreError::Db(format!("non-UTF-8 database path: {}", db_dir.display()))
        })?;
        let db = Database::open(path).map_err(|e| CoreError::Db(e.to_string()))?;
        Ok(Self { db })
    }

    /// Removes one structure key, reporting whether it was there to remove.
    ///
    /// leveldb's `Delete` reports success for a key that was never present, so
    /// the `get` beforehand is what makes "deleted" mean something. The `get`
    /// afterwards is the §15 verification: the removal is confirmed against the
    /// same handle before the caller is told it happened.
    pub fn remove(&self, id: &str) -> Result<bool> {
        // Deliberately a loop rather than `find(|k| matches!(get(k), Ok(Some(_))))`:
        // that spelling reads a database error as "not present" and would
        // report nothing to delete when the truth is that the lookup failed.
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
        // `Iterator` is implemented for `&mut Keys`, not `Keys`, so the binding
        // must be mutable and iterated by reference.
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
        // One pass over the iterator, which yields key and value together.
        // Stage 1 read every structure twice on a `structures` — 63.5 MB on a real
        // 910-structure world.
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
/// This enforces the copy-before-open invariant spec §8 rests on: a read only
/// ever opens a snapshot copy of `db/`, never a world's own database. It lives
/// inside [`BedrockStore::open_copy`] rather than in the caller, so a refactor
/// that routes a world's own path down the read path aborts loudly instead of
/// silently rewriting somebody's save. Tests open real leveldb databases too,
/// and their fixtures unpack into a temp directory for the same reason.
///
/// The write path deliberately does not come through here: `delete` opens a
/// world's own database on purpose, via [`BedrockStore::open_live`], which
/// takes a `&World` so it cannot be reached by accident. Either way the cost of
/// the wrong path is a corrupted save, so this is a hard panic rather than a
/// warning.
pub fn guard_copy_path(path: &Path) {
    let tmp = std::env::temp_dir();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let tmp = tmp.canonicalize().unwrap_or(tmp);
    assert!(
        canonical.starts_with(&tmp),
        "refusing to open a database outside a temp directory: {}",
        canonical.display()
    );
}
