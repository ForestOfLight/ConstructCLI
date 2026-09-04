//! The `bedrock_level` backend: FFI to the leveldb fork Minecraft itself uses.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use bedrock_level::db::Database;
use std::path::Path;

pub struct BedrockStore {
    db: Database,
}

impl BedrockStore {
    /// Opens the database at a world's `db` directory.
    ///
    /// Note this acquires `db/LOCK`: leveldb's C++ API has no read-only open,
    /// so a world currently open in Minecraft cannot be opened here. Callers
    /// that can tolerate a stale read should go through
    /// [`crate::store::open_world_store`], which falls back to a snapshot.
    pub fn open(db_dir: &Path) -> Result<Self> {
        // `Database::open` takes `AsRef<str>`, not a path.
        let path = db_dir.to_str().ok_or_else(|| {
            CoreError::Db(format!("non-UTF-8 database path: {}", db_dir.display()))
        })?;
        let db = Database::open(path).map_err(|e| CoreError::Db(e.to_string()))?;
        Ok(Self { db })
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
        // Stage 1 read every structure twice on a `list` — 63.5 MB on a real
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
/// This enforces the copy-before-open invariant that spec §8 rests on: a read
/// only ever opens a snapshot copy of `db/`, never a world's own database.
/// `snapshot::open_via_snapshot` calls this on every snapshot open, immediately
/// before `BedrockStore::open`, so a future refactor that accidentally passes
/// the original path down this function aborts loudly instead of silently
/// rewriting somebody's save. Tests also open real leveldb databases directly,
/// and use this guard to keep those pointed at a temp directory too. Either
/// way the cost of a path outside temp is a corrupted save, so the check is a
/// hard panic rather than a warning.
pub fn guard_test_path(path: &Path) {
    let tmp = std::env::temp_dir();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let tmp = tmp.canonicalize().unwrap_or(tmp);
    assert!(
        canonical.starts_with(&tmp),
        "refusing to open a database outside a temp directory: {}",
        canonical.display()
    );
}
