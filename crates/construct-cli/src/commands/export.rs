//! Writing structures out as `.mcstructure` files.
//!
//! A structure's leveldb value is byte-identical to a `.mcstructure` file, so
//! this command copies bytes and parses nothing.

use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::World;
use construct_core::error::CoreError;
use construct_core::store::{self, StructureStore};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    world: String,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    path: String,
    bytes: u64,
}

pub fn run(
    world: &World,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let store = store::open_world_store(world)?;
    if let Some(bytes) = store.via_snapshot {
        out.warn(format!("reading from a {} snapshot", human_size(bytes)));
    }
    let entries = catalog::from_world(&store)?;

    // Resolve every name and target path before writing anything, so a
    // collision refuses the whole command rather than leaving half a job done.
    let mut plan: Vec<(catalog::Entry, PathBuf)> = Vec::new();
    for name in structures {
        let entry = catalog::resolve(name, &entries, source)?;
        let target = match output {
            Some(path) => path.to_path_buf(),
            None => PathBuf::from(format!("{}.mcstructure", entry.name)),
        };
        plan.push((entry, target));
    }

    for (_, target) in &plan {
        if target.exists() && !force {
            return Err(CoreError::TargetExists {
                path: target.clone(),
            });
        }
    }

    let mut written = Vec::new();
    for (entry, target) in plan {
        let bytes = store
            .get(&entry.id)?
            .ok_or_else(|| CoreError::StructureNotFound {
                name: entry.name.clone(),
                near: vec![],
            })?;
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        out.line(format!(
            "wrote {} ({})",
            target.display(),
            human_size(bytes.len() as u64)
        ));
        written.push(Written {
            name: entry.name,
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    out.emit(Payload {
        world: world.qualified(),
        written,
    });
    Ok(())
}
