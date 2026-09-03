//! Copying a structure from one world's catalog into another world's Construct.
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
use construct_core::pack::{self, structures};
use construct_core::store::StructureStore;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    name: String,
    id: String,
    from: String,
    to: String,
    path: String,
    bytes: u64,
}

pub fn run(
    src: &World,
    structure: &str,
    dst: &World,
    installations: &[Installation],
    source: Option<Source>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let loaded = loader::for_world(src, installations, source, out)?;
    let entry = catalog::resolve(structure, &loaded.entries, source)?;
    let bytes = catalog::read_entry(
        &entry,
        loaded.store.as_ref().map(|s| s as &dyn StructureStore),
    )?;

    let dst_installation = installation::for_world(installations, dst)?;
    let target = pack::for_world(dst, dst_installation)?;
    if let Some(other) = &target.also_at {
        out.warn(format!(
            "two copies of Construct on {}; writing into {}, not {}",
            dst.display_name,
            target.pack.dir.display(),
            other.display()
        ));
    }
    let path = structures::write(&target.pack.dir, &entry.id, &bytes, force)?;

    out.line(format!(
        "copied {} from {} to {}",
        entry.name, src.display_name, dst.display_name
    ));
    out.line(format!("  {}", path.display()));
    out.line("Reload the destination world before Construct sees it.");

    out.emit(Payload {
        name: entry.name.clone(),
        id: entry.id.clone(),
        from: src.display_name.clone(),
        to: dst.display_name.clone(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
    });
    Ok(())
}
