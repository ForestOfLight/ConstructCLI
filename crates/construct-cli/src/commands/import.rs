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

    // With a world, the write belongs in that world's structures home — its
    // own copy of Construct if it has one, else its structures pack, created
    // here if it has none. Without a world there is no per-world home to
    // choose, and the shared Construct is the deliberate answer: "put this in
    // every world that uses it" is a thing to want, and the line below says
    // that is what happened.
    let home = match world {
        Some(w) => crate::commands::home_for_write(w, installation, out)?,
        None => pack::Home {
            dir: pack::for_installation(installation)?.pack.dir,
            kind: pack::HomeKind::SharedConstruct,
        },
    };
    // Construct's in-game list only shows `mystructure:` structures (§17), so a
    // namespaced name lands somewhere the addon will not display.
    if !id.starts_with(&format!("{}:", key::DEFAULT_NAMESPACE)) {
        out.warn(format!(
            "{id} is outside the mystructure namespace; Construct's in-game list will not show it"
        ));
    }

    if let Some(w) = world {
        crate::commands::warn_if_another_pack_has_it(w, installation, &home.dir, &id, out);
    }
    let path = structures::write(&home.dir, &id, &bytes, force)?;

    out.line(format!(
        "imported {} as {}",
        file.display(),
        key::display_name(&id)
    ));
    out.line(format!(
        "  into {}",
        crate::commands::pack_phrase(home.kind, world.map(|w| w.display_name.as_str()))
    ));
    out.line(format!("  {}", path.display()));
    out.line("Reload the world before Construct sees it.");

    out.emit(Payload {
        name: key::display_name(&id).to_string(),
        id: id.clone(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        pack: home.dir.display().to_string(),
        scope: crate::commands::scope_field(home.kind),
    });
    Ok(())
}
