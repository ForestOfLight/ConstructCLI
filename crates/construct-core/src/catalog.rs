//! One structure namespace over both sources.
//!
//! Construct presents world structures and pack structures as a single in-game
//! list, so the CLI does too — two lists would model it worse than the thing it
//! drives. Stage 1 populates only [`Source::World`]; stage 2 adds packs.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Where the bytes live, for [`Source::Pack`]. `None` for world structures,
    /// which live in a database rather than a file.
    pub path: Option<std::path::PathBuf>,
}

/// The order `list` presents and every command resolves against.
///
/// Name first, then source, so that two entries sharing a display name across
/// sources have a stated order rather than one inherited from concatenation.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.source.cmp(&b.source)));
}

/// Every structure in a world's database.
pub fn from_world(store: &dyn StructureStore) -> Result<Vec<Entry>> {
    let mut out: Vec<Entry> = store
        .sizes()?
        .into_iter()
        .map(|(id, size_bytes)| Entry {
            name: key::display_name(&id).to_string(),
            id,
            source: Source::World,
            size_bytes,
            path: None,
        })
        .collect();
    sort(&mut out);
    Ok(out)
}

/// Every structure file in a pack.
pub fn from_pack(pack_dir: &Path) -> Vec<Entry> {
    crate::pack::structures::list(pack_dir)
        .into_iter()
        .map(|s| Entry {
            name: s.name,
            id: s.id,
            source: Source::Pack,
            size_bytes: s.size_bytes,
            path: Some(s.path),
        })
        .collect()
}

/// The single list Construct presents in-game, over both sources.
///
/// Construct's own list lets a pack structure shadow a world structure of the
/// same name. This does not: §5 refuses an ambiguous name rather than picking a
/// winner, so both entries survive here and `resolve` reports the collision.
pub fn unify(world: Vec<Entry>, pack: Vec<Entry>) -> Vec<Entry> {
    let mut all = world;
    all.extend(pack);
    sort(&mut all);
    all
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
            sources: matches
                .iter()
                .map(|e| e.source.as_str().to_string())
                .collect(),
        }),
    }
}

/// The bytes behind an entry, wherever they live.
///
/// A world entry needs the store it came from; a pack entry is a file. Either
/// way the bytes are a complete `.mcstructure` — that is the byte-transparency
/// property this whole tool rests on.
pub fn read_entry(entry: &Entry, store: Option<&dyn StructureStore>) -> Result<Vec<u8>> {
    match (&entry.path, store) {
        (Some(path), _) => Ok(std::fs::read(path)?),
        (None, Some(store)) => store
            .get(&entry.id)?
            .ok_or_else(|| CoreError::StructureNotFound {
                name: entry.name.clone(),
                near: Vec::new(),
            }),
        (None, None) => Err(CoreError::StructureNotFound {
            name: entry.name.clone(),
            near: Vec::new(),
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
                path: None,
            },
            Entry {
                name: "barn".into(),
                id: "mystructure:barn".into(),
                source: Source::World,
                size_bytes: 4,
                path: None,
            },
            Entry {
                name: "tower".into(),
                id: "mystructure:tower".into(),
                source: Source::Pack,
                size_bytes: 31,
                path: None,
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
            path: None,
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
            path: None,
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

    #[test]
    fn entries_sort_by_name_then_source() {
        // Two sources can hold the same display name; the order between them is
        // stated rather than inherited from concatenation order.
        let entries = vec![
            Entry {
                name: "a".into(),
                id: "mystructure:a".into(),
                source: Source::Pack,
                size_bytes: 1,
                path: None,
            },
            Entry {
                name: "a".into(),
                id: "mystructure:a".into(),
                source: Source::World,
                size_bytes: 1,
                path: None,
            },
        ];
        let mut sorted = entries.clone();
        sort(&mut sorted);
        assert_eq!(sorted[0].source, Source::World);
        assert_eq!(sorted[1].source, Source::Pack);
    }

    #[test]
    fn read_entry_reads_a_pack_entry_from_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("barn.mcstructure");
        std::fs::write(&path, b"pack-bytes").unwrap();
        let entry = Entry {
            name: "barn".into(),
            id: "mystructure:barn".into(),
            source: Source::Pack,
            size_bytes: 10,
            path: Some(path),
        };
        assert_eq!(read_entry(&entry, None).unwrap(), b"pack-bytes");
    }

    #[test]
    fn read_entry_reads_a_world_entry_from_the_store() {
        let store = MemoryStore::with(&[("barn", b"world-bytes")]);
        let entry = Entry {
            name: "barn".into(),
            id: "mystructure:barn".into(),
            source: Source::World,
            size_bytes: 11,
            path: None,
        };
        assert_eq!(read_entry(&entry, Some(&store)).unwrap(), b"world-bytes");
    }

    #[test]
    fn read_entry_without_a_store_for_a_world_entry_is_not_found() {
        let entry = Entry {
            name: "barn".into(),
            id: "mystructure:barn".into(),
            source: Source::World,
            size_bytes: 11,
            path: None,
        };
        assert!(matches!(
            read_entry(&entry, None),
            Err(CoreError::StructureNotFound { .. })
        ));
    }

    #[test]
    fn an_ambiguous_name_names_the_sources_that_matched() {
        let mut e = entries();
        e.push(Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::Pack,
            size_bytes: 9,
            path: None,
        });
        let CoreError::AmbiguousStructure { sources, .. } = resolve("house", &e, None).unwrap_err()
        else {
            panic!("expected AmbiguousStructure");
        };
        assert_eq!(sources, vec!["world".to_string(), "pack".to_string()]);
    }
}
