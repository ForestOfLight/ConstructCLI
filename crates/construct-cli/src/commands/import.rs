//! Copying `.mcstructure` files into Construct's `structures/` folder.
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
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    /// The pack every file in this batch landed in, and what that means for
    /// reach. One destination home is chosen per invocation, so these
    /// describe the command rather than any one row.
    pack: String,
    scope: &'static str,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    id: String,
    path: String,
    bytes: u64,
}

/// The structure id a file imports under: `--name` when given, else derived
/// from the file stem under Construct's rules.
fn id_for(file: &Path, name: Option<&str>) -> Result<String> {
    match name {
        // An explicit --name is the user's own choice; it still has to be a
        // name Construct can address, so it goes through the same validation.
        Some(n) => Ok(key::qualify(n)),
        None => {
            let stem = file.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                CoreError::BadStructureName {
                    name: file.display().to_string(),
                    reason: "the file has no usable stem".to_string(),
                }
            })?;
            Ok(key::qualify(&structures::derive_name(stem)?))
        }
    }
}

/// Construct's in-game list only shows `mystructure:` structures (§17), so a
/// namespaced name lands somewhere the addon will not display.
fn warn_if_outside_default_namespace(id: &str, out: &mut Out) {
    if !id.starts_with(&format!("{}:", key::DEFAULT_NAMESPACE)) {
        out.warn(format!(
            "{id} is outside the mystructure namespace; Construct's in-game list will not show it"
        ));
    }
}

pub fn run(
    files: &[PathBuf],
    world: Option<&World>,
    installation: &Installation,
    name: Option<&str>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    // Read every file and settle every id before writing anything, the way
    // `export` plans every target first. `home_for_write` below can *create*
    // a structures pack, so a batch that cannot be read must fail before it
    // has that side effect.
    let mut sources: Vec<(&PathBuf, String, Vec<u8>)> = Vec::new();
    for file in files {
        let bytes = std::fs::read(file)?;
        let id = id_for(file, name)?;
        sources.push((file, id, bytes));
    }

    // Two files deriving one id would silently collapse into a single
    // structure — the second write landing on the first, or failing halfway
    // through the batch once the first has already landed. Neither is an
    // outcome to discover afterwards, so it is refused up front. `--name`
    // cannot reach here: `main.rs` refuses it for more than one file.
    for i in 1..sources.len() {
        if let Some((earlier, _, _)) = sources[..i].iter().find(|(_, id, _)| *id == sources[i].1) {
            return Err(CoreError::BadStructureName {
                name: sources[i].1.clone(),
                reason: format!(
                    "two files would import under this name: {} and {}",
                    earlier.display(),
                    sources[i].0.display()
                ),
            });
        }
    }

    // With a world, the write belongs in that world's structures home — its
    // own copy of Construct if it has one, else its structures pack, created
    // here if it has none. Without a world there is no per-world home to
    // choose, and the shared copy of Construct is the deliberate answer: "put this in
    // every world that uses it" is a thing to want, and the line below says
    // that is what happened.
    let home = match world {
        Some(w) => crate::commands::home_for_write(w, installation, out)?,
        None => pack::Home {
            dir: pack::for_installation(installation)?.pack.dir,
            kind: pack::HomeKind::SharedConstruct,
        },
    };

    // Settle and check every destination before the first write, so one
    // collision stops the command rather than leaving half the batch
    // imported.
    let mut plan = Vec::new();
    for (file, id, bytes) in sources {
        let target = structures::path_for(&home.dir, &id)?;
        plan.push((file, id, bytes, target));
    }
    if !force {
        for (_, _, _, target) in &plan {
            if target.exists() {
                return Err(CoreError::TargetExists {
                    path: target.clone(),
                });
            }
        }
    }

    let mut written = Vec::new();
    for (file, id, bytes, _) in plan {
        warn_if_outside_default_namespace(&id, out);
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

        written.push(Written {
            name: key::display_name(&id).to_string(),
            id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    // Once, after the whole batch: the advice is about reloading the world,
    // not about any one structure.
    out.line("Reload the world before Construct sees it.");
    out.emit(Payload {
        pack: home.dir.display().to_string(),
        scope: crate::commands::scope_field(home.kind),
        written,
    });
    Ok(())
}
