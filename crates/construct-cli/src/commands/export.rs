//! Writing structures out as `.mcstructure` files.
//!
//! A structure's leveldb value is byte-identical to a `.mcstructure` file, so
//! this command copies bytes and parses nothing.

use crate::commands::catalog as loader;
use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::mcstructure;
use construct_core::merge::{self, MergeOptions, OnOverlap};
use construct_core::store::StructureStore;
use serde::Serialize;
use std::io;
use std::path::{Component, Path, PathBuf};

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

/// A derived filename must be a single path component: no `/` or `\`
/// anywhere in it, and no `..`/root/prefix component. This is a security
/// boundary (structure names come from a world file the user may not have
/// authored), so a violation is refused, never guessed around. An explicit
/// `-o` path is the user's own choice and is never subject to this check.
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
                 outside the output directory. Pass -o to choose the destination \
                 explicitly."
            ),
        )));
    }
    Ok(())
}

/// A structure name that decodes to the empty string (e.g. from a key like
/// `structuretemplate_mystructure:`) would derive the filename
/// `.mcstructure` — a hidden file with no name. Refused, in the same style
/// as a traversal attempt; an explicit `-o` path chooses the destination
/// directly and is not subject to this check.
fn refuse_empty_derived_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "structure has an empty name and cannot be used as a filename. \
             Pass -o to choose the destination explicitly."
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

#[allow(clippy::too_many_arguments)]
pub fn run(
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
    let entries = loaded.entries;

    if merge {
        return run_merge(
            world,
            &entries,
            loaded.store.as_ref(),
            structures,
            output,
            source,
            force,
            on_overlap,
            out,
        );
    }

    // Resolve every name and target path before writing anything, so a
    // collision — or a refused name — stops the whole command rather than
    // leaving half a job done.
    let mut plan: Vec<(catalog::Entry, PathBuf)> = Vec::new();
    for name in structures {
        let entry = catalog::resolve(name, &entries, source)?;
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

    let store = loaded.store.as_ref().map(|s| s as &dyn StructureStore);
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
        world: world.qualified(),
        written,
    });
    Ok(())
}

#[derive(Serialize)]
struct MergedPayload {
    world: String,
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
    world: &World,
    entries: &[catalog::Entry],
    store: Option<&construct_core::store::OpenedStore>,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    let target = output.expect("main.rs refuses --merge without -o");
    if target.exists() && !force {
        return Err(CoreError::TargetExists {
            path: target.to_path_buf(),
        });
    }

    let store = store.map(|s| s as &dyn StructureStore);
    let mut pieces = Vec::new();
    for name in structures {
        let entry = catalog::resolve(name, entries, source)?;
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
    out.line("Reload the world before Construct sees it.");

    out.emit(MergedPayload {
        world: world.qualified(),
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
