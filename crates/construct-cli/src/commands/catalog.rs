//! Building a world's structure catalog, for every command that resolves a name.

use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World, installation};
use construct_core::error::CoreError;
use construct_core::pack;
use construct_core::store::{self, OpenedStore};

pub struct Loaded {
    pub entries: Vec<Entry>,
    /// Held open so `catalog::read_entry` can fetch world-source bytes.
    pub store: Option<OpenedStore>,
    /// The packs the pack half came from, for a caller that has to tell the
    /// world's own copy from the shared one. See [`world_scoped`].
    pub packs: Vec<pack::Home>,
}

/// Builds the unified catalog `structures`, `copy`, and `export` all resolve names
/// against.
///
/// The `--source pack` short-circuit (never open the world's database) is a
/// correctness property, not an optimisation: opening a leveldb runs recovery
/// and rewrites it, which is why reads copy `db/` first.
pub fn for_world(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<Loaded> {
    let (world_entries, store) = if source == Some(Source::Pack) {
        (Vec::new(), None)
    } else {
        let store = store::open_world_store(world)?;
        let entries = catalog::from_world(&store)?;
        (entries, Some(store))
    };

    let (pack_entries, packs) = packs_for_world(world, installations, source, out)?;

    let entries = catalog::unify(world_entries, pack_entries);
    Ok(Loaded {
        entries,
        store,
        packs,
    })
}

/// Drops the shared copy's rows from a world-scoped catalog.
///
/// `--world` means *this world*, and the shared copy of Construct serves worlds
/// the command never named. `export` and `delete` both narrow to it, and both
/// do the dropping here rather than at resolve time: that way a name living
/// only in the shared copy reports "not found in this world" — true, and it
/// points at the right fix — instead of being quietly answered, or unlinked,
/// from under every other world using that copy.
///
/// `pack::serving` reports the shared copy for a world that has no copy of its
/// own, so the rows are filtered out rather than assumed absent. World-database
/// rows have no path and always survive.
pub fn world_scoped(entries: Vec<Entry>, packs: &[pack::Home]) -> Vec<Entry> {
    let shared_dirs: Vec<_> = packs
        .iter()
        .filter(|h| h.kind.scope() == pack::Scope::Shared)
        .map(|h| h.dir.clone())
        .collect();
    entries
        .into_iter()
        .filter(|e| {
            e.path
                .as_ref()
                .is_none_or(|p| !shared_dirs.iter().any(|d| p.starts_with(d)))
        })
        .collect()
}

/// Points a miss at the scope that was actually searched.
///
/// Without this, `delete house --world W` — or `export house --world W` — for a
/// structure that lives only in the shared copy reports a bare "not found",
/// which is the one answer guaranteed to send the user looking in the wrong
/// place. `None` is the shared copy, whose bare message is already unambiguous.
pub fn explain_miss(err: CoreError, world: Option<&World>) -> CoreError {
    match (err, world) {
        (CoreError::StructureNotFound { name, near }, Some(w)) => CoreError::StructureNotFound {
            name: format!("{name} in {}", w.display_name),
            near,
        },
        (other, _) => other,
    }
}

/// The pack half of the catalog, and the packs it came from.
///
/// Split out of [`for_world`] because `delete` builds its world half from a
/// live database rather than a snapshot (it is about to write to it), so it
/// cannot go through `for_world` — but the "which packs serve this world"
/// logic, and the difference between an error and a warning when Construct is
/// missing, must not be written twice.
pub fn packs_for_world(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<(Vec<Entry>, Vec<pack::Home>)> {
    let mut packs = Vec::new();
    let pack_entries = if source == Some(Source::World) {
        Vec::new()
    } else {
        // Every pack serving this world, not just one: a world running the
        // shared copy of Construct sees that pack's structures *and* its own
        // structures pack, and reporting one of the two would misstate what
        // the world has.
        match installation::for_world(installations, world).and_then(|i| {
            let serving = pack::serving(world, i);
            if serving.is_empty() {
                Err(CoreError::ConstructNotInstalled {
                    searched: pack::searched_roots(world, i),
                })
            } else {
                Ok(serving)
            }
        }) {
            Ok(serving) => {
                let mut entries = Vec::new();
                for home in &serving {
                    entries.extend(catalog::from_pack(&home.dir, home.kind.scope()));
                }
                packs = serving;
                entries
            }
            // Asking for pack structures on a machine with no Construct is an
            // error; a plain `structures` just says so and shows the world.
            Err(e) if source == Some(Source::Pack) => return Err(e),
            Err(e) => {
                // Distinguish the reason: `InstallationNotFound` (a
                // path-referenced world with no installations to search) is
                // not the same problem as `ConstructNotInstalled`, and a
                // catch-all message here would misreport the former.
                out.warn(format!("{e}; showing world structures only"));
                Vec::new()
            }
        }
    };

    Ok((pack_entries, packs))
}
