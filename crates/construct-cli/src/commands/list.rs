use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::{Installation, World, installation};
use construct_core::pack;
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

pub fn run(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    // `--source pack` never opens the world: copying gigabytes of db/ to list
    // files that are not in it would be indefensible.
    let world_entries = if source == Some(Source::Pack) {
        Vec::new()
    } else {
        let store = store::open_world_store(world)?;
        if let Some(bytes) = store.via_snapshot {
            out.warn(format!("reading from a {} snapshot", human_size(bytes)));
        }
        catalog::from_world(&store)?
    };

    let pack_entries = if source == Some(Source::World) {
        Vec::new()
    } else {
        match installation::for_world(installations, world).and_then(|i| pack::for_world(world, i))
        {
            Ok(target) => {
                if let Some(other) = &target.also_at {
                    out.warn(format!(
                        "two copies of Construct; using {} (the world's own), not {}",
                        target.pack.dir.display(),
                        other.display()
                    ));
                }
                catalog::from_pack(&target.pack.dir)
            }
            // Asking for pack structures on a machine with no Construct is an
            // error; a plain `list` just says so and shows the world.
            Err(e) if source == Some(Source::Pack) => return Err(e),
            Err(_) => {
                out.warn("Construct is not installed; showing world structures only");
                Vec::new()
            }
        }
    };

    let entries = catalog::unify(world_entries, pack_entries);

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
