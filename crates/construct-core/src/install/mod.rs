//! Installing Construct: place the packs, keep the user's structures.

pub mod adopt;
pub mod mcaddon;
pub mod releases;

use crate::error::{CoreError, Result};
use crate::pack::{self, manifest};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

const STAGING_PREFIX: &str = ".constructcli-staging-";

const STAGING_SENTINEL: &str = ".constructcli-staging-complete";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub dir: PathBuf,
    pub from: Option<[u32; 3]>,
    pub to: [u32; 3],
    /// How many of the user's structures were carried across an upgrade.
    pub preserved: usize,
    pub changed: bool,
}

/// Places an extracted pack into a `development_*_packs` root.
///
/// Staged fully beside `root` before the old install is touched, so a failure
/// during any bulk copy leaves the user with their original pack rather than
/// neither copy.
///
/// The closing remove-then-rename is not atomic. A crash between the two
/// leaves the staging directory holding the only complete replacement, which
/// the next `place` recovers.
pub fn place(root: &Path, src: &Path, force: bool) -> Result<Placed> {
    let incoming = manifest::read(src)?;

    recover_finished_staging(root);

    let existing = pack::find_by_uuid(root, &incoming.uuid);

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
        Some(e) => e.dir.clone(),
        None => root.join(src.file_name().ok_or_else(|| CoreError::BadPack {
            path: src.to_path_buf(),
            reason: "the extracted pack has no folder name".to_string(),
        })?),
    };
    let dest_name = dest
        .file_name()
        .expect("dest is always root joined with a folder name")
        .to_string_lossy()
        .into_owned();

    let staging = root.join(staging_name(&dest_name));
    if let Err(e) = copy_dir(src, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let mut preserved = 0;
    let old_structures = pack::structures::dir(&dest);
    if old_structures.is_dir() {
        let new_structures = pack::structures::dir(&staging);
        match copy_missing(&old_structures, &new_structures) {
            Ok(n) => preserved = n,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
        }
    }

    if let Err(e) = std::fs::write(staging.join(STAGING_SENTINEL), b"") {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(CoreError::from(e));
    }

    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| CoreError::IncompleteInstall {
            dest: dest.clone(),
            staging: staging.clone(),
            reason: format!("the old copy could not be fully removed: {e}"),
        })?;
    }
    std::fs::rename(&staging, &dest).map_err(|e| CoreError::IncompleteInstall {
        dest: dest.clone(),
        staging,
        reason: format!("the staged pack could not be moved into place: {e}"),
    })?;

    let _ = std::fs::remove_file(dest.join(STAGING_SENTINEL));

    Ok(Placed {
        dir: dest,
        from: existing.map(|e| e.manifest.version),
        to: incoming.version,
        preserved,
        changed: true,
    })
}

fn staging_name(dest_name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{STAGING_PREFIX}{}-{nanos}-{dest_name}", std::process::id())
}

fn staged_dest_name(staging_dir_name: &str) -> Option<&str> {
    let rest = staging_dir_name.strip_prefix(STAGING_PREFIX)?;
    let mut parts = rest.splitn(3, '-');
    parts.next()?;
    parts.next()?;
    parts.next()
}

fn recover_finished_staging(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(dest_name) = staged_dest_name(name) else {
            continue;
        };
        let dest = root.join(dest_name);
        if dest.exists() {
            continue;
        }
        if path.join(STAGING_SENTINEL).is_file() && std::fs::rename(&path, &dest).is_ok() {
            let _ = std::fs::remove_file(dest.join(STAGING_SENTINEL));
        }
    }
}

fn copy_missing(src: &Path, dst: &Path) -> Result<usize> {
    let mut count = 0;
    copy_missing_into(src, src, dst, &mut count)?;
    Ok(count)
}

fn copy_missing_into(dir: &Path, base: &Path, dst_root: &Path, count: &mut usize) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            copy_missing_into(&path, base, dst_root, count)?;
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .expect("path was walked from base, so it is under base");
        let to = dst_root.join(relative);
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &to)?;
        *count += 1;
    }
    Ok(())
}
