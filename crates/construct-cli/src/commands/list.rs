use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::World;
use construct_core::store;
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    world: String,
    structures: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    name: &'a str,
    id: &'a str,
    source: &'static str,
    size_bytes: u64,
}

pub fn run(world: &World, source: Option<Source>, out: &mut Out) -> Result<()> {
    let store = store::open_world_store(world)?;
    if let Some(bytes) = store.via_snapshot {
        out.warn(format!("reading from a {} snapshot", human_size(bytes)));
    }

    let mut entries = catalog::from_world(&store)?;
    // Stage 2 adds pack entries here. Until then, filtering to `pack` is
    // legitimately empty rather than an error.
    if let Some(s) = source {
        entries.retain(|e| e.source == s);
    }

    if !out.is_json() {
        if entries.is_empty() {
            out.line("no structures");
        } else {
            out.line(format!("{:<24} {:<8} {:>9}", "NAME", "SOURCE", "SIZE"));
            for e in &entries {
                // Deliberately NOT truncated, unlike worlds.rs: there the
                // truncated column is a display name with a separate,
                // untruncated REFERENCE column carrying the copy-pasteable
                // identifier. Here the NAME column *is* the identifier the
                // user types into `export` — truncating it would hand back a
                // name that doesn't exist. A ragged column is cosmetic; a
                // dead-end copy-paste is functional, so full name wins.
                out.line(format!(
                    "{:<24} {:<8} {:>9}",
                    e.name,
                    e.source.as_str(),
                    human_size(e.size_bytes)
                ));
            }
        }
    }

    out.emit(Payload {
        world: world.qualified(),
        structures: entries
            .iter()
            .map(|e| Row {
                name: &e.name,
                id: &e.id,
                source: e.source.as_str(),
                size_bytes: e.size_bytes,
            })
            .collect(),
    });
    Ok(())
}
