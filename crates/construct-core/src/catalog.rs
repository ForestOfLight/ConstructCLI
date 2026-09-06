//! One structure namespace over both sources.
//!
//! Construct presents world structures and pack structures as a single in-game
//! list, so the CLI does too — two lists would model it worse than the thing it
//! drives. Stage 1 populates only [`Source::World`]; stage 2 adds packs.

use crate::error::{CoreError, Result};
use crate::pack::Scope;
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
    /// For a pack entry, whether that pack serves this world alone or every
    /// world the shared copy of Construct serves. `None` for world entries,
    /// which are per-world by construction.
    pub scope: Option<Scope>,
}

impl Entry {
    /// How this entry is named in a listing and in an ambiguity error.
    ///
    /// `pack` alone was ambiguous once a world could see two packs at
    /// once — an error reading "found in pack and pack" names nothing.
    pub fn source_label(&self) -> &'static str {
        match (self.source, self.scope) {
            (Source::World, _) => "world",
            (Source::Pack, Some(Scope::World)) => "pack:world",
            (Source::Pack, Some(Scope::Shared)) => "pack:shared",
            (Source::Pack, None) => "pack",
        }
    }
}

/// The order `structures` presents and every command resolves against.
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
            scope: None,
        })
        .collect();
    sort(&mut out);
    Ok(out)
}

/// Every structure file in a pack, tagged with how far that pack reaches.
pub fn from_pack(pack_dir: &Path, scope: Scope) -> Vec<Entry> {
    crate::pack::structures::list(pack_dir)
        .into_iter()
        .map(|s| Entry {
            name: s.name,
            id: s.id,
            source: Source::Pack,
            size_bytes: s.size_bytes,
            path: Some(s.path),
            scope: Some(scope),
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
///
/// `pack_scope` narrows to one pack when a world sees the same name in two of
/// them — its own and the shared copy of Construct. Without it such a name has no
/// single answer, and this refuses rather than picking: the two files are
/// different structures that happen to share a name, and guessing which one a
/// `delete` meant is the guess with the worst consequence.
pub fn resolve(
    name: &str,
    entries: &[Entry],
    source: Option<Source>,
    pack_scope: Option<Scope>,
) -> Result<Entry> {
    let qualified = key::qualify(name);
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.name == name || e.id == qualified)
        .filter(|e| source.is_none_or(|s| e.source == s))
        // A pack filter is about packs: naming one implies pack entries, so a
        // world-database entry (which has no pack scope) falls out here.
        .filter(|e| pack_scope.is_none_or(|s| e.scope == Some(s)))
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
                .map(|e| e.source_label().to_string())
                .collect(),
        }),
    }
}

/// Every structure matching a name, for the one command that wants them all.
///
/// `resolve` refuses when a name is in two places, because `export -o` and
/// `copy` have to pick one and guessing is the wrong answer. `delete` is the
/// exception: "remove this name from this world" is a complete instruction
/// with no guess in it, so a name in the world's database *and* in both packs
/// yields three entries and all three go. `--source` and `--pack` still narrow
/// it for anyone who wants one copy gone and the others kept.
///
/// Zero matches is still an error, with the same near-match suggestions
/// `resolve` offers.
pub fn resolve_all(
    name: &str,
    entries: &[Entry],
    source: Option<Source>,
    pack_scope: Option<Scope>,
) -> Result<Vec<Entry>> {
    let qualified = key::qualify(name);
    let matches: Vec<Entry> = entries
        .iter()
        .filter(|e| e.name == name || e.id == qualified)
        .filter(|e| source.is_none_or(|s| e.source == s))
        // A pack filter is about packs: naming one implies pack entries, so a
        // world-database entry (which has no pack scope) falls out here.
        .filter(|e| pack_scope.is_none_or(|s| e.scope == Some(s)))
        .cloned()
        .collect();

    if matches.is_empty() {
        return Err(CoreError::StructureNotFound {
            name: name.to_string(),
            near: near_matches(name, entries, source),
        });
    }
    Ok(matches)
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
                scope: None,
            },
            Entry {
                name: "barn".into(),
                id: "mystructure:barn".into(),
                source: Source::World,
                size_bytes: 4,
                path: None,
                scope: None,
            },
            Entry {
                name: "tower".into(),
                id: "mystructure:tower".into(),
                source: Source::Pack,
                size_bytes: 31,
                path: None,
                scope: None,
            },
        ]
    }

    /// `house` in the world database, in the world's own pack, and in the
    /// shared one — the three-way collision `delete` sweeps up.
    fn house_everywhere() -> Vec<Entry> {
        let pack = |scope| Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::Pack,
            size_bytes: 1,
            path: Some(std::path::PathBuf::from("/packs/house.mcstructure")),
            scope: Some(scope),
        };
        vec![
            Entry {
                name: "house".into(),
                id: "mystructure:house".into(),
                source: Source::World,
                size_bytes: 1,
                path: None,
                scope: None,
            },
            pack(Scope::World),
            pack(Scope::Shared),
        ]
    }

    #[test]
    fn resolve_all_returns_every_copy_of_a_name() {
        // What `resolve` refuses, this returns. `delete` is the only caller:
        // "remove this name from this world" needs no guess, so all three go.
        let got = resolve_all("house", &house_everywhere(), None, None).unwrap();
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn resolve_all_still_honours_source_and_pack_filters() {
        let entries = house_everywhere();
        let world = resolve_all("house", &entries, Some(Source::World), None).unwrap();
        assert_eq!(world.len(), 1);
        assert_eq!(world[0].source, Source::World);

        let packs = resolve_all("house", &entries, Some(Source::Pack), None).unwrap();
        assert_eq!(packs.len(), 2);

        // A pack filter is about packs, so the database row falls out too.
        let shared = resolve_all("house", &entries, None, Some(Scope::Shared)).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].scope, Some(Scope::Shared));
    }

    #[test]
    fn resolve_all_still_refuses_a_name_that_is_nowhere() {
        // Zero matches is the one case `resolve` and `resolve_all` agree on.
        let err = resolve_all("nope", &house_everywhere(), None, None).unwrap_err();
        assert!(matches!(err, CoreError::StructureNotFound { .. }));
    }

    #[test]
    fn resolve_all_finds_a_name_by_its_qualified_form() {
        let got = resolve_all("mystructure:house", &house_everywhere(), None, None).unwrap();
        assert_eq!(got.len(), 3);
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
            resolve("house", &entries(), None, None).unwrap().id,
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
            scope: None,
        });
        assert!(matches!(
            resolve("house", &e, None, None),
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
            scope: None,
        });
        assert_eq!(
            resolve("house", &e, Some(Source::Pack), None)
                .unwrap()
                .size_bytes,
            9
        );
        assert_eq!(
            resolve("house", &e, Some(Source::World), None)
                .unwrap()
                .size_bytes,
            12
        );
    }

    #[test]
    fn a_missing_structure_suggests_near_matches() {
        let CoreError::StructureNotFound { near, .. } =
            resolve("hous", &entries(), None, None).unwrap_err()
        else {
            panic!("expected StructureNotFound");
        };
        assert!(near.contains(&"house".to_string()));
    }

    #[test]
    fn a_qualified_name_resolves() {
        assert_eq!(
            resolve("mystructure:house", &entries(), None, None)
                .unwrap()
                .name,
            "house"
        );
    }

    #[test]
    fn filtering_by_a_source_with_no_matches_is_not_found() {
        assert!(matches!(
            resolve("barn", &entries(), Some(Source::Pack), None),
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
                scope: None,
            },
            Entry {
                name: "a".into(),
                id: "mystructure:a".into(),
                source: Source::World,
                size_bytes: 1,
                path: None,
                scope: None,
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
            scope: None,
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
            scope: None,
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
            scope: None,
        };
        assert!(matches!(
            read_entry(&entry, None),
            Err(CoreError::StructureNotFound { .. })
        ));
    }

    #[test]
    fn a_pack_filter_picks_between_two_packs() {
        // The case the filter exists for: one name, two packs serving one
        // world. Without it this name resolves to nothing usable.
        let e = vec![
            Entry {
                name: "house".into(),
                id: "mystructure:house".into(),
                source: Source::Pack,
                size_bytes: 1,
                path: None,
                scope: Some(Scope::Shared),
            },
            Entry {
                name: "house".into(),
                id: "mystructure:house".into(),
                source: Source::Pack,
                size_bytes: 2,
                path: None,
                scope: Some(Scope::World),
            },
        ];
        assert!(resolve("house", &e, None, None).is_err(), "ambiguous");
        assert_eq!(
            resolve("house", &e, None, Some(Scope::Shared))
                .unwrap()
                .size_bytes,
            1
        );
        assert_eq!(
            resolve("house", &e, None, Some(Scope::World))
                .unwrap()
                .size_bytes,
            2
        );
    }

    #[test]
    fn a_pack_filter_excludes_world_database_entries() {
        // A pack filter is about packs. A world's database structure has no
        // pack to be in, so naming one drops it — otherwise `--pack world`
        // would still resolve to a database entry and delete would refuse a
        // world delete it never meant to attempt.
        let e = vec![Entry {
            name: "house".into(),
            id: "mystructure:house".into(),
            source: Source::World,
            size_bytes: 1,
            path: None,
            scope: None,
        }];
        assert!(resolve("house", &e, None, None).is_ok());
        assert!(matches!(
            resolve("house", &e, None, Some(Scope::World)),
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
            scope: None,
        });
        let CoreError::AmbiguousStructure { sources, .. } =
            resolve("house", &e, None, None).unwrap_err()
        else {
            panic!("expected AmbiguousStructure");
        };
        assert_eq!(sources, vec!["world".to_string(), "pack".to_string()]);
    }
}
