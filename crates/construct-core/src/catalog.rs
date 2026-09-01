//! One structure namespace over both sources.
//!
//! Construct presents world structures and pack structures as a single in-game
//! list, so the CLI does too — two lists would model it worse than the thing it
//! drives. Stage 1 populates only [`Source::World`]; stage 2 adds packs.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    World,
    Pack,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::World => "world",
            Source::Pack => "pack",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the user types and sees: bare for `mystructure`, qualified otherwise.
    pub name: String,
    /// The fully qualified id, always carrying a namespace.
    pub id: String,
    pub source: Source,
    pub size_bytes: u64,
}

/// Every structure in a world's database.
pub fn from_world(store: &dyn StructureStore) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for id in store.ids()? {
        let size_bytes = store.get(&id)?.map(|b| b.len() as u64).unwrap_or(0);
        out.push(Entry {
            name: key::display_name(&id).to_string(),
            id,
            source: Source::World,
            size_bytes,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Finds exactly one structure by name, never guessing between sources.
pub fn resolve(name: &str, entries: &[Entry], source: Option<Source>) -> Result<Entry> {
    let qualified = key::qualify(name);
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.name == name || e.id == qualified)
        .filter(|e| source.is_none_or(|s| e.source == s))
        .collect();

    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(CoreError::StructureNotFound {
            name: name.to_string(),
            near: near_matches(name, entries, source),
        }),
        _ => Err(CoreError::AmbiguousStructure {
            name: name.to_string(),
        }),
    }
}

fn near_matches(needle: &str, entries: &[Entry], source: Option<Source>) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut out: Vec<String> = entries
        .iter()
        .filter(|e| source.is_none_or(|s| e.source == s))
        .filter(|e| e.name.to_lowercase().contains(&needle))
        .map(|e| e.name.clone())
        .collect();
    out.truncate(5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    fn entries() -> Vec<Entry> {
        vec![
            Entry {
                name: "house".into(),
                id: "mystructure:house".into(),
                source: Source::World,
                size_bytes: 12,
            },
            Entry {
                name: "barn".into(),
                id: "mystructure:barn".into(),
                source: Source::World,
                size_bytes: 4,
            },
            Entry {
                name: "tower".into(),
                id: "mystructure:tower".into(),
                source: Source::Pack,
                size_bytes: 31,
            },
        ]
    }

    #[test]
    fn builds_entries_from_a_world_store() {
        let store = MemoryStore::with(&[("house", b"abc"), ("understudy:players", b"de")]);
        let mut got = from_world(&store).unwrap();
        got.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(got[0].name, "house");
        assert_eq!(got[0].id, "mystructure:house");
        assert_eq!(got[0].size_bytes, 3);
        assert_eq!(got[0].source, Source::World);
        // A non-default namespace stays visible in the display name.
        assert_eq!(got[1].name, "understudy:players");
    }

    #[test]
    fn resolves_a_unique_bare_name() {
        assert_eq!(
            resolve("house", &entries(), None).unwrap().id,
            "mystructure:house"
        );
    }

    #[test]
    fn a_name_in_both_sources_is_an_error_pointing_at_source() {
        let mut e = entries();
        e.push(Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::Pack,
            size_bytes: 9,
        });
        assert!(matches!(
            resolve("house", &e, None),
            Err(CoreError::AmbiguousStructure { .. })
        ));
    }

    #[test]
    fn source_disambiguates_a_name_present_in_both() {
        let mut e = entries();
        e.push(Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::Pack,
            size_bytes: 9,
        });
        assert_eq!(
            resolve("house", &e, Some(Source::Pack)).unwrap().size_bytes,
            9
        );
        assert_eq!(
            resolve("house", &e, Some(Source::World))
                .unwrap()
                .size_bytes,
            12
        );
    }

    #[test]
    fn a_missing_structure_suggests_near_matches() {
        let CoreError::StructureNotFound { near, .. } =
            resolve("hous", &entries(), None).unwrap_err()
        else {
            panic!("expected StructureNotFound");
        };
        assert!(near.contains(&"house".to_string()));
    }

    #[test]
    fn a_qualified_name_resolves() {
        assert_eq!(
            resolve("mystructure:house", &entries(), None).unwrap().name,
            "house"
        );
    }

    #[test]
    fn filtering_by_a_source_with_no_matches_is_not_found() {
        assert!(matches!(
            resolve("barn", &entries(), Some(Source::Pack)),
            Err(CoreError::StructureNotFound { .. })
        ));
    }
}
