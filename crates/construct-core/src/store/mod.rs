pub mod bedrock;
pub mod key;

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
