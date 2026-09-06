//! One structure namespace over every place a structure can live.
//!
//! Construct presents world structures and pack structures as a single in-game
//! list, so the CLI does too — two lists would model it worse than the thing it
//! drives. [`Source`] is the one axis that says which place a row came from:
//! the world's database, a pack serving that world alone, or the shared copy of
//! Construct serving every world that uses it.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use std::path::Path;

/// Where a structure lives — the single axis `--source` selects on.
///
/// Ordered as declared: database, then the world's own pack, then the shared
/// copy. [`sort`] leans on that so two rows sharing a display name have a
/// stated order rather than one inherited from concatenation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The world's own leveldb database.
    WorldDb,
    /// A pack serving this world alone — its structures pack, or its own copy
    /// of Construct.
    WorldPack,
    /// The shared copy of Construct in `development_behavior_packs`, which
    /// serves every world using it.
    SharedPack,
}

impl Source {
    /// The JSON `source` value, spelled the same as the `--source` value that
    /// selects it: a machine reader can feed a row straight back to the CLI.
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::WorldDb => "world-db",
            Source::WorldPack => "world-pack",
            Source::SharedPack => "shared-pack",
        }
    }

    /// Whether the bytes are a file in a pack rather than a database value.
    /// The distinction the callers that never open a leveldb care about.
    pub fn is_pack(&self) -> bool {
        matches!(self, Source::WorldPack | Source::SharedPack)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the user types and sees: bare for `mystructure`, qualified otherwise.
    pub name: String,
    /// The fully qualified id, always carrying a namespace.
    pub id: String,
    /// Which of the three places this copy lives in.
    pub source: Source,
    pub size_bytes: u64,
    /// Where the bytes live, for a pack source. `None` for world-database
    /// structures, which live in a database rather than a file.
    pub path: Option<std::path::PathBuf>,
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
            source: Source::WorldDb,
            size_bytes,
            path: None,
        })
        .collect();
    sort(&mut out);
    Ok(out)
}

/// Every structure file in a pack, tagged with how far that pack reaches.
pub fn from_pack(pack_dir: &Path, source: Source) -> Vec<Entry> {
    crate::pack::structures::list(pack_dir)
        .into_iter()
        .map(|s| Entry {
            name: s.name,
            id: s.id,
            source,
            size_bytes: s.size_bytes,
            path: Some(s.path),
        })
        .collect()
}

/// The single list Construct presents in-game, over every source.
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
/// `source` is the only narrowing there is: it separates the database from a
/// pack *and* one pack from the other, which is why a world seeing the same
/// name in its own pack and in the shared copy has an answer to give. Without
/// it such a name has no single answer, and this refuses rather than picking:
/// the two files are different structures that happen to share a name, and
/// guessing which one a `delete` meant is the guess with the worst consequence.
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

/// Every structure matching a name, for the one command that wants them all.
///
/// `resolve` refuses when a name is in two places, because `export -n` and
/// `copy` have to pick one and guessing is the wrong answer. `delete` is the
/// exception: "remove this name from this world" is a complete instruction
/// with no guess in it, so a name in the world's database *and* in its pack
/// yields two entries and both go. `--source` still narrows it for anyone who
/// wants one copy gone and the other kept.
///
/// Zero matches is still an error, with the same near-match suggestions
/// `resolve` offers.
pub fn resolve_all(name: &str, entries: &[Entry], source: Option<Source>) -> Result<Vec<Entry>> {
    let qualified = key::qualify(name);
    let matches: Vec<Entry> = entries
        .iter()
        .filter(|e| e.name == name || e.id == qualified)
        .filter(|e| source.is_none_or(|s| e.source == s))
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

    fn entry(name: &str, source: Source, size_bytes: u64) -> Entry {
        Entry {
            name: name.into(),
            id: format!("mystructure:{name}"),
            source,
            size_bytes,
            path: source
                .is_pack()
                .then(|| std::path::PathBuf::from(format!("/packs/{name}.mcstructure"))),
        }
    }

    fn entries() -> Vec<Entry> {
        vec![
            entry("house", Source::WorldDb, 12),
            entry("barn", Source::WorldDb, 4),
            entry("tower", Source::WorldPack, 31),
        ]
    }

    /// `house` in the world database, in the world's own pack, and in the
    /// shared one — the three-way collision `delete` sweeps up.
    fn house_everywhere() -> Vec<Entry> {
        vec![
            entry("house", Source::WorldDb, 1),
            entry("house", Source::WorldPack, 1),
            entry("house", Source::SharedPack, 1),
        ]
    }

    #[test]
    fn resolve_all_returns_every_copy_of_a_name() {
        // What `resolve` refuses, this returns. `delete` is the only caller:
        // "remove this name from this world" needs no guess, so all three go.
        let got = resolve_all("house", &house_everywhere(), None).unwrap();
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn resolve_all_still_honours_the_source_filter() {
        let entries = house_everywhere();
        for source in [Source::WorldDb, Source::WorldPack, Source::SharedPack] {
            let got = resolve_all("house", &entries, Some(source)).unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].source, source);
        }
    }

    #[test]
    fn resolve_all_still_refuses_a_name_that_is_nowhere() {
        // Zero matches is the one case `resolve` and `resolve_all` agree on.
        let err = resolve_all("nope", &house_everywhere(), None).unwrap_err();
        assert!(matches!(err, CoreError::StructureNotFound { .. }));
    }

    #[test]
    fn resolve_all_finds_a_name_by_its_qualified_form() {
        let got = resolve_all("mystructure:house", &house_everywhere(), None).unwrap();
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
        assert_eq!(got[0].source, Source::WorldDb);
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
    fn a_name_in_two_sources_is_an_error_pointing_at_source() {
        let mut e = entries();
        e.push(entry("house", Source::WorldPack, 9));
        assert!(matches!(
            resolve("house", &e, None),
            Err(CoreError::AmbiguousStructure { .. })
        ));
    }

    #[test]
    fn source_disambiguates_a_name_present_in_both() {
        let mut e = entries();
        e.push(entry("house", Source::WorldPack, 9));
        assert_eq!(
            resolve("house", &e, Some(Source::WorldPack))
                .unwrap()
                .size_bytes,
            9
        );
        assert_eq!(
            resolve("house", &e, Some(Source::WorldDb))
                .unwrap()
                .size_bytes,
            12
        );
    }

    #[test]
    fn a_missing_structure_suggests_near_matches() {
        let CoreError::StructureNotFound { near, .. } = resolve("hous", &entries(), None).unwrap_err()
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
            resolve("barn", &entries(), Some(Source::WorldPack)),
            Err(CoreError::StructureNotFound { .. })
        ));
    }

    #[test]
    fn entries_sort_by_name_then_source() {
        // Three sources can hold the same display name; the order between them
        // is stated rather than inherited from concatenation order.
        let mut sorted = vec![
            entry("a", Source::SharedPack, 1),
            entry("a", Source::WorldPack, 1),
            entry("a", Source::WorldDb, 1),
        ];
        sort(&mut sorted);
        assert_eq!(sorted[0].source, Source::WorldDb);
        assert_eq!(sorted[1].source, Source::WorldPack);
        assert_eq!(sorted[2].source, Source::SharedPack);
    }

    #[test]
    fn read_entry_reads_a_pack_entry_from_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("barn.mcstructure");
        std::fs::write(&path, b"pack-bytes").unwrap();
        let mut e = entry("barn", Source::WorldPack, 10);
        e.path = Some(path);
        assert_eq!(read_entry(&e, None).unwrap(), b"pack-bytes");
    }

    #[test]
    fn read_entry_reads_a_world_entry_from_the_store() {
        let store = MemoryStore::with(&[("barn", b"world-bytes")]);
        let e = entry("barn", Source::WorldDb, 11);
        assert_eq!(read_entry(&e, Some(&store)).unwrap(), b"world-bytes");
    }

    #[test]
    fn read_entry_without_a_store_for_a_world_entry_is_not_found() {
        let e = entry("barn", Source::WorldDb, 11);
        assert!(matches!(
            read_entry(&e, None),
            Err(CoreError::StructureNotFound { .. })
        ));
    }

    #[test]
    fn source_picks_between_the_two_packs_a_world_sees() {
        // The case one merged axis exists for: one name, two packs serving one
        // world. `world-pack` and `shared-pack` separate them where the old
        // `--source world-pack` could not, and needed a second `--pack` flag.
        let e = vec![
            entry("house", Source::SharedPack, 1),
            entry("house", Source::WorldPack, 2),
        ];
        assert!(resolve("house", &e, None).is_err(), "ambiguous");
        assert_eq!(
            resolve("house", &e, Some(Source::SharedPack))
                .unwrap()
                .size_bytes,
            1
        );
        assert_eq!(
            resolve("house", &e, Some(Source::WorldPack))
                .unwrap()
                .size_bytes,
            2
        );
    }

    #[test]
    fn a_pack_source_excludes_world_database_entries() {
        // A pack source is about packs. A world's database structure has no
        // pack to be in, so naming one drops it — otherwise `--source
        // world-pack` would still resolve to a database entry and delete would
        // refuse a world delete it never meant to attempt.
        let e = vec![entry("house", Source::WorldDb, 1)];
        assert!(resolve("house", &e, None).is_ok());
        assert!(matches!(
            resolve("house", &e, Some(Source::WorldPack)),
            Err(CoreError::StructureNotFound { .. })
        ));
    }

    #[test]
    fn an_ambiguous_name_names_the_sources_that_matched() {
        let mut e = entries();
        e.push(entry("house", Source::SharedPack, 9));
        let CoreError::AmbiguousStructure { sources, .. } = resolve("house", &e, None).unwrap_err()
        else {
            panic!("expected AmbiguousStructure");
        };
        assert_eq!(
            sources,
            vec!["world-db".to_string(), "shared-pack".to_string()]
        );
    }
}
