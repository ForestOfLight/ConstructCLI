//! Construct's `structures/` folder: ordinary `.mcstructure` files, no
//! database.
//!
//! A file directly in `structures/` is `mystructure:<stem>`, confirmed by
//! Construct's own source, which strips exactly that prefix from the ids the
//! game hands it. Below that, the first subfolder is the namespace and every
//! folder after it is part of the name: `structures/stuff/towers/diamond` is
//! `stuff:towers/diamond`.
//!
//! The walk goes to full depth — everything below the first subfolder is
//! addressable in-game. See `docs/bedrock-mcstructure-files.md`.

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
    collect(&root, &root, &mut out);
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PackStructure>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect(root, &path, out);
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let mut components: Vec<&str> = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        if let Some(last) = components.last_mut() {
            *last = stem;
        }
        let id = match components.split_first() {
            Some((namespace, rest)) if !rest.is_empty() => {
                format!("{}:{}", namespace, rest.join("/"))
            }
            _ => key::qualify(stem),
        };
        let size_bytes = std::fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0);
        out.push(PackStructure {
            name: key::display_name(&id).to_string(),
            id,
            path,
            size_bytes,
        });
    }
}

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
        .find(|c| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '-'))
    {
        return Err(bad(&format!(
            "{c:?} is not allowed; names may use A-Z, a-z, 0-9, and _ . -"
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
    if namespace == key::DEFAULT_NAMESPACE && name.contains('/') {
        return Err(CoreError::BadStructureName {
            name: id.to_string(),
            reason: "the mystructure namespace has no folder of its own, so a \
                     nested name there would be read back under a different \
                     namespace"
                .to_string(),
        });
    }
    let segments: Vec<&str> = name.split('/').collect();
    for segment in &segments {
        safe_segment(segment, id)?;
    }

    let root = dir(pack_dir);
    let mut path = if namespace == key::DEFAULT_NAMESPACE {
        root
    } else {
        root.join(namespace)
    };
    let (file, parents) = segments.split_last().expect("split always yields one");
    for parent in parents {
        path.push(parent);
    }
    path.push(format!("{file}.{EXTENSION}"));
    Ok(path)
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

/// A structure name from a file stem: trimmed, spaces to `_`, case kept.
///
/// Anything outside `[A-Za-z0-9_.-]` is rejected rather than mangled, since a
/// mangled name is one Construct will not list — the user is told to pass
/// `--name` instead. Case is kept for the same reason: the game stores names
/// like `10HzCounter`, so lowercasing would hand back a name quietly different
/// from the one asked for.
pub fn derive_name(stem: &str) -> Result<String> {
    let derived: String = stem.trim().replace(' ', "_");
    safe_segment(&derived, stem)?;
    Ok(derived)
}
