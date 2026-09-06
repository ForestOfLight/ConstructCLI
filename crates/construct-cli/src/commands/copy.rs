//! Copying structures from one world's catalog into another world's Construct.
//!
//! The source is read from the source world's unified catalog — a structure
//! already sitting in the source world's Construct copy is as copyable as one
//! in its database — and always written into the destination's Construct
//! `structures/`, never into a leveldb. Source and destination resolve
//! independently, so cross-root copies work (§6).

use crate::commands::catalog as loader;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::{Installation, World, installation};
use construct_core::error::CoreError;
use construct_core::pack::structures;
use construct_core::store::StructureStore;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    // Qualified, not display_name: §6 supports cross-root copies, and two
    // worlds named identically under different installations are
    // indistinguishable by display_name alone — the exact ambiguity
    // `qualified()` exists to remove. Matches `list.rs`'s and `worlds.rs`'s
    // convention of identifying a world in JSON by its qualified reference.
    // The human-readable lines keep display_name; a terminal reader wants the
    // friendly name.
    from: String,
    to: String,
    /// Which pack took the writes: the destination world's own, or the shared
    /// copy of Construct. Spelled as a `--source` value — `world-pack` or
    /// `shared-pack` — so a reader can name the same place back to
    /// `structures` or `export`. `import` reports it the same way; `copy`
    /// writes into exactly the same two places. One destination home is
    /// chosen per invocation, so this describes the command rather than any
    /// one row.
    target: &'static str,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    id: String,
    path: String,
    bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    src: &World,
    dst: &World,
    names: &[String],
    installations: &[Installation],
    source: Option<Source>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let loaded = loader::for_world(src, installations, source, out)?;
    let store = loaded.store.as_ref().map(|s| s as &dyn StructureStore);

    // Resolve and read the whole batch before touching the destination at
    // all. Reading first is what keeps a bad name from having side effects:
    // `home_for_write` below can *create* a structures pack, and an unknown
    // structure must not leave one behind.
    let mut sources = Vec::new();
    for name in names {
        let entry = catalog::resolve(name, &loaded.entries, source)?;
        let bytes = catalog::read_entry(&entry, store)?;
        sources.push((entry, bytes));
    }

    let dst_installation = installation::for_world(installations, dst)?;
    let home = crate::commands::home_for_write(dst, dst_installation, out)?;

    // Settle every destination path and check them all before the first
    // write, the way `export` plans its targets — one collision stops the
    // command rather than leaving half the batch copied.
    let mut plan = Vec::new();
    for (entry, bytes) in sources {
        let target = structures::path_for(&home.dir, &entry.id)?;
        plan.push((entry, bytes, target));
    }
    if !force {
        for (_, _, target) in &plan {
            if target.exists() {
                return Err(CoreError::TargetExists {
                    path: target.clone(),
                });
            }
        }
    }

    let mut written = Vec::new();
    for (entry, bytes, _) in plan {
        crate::commands::warn_if_another_pack_has_it(
            dst,
            dst_installation,
            &home.dir,
            &entry.id,
            out,
        );
        let path = structures::write(&home.dir, &entry.id, &bytes, force)?;

        out.line(format!(
            "copied {} from {} to {}",
            entry.name, src.display_name, dst.display_name
        ));
        out.line(format!(
            "  into {}",
            crate::commands::pack_phrase(home.kind, Some(dst.display_name.as_str()))
        ));
        out.line(format!("  {}", path.display()));

        written.push(Written {
            name: entry.name,
            id: entry.id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    // Once, after the whole batch: the advice is about reloading the world,
    // not about any one structure.
    out.line("Reload the destination world before Construct sees it.");
    out.emit(Payload {
        from: src.qualified(),
        to: dst.qualified(),
        target: crate::commands::target_field(home.kind),
        written,
    });
    Ok(())
}
