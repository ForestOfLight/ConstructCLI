pub mod bedrock;
pub mod key;
pub mod snapshot;

use crate::discovery::World;
use crate::error::Result;
use std::collections::BTreeMap;

/// Read access to a world's structures.
///
/// A trait rather than a concrete type so the leveldb backend can be replaced —
/// `bedrock-leveldb` compiles to WASM and never takes the LOCK, which a future
/// web UI would want.
pub trait StructureStore {
    /// Every structure id in the store, qualified (`mystructure:house`).
    fn ids(&self) -> Result<Vec<String>>;

    /// The raw bytes of one structure, which are byte-identical to a
    /// `.mcstructure` file. `id` may be bare or qualified.
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>>;
}

/// An in-memory store, for testing everything above this layer without a database.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore(pub BTreeMap<String, Vec<u8>>);

impl MemoryStore {
    pub fn with(entries: &[(&str, &[u8])]) -> Self {
        Self(
            entries
                .iter()
                .map(|(k, v)| (key::qualify(k), v.to_vec()))
                .collect(),
        )
    }
}

impl StructureStore for MemoryStore {
    fn ids(&self) -> Result<Vec<String>> {
        Ok(self.0.keys().cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.get(&key::qualify(id)).cloned())
    }
}

/// An opened store plus whatever it needs to stay alive.
pub struct OpenedStore {
    pub(crate) inner: Box<dyn StructureStore>,
    /// Held so the snapshot directory outlives the database handle.
    pub(crate) _snapshot: Option<tempfile::TempDir>,
    /// `Some(bytes)` when the read came from a snapshot rather than the world.
    pub via_snapshot: Option<u64>,
}

impl StructureStore for OpenedStore {
    fn ids(&self) -> Result<Vec<String>> {
        self.inner.ids()
    }
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.inner.get(id)
    }
}

/// Opens a world's structures for reading.
///
/// This *always* copies `db/` and opens the copy. Opening a leveldb database runs
/// recovery and rewrites it, so there is no such thing as a read-only open with
/// this backend — the only safe read is one that never touches the original.
/// There is deliberately no direct path and no fallback logic here.
pub fn open_world_store(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();
    if !db.is_dir() {
        return Err(crate::error::CoreError::Db(format!(
            "no database at {}",
            db.display()
        )));
    }
    snapshot::open_via_snapshot(world)
}
