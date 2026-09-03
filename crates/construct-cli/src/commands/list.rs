use crate::commands::catalog;
use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::Source;
use construct_core::discovery::{Installation, World};
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

pub fn run(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    let loaded = catalog::for_world(world, installations, source, out)?;
    let entries = loaded.entries;

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
