//! Removing imported structures from Construct's `structures/` folder.
//!
//! Deleting a pack structure is an ordinary file `unlink` and carries none of
//! the ceremony a world-database delete would need. Deleting from a world's
//! database would be the only leveldb write this tool makes and does not
//! exist yet (stage 4) — refusing that form here, before any work begins, is
//! the entire reason this command ships now instead of waiting for stage 4.

use crate::commands::catalog as loader;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::pack::structures;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    world: String,
    deleted: Vec<Deleted>,
}

#[derive(Serialize)]
struct Deleted {
    name: String,
    id: String,
    path: String,
    /// Which copy of Construct lost the file, in the same vocabulary
    /// `import` and `copy` use for the copy that gained one. Per row rather
    /// than per command: the entries in one batch are found by name across
    /// every pack the world sees, so two of them can come from different
    /// packs.
    scope: Option<&'static str>,
}

/// Stage 4 owns the leveldb write path. Refusing here is deliberate.
fn not_yet_implemented() -> CoreError {
    CoreError::NotImplemented {
        what: "deleting a structure from a world's database".to_string(),
    }
}

pub fn run(
    world: &World,
    names: &[String],
    installations: &[Installation],
    source: Option<Source>,
    pack_scope: Option<construct_core::pack::Scope>,
    out: &mut Out,
) -> Result<()> {
    // Refuse before doing any work at all: this is the one command whose
    // world-source form can damage a save, and it does not exist yet.
    if source == Some(Source::World) {
        return Err(not_yet_implemented());
    }
    // Only the pack half exists, so the lookup is pinned to it regardless of
    // whether the caller passed --source pack explicitly. This also keeps
    // the loader on its --source pack short-circuit, so a world's database
    // is never opened for a delete.
    let loaded = loader::for_world(world, installations, Some(Source::Pack), out)?;

    // Resolve every name before unlinking anything, the way `export` plans
    // every target before writing one. An unknown name halfway down the list
    // must leave the structures named before it still on disk — a delete that
    // half-happened is the one outcome there is no undo for.
    let mut plan = Vec::new();
    for name in names {
        let entry = catalog::resolve(name, &loaded.entries, Some(Source::Pack), pack_scope)?;
        // Resolving with Some(Source::Pack) always yields a pack entry, which
        // always carries a path — but the invariant lives in another module,
        // so this stays a checked refusal rather than an assumption.
        let Some(path) = entry.path.clone() else {
            return Err(not_yet_implemented());
        };
        plan.push((entry, path));
    }

    let mut deleted = Vec::new();
    for (entry, path) in plan {
        structures::remove(&path)?;

        out.line(format!("deleted {}", entry.name));
        // Which pack lost the file matters as much here as it does on the way
        // in: removing a structure from the shared copy of Construct takes it away
        // from every world using that pack, not just the one named on the
        // command line. The pack is found by the path the entry came from
        // rather than by asking for the world's home — a world can see more
        // than one pack, and the file being deleted is not always in the one
        // a write would go to.
        let from = loaded.packs.iter().find(|h| path.starts_with(&h.dir));
        if let Some(home) = from {
            out.line(format!(
                "  from {}",
                crate::commands::pack_phrase(home.kind, Some(world.display_name.as_str()))
            ));
        }
        out.line(format!("  {}", path.display()));

        deleted.push(Deleted {
            name: entry.name,
            id: entry.id,
            path: path.display().to_string(),
            scope: from.map(|h| crate::commands::scope_field(h.kind)),
        });
    }

    // Once, after the whole batch: the advice is about reloading the world,
    // not about any one structure.
    out.line("Reload the world before Construct stops showing it.");
    out.emit(Payload {
        world: world.qualified(),
        deleted,
    });
    Ok(())
}
