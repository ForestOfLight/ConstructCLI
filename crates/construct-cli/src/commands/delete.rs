//! Removing structures, from a world or from the shared copy of Construct.
//!
//! `delete` is `import` read backwards, and its grammar says so: `import
//! <file>` places into the shared copy of Construct and `import <file> --world
//! W` places into that world's own, so `delete <name>` removes from the shared
//! copy and `delete <name> --world W` removes from that world. Absence means
//! shared here exactly as it does for `import` and for `structures`.
//!
//! **`--world` never touches the shared copy.** That is the point of it: a
//! structure in the shared Construct belongs to every world using that copy, so
//! removing one is not something a command scoped to a single world should do
//! as a side effect. The two scopes are reached by two different invocations
//! and neither can reach the other's files.
//!
//! Within a world, though, a name can still mean two things at once: a key in
//! the world's database and a file in the world's own pack. Every other command
//! refuses such a name and asks for `--source`, because `export -n` and `copy`
//! have to pick one and guessing is the wrong answer. `delete` removes both,
//! because "remove this name from this world" is a complete instruction with no
//! guess in it. `--source world-db` or `--source world-pack` narrows it for
//! anyone who wants one gone and the other kept; `--source shared-pack` is
//! refused under `--world`, since that is the copy `--world` exists to spare.
//!
//! `--world` is also the only form that opens a world's own database, which is
//! the only leveldb write this tool makes. Opening a leveldb runs recovery and
//! rewrites its file set, so every other command works from a copy (spec §8) —
//! but a write has to touch the original, and a copy would be thrown away
//! unwritten. What guards it is the in-use refusal: writing underneath a world
//! Minecraft has open is how saves get damaged, and there is no `--force`.

use crate::commands::catalog as loader;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::inuse;
use construct_core::pack::{self, structures};
use construct_core::store::bedrock::BedrockStore;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    /// The world the deletion was aimed at, or `None` for the shared copy.
    /// Which copy of Construct was reachable follows from it: `null` is the
    /// shared copy and nothing else, a world is that world and never the
    /// shared copy. Each row then says which of the places it came out of.
    world: Option<String>,
    deleted: Vec<Deleted>,
}

#[derive(Serialize)]
struct Deleted {
    name: String,
    id: String,
    /// Which of the three places this copy lived: `world-db` for a database
    /// key, `world-pack` or `shared-pack` for a file. One name can produce a
    /// row of each under `--world`, so this is the field telling two rows
    /// sharing a `name` apart. Spelled as the `--source` value that selects
    /// it, so a reader can narrow the next run to one of them.
    source: &'static str,
    /// Where the file was, for a pack row. `None` for a database row, which has
    /// no path.
    path: Option<String>,
}

/// `delete <names…>` — the shared copy of Construct, and nothing else.
///
/// No database is opened and no world is consulted. A structure here serves
/// every world using the shared copy, which is why the removal says so.
pub fn shared(
    installation: &Installation,
    names: &[String],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    let pack = pack::for_installation(installation)?.pack;
    let entries = catalog::from_pack(&pack.dir, Source::SharedPack);

    let plan = resolve(names, &entries, source, None)?;
    let mut deleted = Vec::new();
    for entry in &plan {
        let path = unlink(entry)?;
        out.line(format!("deleted {}", entry.name));
        // Named even though this form has only one possible target: it is the
        // same sentence `import` prints on the way in, and the reach of a
        // shared-copy removal is the thing most worth stating plainly.
        out.line(format!(
            "  from {}",
            crate::commands::pack_phrase(pack::HomeKind::SharedConstruct, None)
        ));
        out.line(format!("  {}", path.display()));
        deleted.push(row(entry, Some(path.display().to_string())));
    }

    out.warn(
        "removed from the shared copy of Construct; every world using it loses these \
         structures",
    );
    out.line("Reload affected worlds before Construct stops showing them.");
    out.emit(Payload {
        world: None,
        deleted,
    });
    Ok(())
}

/// `delete <names…> --world W` — that world's database and its own pack.
///
/// The shared copy is out of scope by construction: `pack::serving` reports it
/// for a world that has no copy of its own, so it is filtered out here rather
/// than relied upon to be absent.
pub fn for_world(
    world: &World,
    names: &[String],
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    // A pack `--source` is an ordinary unlink and must not open a database at
    // all, not even to look. Keeping this branch first is what preserves that.
    let live = if source.is_some_and(|s| s.is_pack()) {
        None
    } else {
        // Refuse before opening, because the open is itself a write. A world
        // the game has loaded is one where leveldb has another writer, which is
        // how databases get corrupted rather than merely stale.
        crate::commands::refuse_if_in_use(world, inuse::AtRisk::Database, out)?;
        Some(BedrockStore::open_live(world)?)
    };

    let world_entries = match &live {
        Some(store) => catalog::from_world(store)?,
        None => Vec::new(),
    };
    let (all_pack_entries, packs) = loader::packs_for_world(world, installations, source, out)?;

    // The shared copy is not this command's business, and `export --world`
    // draws the same line for the same reason — so the dropping lives in
    // `loader::world_scoped` rather than here.
    let pack_entries = loader::world_scoped(all_pack_entries);

    let entries = catalog::unify(world_entries, pack_entries);
    let plan = resolve(names, &entries, source, Some(world))?;

    // The database first, while nothing else has changed. A failure here means
    // no file has been unlinked yet, so the command failed cleanly instead of
    // half-way across two kinds of storage; the reverse order could not offer
    // that, because nothing brings an unlinked file back.
    let mut deleted = Vec::new();
    for entry in plan.iter().filter(|e| e.source == Source::WorldDb) {
        let store = live
            .as_ref()
            .ok_or_else(|| internal(format!("world entry {} with no open database", entry.id)))?;
        // The entry came from `ids()` on this same handle a moment ago, so a
        // `false` here would mean the catalog and the database disagree.
        if !store.remove(&entry.id)? {
            return Err(internal(format!(
                "{} was listed in the database but not there to remove",
                entry.id
            )));
        }
        out.line(format!("deleted {}", entry.name));
        out.line("  from this world's database");
        deleted.push(row(entry, None));
    }

    // Drop the handle before touching anything else, so leveldb has flushed and
    // released the world by the time the command reports success.
    let wrote_to_db = !deleted.is_empty();
    drop(live);

    // Remember the write, now that the handle is closed and the mtimes are
    // final. Without this the *next* db-touching command inside the in-use
    // window cannot tell this write from a running game, and pays a full watch
    // to conclude what we already know.
    if wrote_to_db {
        construct_core::writemark::record(world);
    }

    for entry in plan.iter().filter(|e| e.source.is_pack()) {
        let path = unlink(entry)?;
        out.line(format!("deleted {}", entry.name));
        if let Some(home) = packs.iter().find(|h| path.starts_with(&h.dir)) {
            out.line(format!(
                "  from {}",
                crate::commands::pack_phrase(home.kind, Some(world.display_name.as_str()))
            ));
        }
        out.line(format!("  {}", path.display()));
        deleted.push(row(entry, Some(path.display().to_string())));
    }

    out.line("Reload the world before Construct stops showing it.");
    out.emit(Payload {
        world: Some(world.qualified()),
        deleted,
    });
    Ok(())
}

/// Resolves every name before anything is removed.
///
/// An unknown name halfway down the list must leave the structures named before
/// it intact — a delete that half-happened is the one outcome there is no undo
/// for. `resolve_all` rather than `resolve`: within one world a name can still
/// be in both the database and a pack, and both go.
fn resolve(
    names: &[String],
    entries: &[Entry],
    source: Option<Source>,
    world: Option<&World>,
) -> Result<Vec<Entry>> {
    let mut plan = Vec::new();
    for name in names {
        plan.extend(
            catalog::resolve_all(name, entries, source)
                .map_err(|e| loader::explain_miss(e, world))?,
        );
    }
    Ok(plan)
}

fn unlink(entry: &Entry) -> Result<std::path::PathBuf> {
    // `catalog::from_pack` always records a path, so this cannot fire — but the
    // invariant lives in another module, and `construct-core` must not panic, so
    // it stays a checked refusal rather than an assumption.
    let path = entry
        .path
        .clone()
        .ok_or_else(|| internal(format!("pack entry {} with no path", entry.id)))?;
    structures::remove(&path)?;
    Ok(path)
}

fn row(entry: &Entry, path: Option<String>) -> Deleted {
    Deleted {
        name: entry.name.clone(),
        id: entry.id.clone(),
        source: entry.source.as_str(),
        path,
    }
}

fn internal(what: String) -> CoreError {
    CoreError::Internal { what }
}
