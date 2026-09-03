//! Copying a `.mcstructure` file into Construct's `structures/` folder.
//!
//! No database is involved in either direction: a structure's leveldb value is
//! byte-identical to a `.mcstructure` file, so this is a file copy with a name
//! derived under Construct's rules.

use crate::output::Out;
use construct_core::discovery::{Installation, World};
use construct_core::pack::{self, structures};
use construct_core::store::key;
use construct_core::{CoreError, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Payload {
    name: String,
    id: String,
    path: String,
    bytes: u64,
    pack: String,
    scope: &'static str,
}

pub fn run(
    file: &Path,
    world: Option<&World>,
    installation: &Installation,
    name: Option<&str>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let bytes = std::fs::read(file)?;

    let id = match name {
        // An explicit --name is the user's own choice; it still has to be a
        // name Construct can address, so it goes through the same validation.
        Some(n) => key::qualify(n),
        None => {
            let stem = file.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                CoreError::BadStructureName {
                    name: file.display().to_string(),
                    reason: "the file has no usable stem".to_string(),
                }
            })?;
            key::qualify(&structures::derive_name(stem)?)
        }
    };

    let target = match world {
        Some(w) => pack::for_world(w, installation)?,
        None => pack::for_installation(installation)?,
    };
    if let Some(other) = &target.also_at {
        out.warn(format!(
            "two copies of Construct; writing into {} (the world's own), not {}",
            target.pack.dir.display(),
            other.display()
        ));
    }
    // Construct's in-game list only shows `mystructure:` structures (§17), so a
    // namespaced name lands somewhere the addon will not display.
    if !id.starts_with(&format!("{}:", key::DEFAULT_NAMESPACE)) {
        out.warn(format!(
            "{id} is outside the mystructure namespace; Construct's in-game list will not show it"
        ));
    }

    let path = structures::write(&target.pack.dir, &id, &bytes, force)?;

    out.line(format!(
        "imported {} as {}",
        file.display(),
        key::display_name(&id)
    ));
    out.line(format!("  {}", path.display()));
    out.line("Reload the world before Construct sees it.");

    out.emit(Payload {
        name: key::display_name(&id).to_string(),
        id: id.clone(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        pack: target.pack.dir.display().to_string(),
        scope: match target.scope {
            pack::Scope::WorldLocal => "world",
            pack::Scope::Shared => "shared",
        },
    });
    Ok(())
}
