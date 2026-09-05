//! Building a world's structure catalog, for every command that resolves a name.

use crate::commands::worlds::human_size;
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
    /// Every pack whose structures this world sees, in the order
    /// `pack::serving` returns them. Commands that write need the first one
    /// (the world's home); `delete` needs to know which pack a file it is
    /// about to unlink came from.
    pub packs: Vec<pack::Home>,
}

/// Builds the unified catalog `list`, `copy`, and `export` all resolve names
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
        if let Some(bytes) = store.via_snapshot {
            out.warn(format!("reading from a {} snapshot", human_size(bytes)));
        }
        let entries = catalog::from_world(&store)?;
        (entries, Some(store))
    };

    let mut packs = Vec::new();
    let pack_entries = if source == Some(Source::World) {
        Vec::new()
    } else {
        // Every pack serving this world, not just one: a world running the
        // shared Construct sees that pack's structures *and* its own
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
            // error; a plain `list` just says so and shows the world.
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

    let entries = catalog::unify(world_entries, pack_entries);
    Ok(Loaded {
        entries,
        store,
        packs,
    })
}
