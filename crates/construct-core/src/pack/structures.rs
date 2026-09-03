//! Construct's `structures/` folder: ordinary `.mcstructure` files, no database.
//!
//! A file directly in `structures/` is `mystructure:<stem>` — confirmed by
//! Construct's own source, which strips exactly that prefix from the ids the
//! game hands it. A file in `structures/<dir>/` is `<dir>:<stem>`, inferred from
//! the one shipped pack that uses the layout. Anything deeper is not addressable
//! in-game under either reading, so it is not listed.

use crate::error::{CoreError, Result};
use crate::store::key;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "mcstructure";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStructure {
    /// The qualified id, always carrying a namespace.
    pub id: String,
    /// What the user types and sees.
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn dir(pack_dir: &Path) -> PathBuf {
    pack_dir.join("structures")
}

/// Every structure file in a pack, sorted by id.
pub fn list(pack_dir: &Path) -> Vec<PackStructure> {
    let root = dir(pack_dir);
    let mut out = Vec::new();
    collect(&root, None, &mut out);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let ns = e.file_name().to_string_lossy().to_lowercase();
            collect(&e.path(), Some(&ns), &mut out);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn collect(dir: &Path, namespace: Option<&str>, out: &mut Vec<PackStructure>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = match namespace {
            Some(ns) => format!("{ns}:{stem}"),
            None => key::qualify(stem),
        };
        let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(PackStructure {
            name: key::display_name(&id).to_string(),
            id,
            path,
            size_bytes,
        });
    }
}

/// One segment of a structure id, validated for use as a path component.
///
/// This is a security boundary, not a nicety: ids arrive from file stems and
/// from `--name`, so a segment containing a separator or `..` would let a
/// crafted name write outside the pack.
fn safe_segment(segment: &str, whole: &str) -> Result<()> {
    let bad = |reason: &str| CoreError::BadStructureName {
        name: whole.to_string(),
        reason: reason.to_string(),
    };
    if segment.is_empty() {
        return Err(bad("an empty name or namespace"));
    }
    if segment == "." || segment == ".." {
        return Err(bad("a name that would point outside the pack"));
    }
    if let Some(c) = segment
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-'))
    {
        return Err(bad(&format!(
            "{c:?} is not allowed; names may use a-z, 0-9, and _ . -"
        )));
    }
    Ok(())
}

/// Where a structure with this id belongs inside a pack.
pub fn path_for(pack_dir: &Path, id: &str) -> Result<PathBuf> {
    let qualified = key::qualify(id);
    let (namespace, name) =
        qualified
            .split_once(':')
            .ok_or_else(|| CoreError::BadStructureName {
                name: id.to_string(),
                reason: "no namespace".to_string(),
            })?;
    if name.contains(':') {
        return Err(CoreError::BadStructureName {
            name: id.to_string(),
            reason: "more than one namespace separator".to_string(),
        });
    }
    safe_segment(namespace, id)?;
    safe_segment(name, id)?;

    let root = dir(pack_dir);
    let file = format!("{name}.{EXTENSION}");
    Ok(if namespace == key::DEFAULT_NAMESPACE {
        root.join(file)
    } else {
        root.join(namespace).join(file)
    })
}

/// Writes a structure into a pack, refusing an existing file unless forced.
pub fn write(pack_dir: &Path, id: &str, bytes: &[u8], force: bool) -> Result<PathBuf> {
    let path = path_for(pack_dir, id)?;
    if path.exists() && !force {
        return Err(CoreError::TargetExists { path });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

pub fn remove(path: &Path) -> Result<()> {
    std::fs::remove_file(path)?;
    Ok(())
}

/// A structure name from a file stem: lowercased, spaces to `_`.
///
/// Anything outside `[a-z0-9_.-]` is rejected rather than mangled — a mangled
/// name is one Construct will not list, so the user gets told to pass `--name`.
pub fn derive_name(stem: &str) -> Result<String> {
    let derived: String = stem.trim().to_lowercase().replace(' ', "_");
    safe_segment(&derived, stem)?;
    Ok(derived)
}
