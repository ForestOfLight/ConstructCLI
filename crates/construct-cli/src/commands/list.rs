use crate::commands::catalog;
use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::pack;
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    /// The world listed, or null when the shared pack was listed on its own.
    world: Option<String>,
    structures: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    name: &'a str,
    id: &'a str,
    source: &'static str,
    /// `world` or `shared` for a pack row — which worlds this structure
    /// reaches. Null for a world-database row, which reaches one world by
    /// construction.
    scope: Option<&'static str>,
    size_bytes: u64,
}

pub fn run(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    pack_scope: Option<pack::Scope>,
    out: &mut Out,
) -> Result<()> {
    let loaded = catalog::for_world(world, installations, source, out)?;
    // `--pack` is about packs, so naming one drops world-database rows along
    // with the other pack's — the same rule `catalog::resolve` applies.
    let entries: Vec<Entry> = match pack_scope {
        None => loaded.entries,
        Some(s) => loaded
            .entries
            .into_iter()
            .filter(|e| e.scope == Some(s))
            .collect(),
    };
    render(Some(world.qualified()), &entries, out);
    Ok(())
}

/// The shared pack on its own, with no world named.
///
/// Deliberately not a world's view: this is the one listing that is about the
/// installation rather than about a world, and it answers "what does every
/// world using this pack get?". A world's own structures are not in it, and
/// neither is any world's database — nothing here opens one.
pub fn shared(installation: &Installation, out: &mut Out) -> Result<()> {
    let pack = pack::for_installation(installation)?.pack;
    let entries = construct_core::catalog::from_pack(&pack.dir, pack::Scope::Shared);
    out.line(format!("{}", pack.dir.display()));
    render(None, &entries, out);
    Ok(())
}

fn render(world: Option<String>, entries: &[Entry], out: &mut Out) {
    if !out.is_json() {
        if entries.is_empty() {
            out.line("no structures");
        } else {
            out.line(format!("{:<24} {:<12} {:>9}", "NAME", "SOURCE", "SIZE"));
            for e in entries {
                // Deliberately NOT truncated, unlike worlds.rs: there the
                // truncated column is a display name with a separate,
                // untruncated REFERENCE column carrying the copy-pasteable
                // identifier. Here the NAME column *is* the identifier the
                // user types into `export` — truncating it would hand back a
                // name that doesn't exist. A ragged column is cosmetic; a
                // dead-end copy-paste is functional, so full name wins.
                //
                // `pack` alone stopped being an answer once a world could see
                // two packs at once: `pack:world` belongs to this world,
                // `pack:shared` is in every world using the shared install.
                out.line(format!(
                    "{:<24} {:<12} {:>9}",
                    e.name,
                    e.source_label(),
                    human_size(e.size_bytes)
                ));
            }
        }
    }

    out.emit(Payload {
        world,
        structures: entries
            .iter()
            .map(|e| Row {
                name: &e.name,
                id: &e.id,
                source: e.source.as_str(),
                scope: e.scope.map(|s| match s {
                    pack::Scope::WorldLocal => "world",
                    pack::Scope::Shared => "shared",
                }),
                size_bytes: e.size_bytes,
            })
            .collect(),
    });
}
