//! Installing Construct: place the packs, keep the user's structures.

pub mod mcaddon;
pub mod releases;

use crate::error::{CoreError, Result};
use crate::pack::{self, manifest};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub dir: PathBuf,
    /// The version that was there before, if any.
    pub from: Option<[u32; 3]>,
    pub to: [u32; 3],
    /// How many of the user's structures were carried across an upgrade.
    pub preserved: usize,
    pub changed: bool,
}

/// Places an extracted pack into a `development_*_packs` root.
pub fn place(root: &Path, src: &Path, force: bool) -> Result<Placed> {
    let incoming = manifest::read(src)?;
    let existing = pack::find_by_uuid(root, &incoming.uuid);

    // Idempotent: the same version already installed is nothing to do.
    if let Some(existing) = &existing
        && existing.manifest.version == incoming.version
        && !force
    {
        return Ok(Placed {
            dir: existing.dir.clone(),
            from: Some(existing.manifest.version),
            to: incoming.version,
            preserved: 0,
            changed: false,
        });
    }

    let dest = match &existing {
        // Matched by UUID, so a renamed folder is upgraded where it stands.
        Some(e) => e.dir.clone(),
        None => root.join(src.file_name().ok_or_else(|| CoreError::BadPack {
            path: src.to_path_buf(),
            reason: "the extracted pack has no folder name".to_string(),
        })?),
    };

    // Carry the user's structures out of harm's way before the old directory
    // goes. This is the data the whole tool exists to put there.
    let holding = tempfile::tempdir()?;
    let mut preserved = Vec::new();
    let old_structures = pack::structures::dir(&dest);
    if old_structures.is_dir() {
        copy_dir(&old_structures, holding.path())?;
        preserved = relative_files(holding.path());
    }

    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;
    copy_dir(src, &dest)?;

    // Restore everything the new version does not ship itself.
    let new_structures = pack::structures::dir(&dest);
    let mut restored = 0;
    for relative in &preserved {
        let to = new_structures.join(relative);
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(holding.path().join(relative), &to)?;
        restored += 1;
    }

    Ok(Placed {
        dir: dest,
        from: existing.map(|e| e.manifest.version),
        to: incoming.version,
        preserved: restored,
        changed: true,
    })
}

/// Every file under `dir`, as paths relative to it.
fn relative_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
