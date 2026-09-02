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
