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
    /// describe the command rather than any one row. `target` is spelled as a
    /// `--source` value — `world-pack` or `shared-pack` — so a reader can
    /// name the same place back to `structures` or `export`.
    pack: String,
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

/// One file the command will import, and the id it lands under.
struct Planned {
    file: PathBuf,
    id: String,
}

/// Every `.mcstructure` under `dir`, deepest paths included, sorted by path so
/// a directory import reports in a stable order.
///
/// Files that are not `.mcstructure` are skipped rather than refused: a folder
/// of structures routinely carries a README or a `.DS_Store`, and naming them
/// on the command line was never how they got here. `file_type` reports a
/// symlink as a symlink rather than following it, so a directory symlink is
/// never recursed into and the walk cannot be led outside `dir` — the same
/// guarantee `pack::structures::collect` relies on when reading a pack.
fn mcstructures_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk(dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(&path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some(structures::EXTENSION) {
            out.push(path);
        }
    }
    Ok(())
}

/// The id a file inside an imported directory lands under.
///
/// The directory's own name becomes the namespace and its tree becomes the
/// name, so `Amelix/sub/tower.mcstructure` imports as `Amelix:sub/tower` and
/// lands at `structures/Amelix/sub/tower.mcstructure` — the same tree, in the
/// pack. Every segment goes through `derive_name`, so a folder named `My
/// Builds` imports as `My_Builds` and a segment that cannot be a name at all
/// stops the command instead of being mangled into one.
fn id_under_directory(root_name: &str, file: &Path, dir: &Path) -> Result<String> {
    let rel = file.strip_prefix(dir).map_err(|_| CoreError::Internal {
        what: format!("{} is not under {}", file.display(), dir.display()),
    })?;

    let mut segments = vec![structures::derive_name(root_name)?];
    let parents = rel.parent().map(Path::to_path_buf).unwrap_or_default();
    for component in parents.components() {
        let part = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| CoreError::BadStructureName {
                name: file.display().to_string(),
                reason: "a folder name that is not valid UTF-8".to_string(),
            })?;
        segments.push(structures::derive_name(part)?);
    }
    let stem =
        file.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| CoreError::BadStructureName {
                name: file.display().to_string(),
                reason: "the file has no usable stem".to_string(),
            })?;
    segments.push(structures::derive_name(stem)?);

    let (namespace, rest) = segments.split_first().expect("root name is always present");
    Ok(format!("{}:{}", namespace, rest.join("/")))
}

/// Turns the paths named on the command line into the files to import.
///
/// A file imports under its own stem, exactly as it always has. A directory
/// expands to every `.mcstructure` beneath it, keeping its tree. The two mix
/// freely in one invocation.
fn expand(paths: &[PathBuf], name: Option<&str>) -> Result<Vec<Planned>> {
    let mut planned = Vec::new();
    for path in paths {
        if !path.is_dir() {
            planned.push(Planned {
                id: id_for(path, name)?,
                file: path.clone(),
            });
            continue;
        }

        // `file_name` is `None` for `.`, `..` and a root, none of which offer
        // a name to file the tree under. Asking for the folder to be named
        // outright beats guessing one from the current directory.
        let root_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
            CoreError::BadStructureName {
                name: path.display().to_string(),
                reason: "this folder has no name to import under; \
                         name the folder by its own path"
                    .to_string(),
            }
        })?;

        let files = mcstructures_under(path)?;
        if files.is_empty() {
            return Err(CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} holds no .mcstructure files", path.display()),
            )));
        }
        for file in files {
            planned.push(Planned {
                id: id_under_directory(root_name, &file, path)?,
                file,
            });
        }
    }
    Ok(planned)
}

/// Construct's in-game list only shows `mystructure:` structures (§17), so a
/// namespaced name lands somewhere the addon will not display.
///
/// Warned once per namespace rather than once per structure: a folder import
/// puts every one of its files in the same namespace, and forty identical
/// lines bury the forty that actually differ.
fn warn_about_namespaces_outside_the_default(ids: &[String], out: &mut Out) {
    let default = format!("{}:", key::DEFAULT_NAMESPACE);
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if id.starts_with(&default) {
            continue;
        }
        let Some((namespace, _)) = id.split_once(':') else {
            continue;
        };
        if seen.contains(&namespace) {
            continue;
        }
        seen.push(namespace);
        out.warn(format!(
            "{namespace} is outside the mystructure namespace; \
             Construct's in-game list will not show its structures"
        ));
    }
}

pub fn run(
    paths: &[PathBuf],
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
    let mut sources: Vec<(PathBuf, String, Vec<u8>)> = Vec::new();
    for Planned { file, id } in expand(paths, name)? {
        let bytes = std::fs::read(&file)?;
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

    let ids: Vec<String> = plan.iter().map(|(_, id, _, _)| id.clone()).collect();
    warn_about_namespaces_outside_the_default(&ids, out);

    let mut written = Vec::new();
    for (file, id, bytes, _) in plan {
        if let Some(w) = world {
            crate::commands::warn_if_another_pack_has_it(w, installation, &home.dir, &id, out);
        }
        let path = structures::write(&home.dir, &id, &bytes, force)?;

        out.line(format!(
            "imported {} as {}",
            file.display(),
            key::display_name(&id)
        ));
        out.line(format!("  {}", path.display()));

        written.push(Written {
            name: key::display_name(&id).to_string(),
            id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    // Once, after the whole batch: one destination home is chosen per
    // invocation, so where the files landed is an answer about the command
    // rather than about any one structure — and so is the reload advice.
    out.line(format!(
        "into {}",
        crate::commands::pack_phrase(home.kind, world.map(|w| w.display_name.as_str()))
    ));
    out.line("Reload the world before Construct sees it.");
    out.emit(Payload {
        pack: home.dir.display().to_string(),
        target: crate::commands::target_field(home.kind),
        written,
    });
    Ok(())
}
