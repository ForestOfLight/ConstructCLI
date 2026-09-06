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

    /// Every structure id with the byte length of its value.
    ///
    /// Separate from `ids` because `list` needs both and the leveldb backend can
    /// produce them in a single pass. The default implementation is the obvious
    /// two-step; backends that can do better should.
    fn sizes(&self) -> Result<Vec<(String, u64)>> {
        let mut out = Vec::new();
        for id in self.ids()? {
            let len = self.get(&id)?.map(|b| b.len() as u64).unwrap_or(0);
            out.push((id, len));
        }
        Ok(out)
    }
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
        Ok(self
            .0
            .get(&key::qualify(id))
            .or_else(|| self.0.get(id))
            .cloned())
    }
}

/// An opened store plus whatever it needs to stay alive.
pub struct OpenedStore {
    pub(crate) inner: Box<dyn StructureStore>,
    /// Held so the snapshot directory outlives the database handle.
    pub(crate) _snapshot: Option<tempfile::TempDir>,
    /// `Some(bytes)` when the read came from a snapshot rather than the world.
    ///
    /// The figure is bytes *copied*, which on a linking snapshot is far less
    /// than the size of `db/`: the table files are hardlinked and cost nothing.
    /// See [`snapshot::link_or_copy_dir`].
    pub via_snapshot: Option<u64>,
}

impl StructureStore for OpenedStore {
    fn ids(&self) -> Result<Vec<String>> {
        self.inner.ids()
    }
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.inner.get(id)
    }
    fn sizes(&self) -> Result<Vec<(String, u64)>> {
        // Must forward rather than fall back to the trait default, or every real
        // caller — which always goes through `OpenedStore` — loses the one-pass
        // `BedrockStore` override this method exists for.
        self.inner.sizes()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_store_fetches_an_unqualified_id() {
        let mut map = BTreeMap::new();
        map.insert("foo".to_string(), b"bytes".to_vec());
        let store = MemoryStore(map);
        assert_eq!(store.get("foo").unwrap(), Some(b"bytes".to_vec()));
    }

    #[test]
    fn sizes_reports_every_id_with_its_length() {
        let store = MemoryStore::with(&[("house", b"abc"), ("barn", b"de")]);
        let mut got = store.sizes().unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("mystructure:barn".to_string(), 2),
                ("mystructure:house".to_string(), 3),
            ]
        );
    }
}
