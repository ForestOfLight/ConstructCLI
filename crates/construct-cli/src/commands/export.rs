//! Writing structures out as `.mcstructure` files.
//!
//! A structure's leveldb value is byte-identical to a `.mcstructure` file, so
//! this command copies bytes and parses nothing.

use crate::commands::catalog as loader;
use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::mcstructure;
use construct_core::merge::{self, MergeOptions, OnOverlap};
use construct_core::pack;
use construct_core::store::{OpenedStore, StructureStore};
use serde::Serialize;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    /// The world read from, or `None` for the shared copy of Construct. Which
    /// places were reachable follows from it: `null` is the shared copy and
    /// nothing else, a world is that world's database and its own pack and
    /// never the shared copy.
    world: Option<String>,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    path: String,
    bytes: u64,
}

/// A derived filename must be a single path component: no `/` or `\`
/// anywhere in it, and no `..`/root/prefix component. This is a security
/// boundary (structure names come from a world file the user may not have
/// authored), so a violation is refused, never guessed around. An explicit
/// `-n` path is the user's own choice and is never subject to this check.
fn refuse_traversal(name: &str) -> Result<()> {
    let has_separator = name.contains('/') || name.contains('\\');
    let has_dangerous_component = Path::new(name).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if has_separator || has_dangerous_component {
        return Err(CoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "structure {name:?} cannot be used as a filename: it would write \
                 outside the output directory. Pass -n to choose the destination \
                 explicitly."
            ),
        )));
    }
    Ok(())
}

/// A structure name that decodes to the empty string (e.g. from a key like
/// `structuretemplate_mystructure:`) would derive the filename
/// `.mcstructure` — a hidden file with no name. Refused, in the same style
/// as a traversal attempt; an explicit `-n` path chooses the destination
/// directly and is not subject to this check.
fn refuse_empty_derived_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "structure has an empty name and cannot be used as a filename. \
             Pass -n to choose the destination explicitly."
                .to_string(),
        )));
    }
    Ok(())
}

/// Windows reserved device names: writing to one of these addresses a
/// device, not a file, regardless of extension (`CON.mcstructure` is just as
/// reserved as `CON`). Matched on the portion of the sanitized name before
/// the first `.`, case-insensitively.
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Replaces characters that are illegal in a filename on Windows with `_`,
/// and prefixes a Windows reserved device name (`CON`, `PRN`, `COM1`, …) with
/// `_` so it addresses a file rather than a device. This is sanitization,
/// not refusal: unlike a traversal attempt, a name like `understudy:players`
/// or `CON` is not trying to escape the output directory, it just can't be
/// written verbatim (or at all, for a device name) on every platform this
/// project targets. The caller always prints the resulting path, so the
/// substitution is visible to the user.
fn sanitize_for_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            ':' | '<' | '>' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let stem = sanitized.split('.').next().unwrap_or("");
    if RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

/// One invocation's catalog, and the world it belongs to.
///
/// The two entry points differ only in how they build this: [`shared`] reads
/// one pack and opens no database at all, [`for_world`] reads a world and the
/// pack it owns. Everything after — resolve, plan, write, merge — is the same
/// work on the same rows, so it is written once below.
struct View<'a> {
    /// The world read from, or `None` for the shared copy of Construct.
    world: Option<&'a World>,
    entries: Vec<Entry>,
    /// Held open for world-database rows. `None` when the view has none.
    store: Option<OpenedStore>,
}

/// `export <names…>` — the shared copy of Construct, and nothing else.
///
/// No world is resolved and no database is opened. A structure here serves
/// every world using the shared copy, so it is the one answer that does not
/// depend on which world is asking.
#[allow(clippy::too_many_arguments)]
pub fn shared(
    installation: &Installation,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    merge: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    let home = pack::for_installation(installation)?.pack;
    let entries = catalog::from_pack(&home.dir, Source::SharedPack);
    let view = View {
        world: None,
        entries,
        store: None,
    };
    run(
        &view, structures, output, source, force, merge, on_overlap, out,
    )
}

/// `export <names…> --world W` — that world's database and its own pack.
///
/// The shared copy is out of reach by construction: `--world` and `--source
/// shared-pack` contradict each other and `main.rs` refuses the pair, so the
/// only pack left in view is the world's own. `loader::world_scoped` does the
/// dropping, because `pack::serving` reports the shared copy for a world that
/// has no copy of its own.
#[allow(clippy::too_many_arguments)]
pub fn for_world(
    world: &World,
    installations: &[Installation],
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    merge: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    let loaded = loader::for_world(world, installations, source, out)?;
    let view = View {
        world: Some(world),
        entries: loader::world_scoped(loaded.entries),
        store: loaded.store,
    };
    run(
        &view, structures, output, source, force, merge, on_overlap, out,
    )
}

#[allow(clippy::too_many_arguments)]
fn run(
    view: &View,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    merge: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    if merge {
        return run_merge(view, structures, output, source, force, on_overlap, out);
    }

    // Resolve every name and target path before writing anything, so a
    // collision — or a refused name — stops the whole command rather than
    // leaving half a job done.
    let mut plan: Vec<(catalog::Entry, PathBuf)> = Vec::new();
    for name in structures {
        let entry = resolve(name, view, source)?;
        let target = match output {
            Some(path) => path.to_path_buf(),
            None => {
                refuse_empty_derived_name(&entry.name)?;
                refuse_traversal(&entry.name)?;
                PathBuf::from(format!(
                    "{}.mcstructure",
                    sanitize_for_filename(&entry.name)
                ))
            }
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

    let store = view.store.as_ref().map(|s| s as &dyn StructureStore);
    let mut written = Vec::new();
    for (entry, target) in plan {
        let bytes = catalog::read_entry(&entry, store)?;
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
        world: view.world.map(World::qualified),
        written,
    });
    Ok(())
}

/// `catalog::resolve` with the world named on a miss.
fn resolve(name: &str, view: &View, source: Option<Source>) -> Result<catalog::Entry> {
    catalog::resolve(name, &view.entries, source).map_err(|e| loader::explain_miss(e, view.world))
}

#[derive(Serialize)]
struct MergedPayload {
    /// The world read from, or `None` for the shared copy of Construct.
    world: Option<String>,
    merged: Merged,
}

#[derive(Serialize)]
struct Merged {
    path: String,
    bytes: u64,
    sources: Vec<String>,
    size: [i32; 3],
    origin: [i32; 3],
    overlaps: Vec<OverlapRow>,
}

#[derive(Serialize)]
struct OverlapRow {
    count: u64,
    pieces: Vec<String>,
}

/// `--merge`: decode every named structure, combine them, and write one file.
///
/// Unlike the plain path this one must decode — merge is the only command that
/// looks inside a `.mcstructure` at all. Everything else copies bytes.
#[allow(clippy::too_many_arguments)]
fn run_merge(
    view: &View,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    let target = output.expect("main.rs refuses --merge without -n");
    if target.exists() && !force {
        return Err(CoreError::TargetExists {
            path: target.to_path_buf(),
        });
    }

    let store = view.store.as_ref().map(|s| s as &dyn StructureStore);
    let mut pieces = Vec::new();
    for name in structures {
        let entry = resolve(name, view, source)?;
        let bytes = catalog::read_entry(&entry, store)?;
        let decoded = mcstructure::decode(&bytes, &entry.name)?;
        pieces.push((entry.name.clone(), decoded));
    }

    let options = MergeOptions {
        on_overlap,
        ..MergeOptions::default()
    };
    let report = merge::merge(&pieces, &options)?;

    for warning in &report.warnings {
        out.warn(warning.clone());
    }
    for overlap in &report.overlaps {
        out.warn(format!(
            "{} blocks overlapped between {:?} and {:?}",
            overlap.count, overlap.pieces[0], overlap.pieces[1]
        ));
    }

    let bytes = mcstructure::encode(&report.structure, &target.display().to_string())?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, &bytes)?;

    let s = &report.structure;
    out.line(format!(
        "wrote {} ({}) — {} x {} x {} from {} structures",
        target.display(),
        human_size(bytes.len() as u64),
        s.size.x,
        s.size.y,
        s.size.z,
        pieces.len()
    ));

    out.emit(MergedPayload {
        world: view.world.map(World::qualified),
        merged: Merged {
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
            sources: pieces.into_iter().map(|(n, _)| n).collect(),
            size: [s.size.x, s.size.y, s.size.z],
            origin: [s.origin.x, s.origin.y, s.origin.z],
            overlaps: report
                .overlaps
                .iter()
                .map(|o| OverlapRow {
                    count: o.count,
                    pieces: o.pieces.clone(),
                })
                .collect(),
        },
    });
    Ok(())
}
