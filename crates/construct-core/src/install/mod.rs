//! Installing Construct: place the packs, keep the user's structures.

pub mod mcaddon;
pub mod releases;

use crate::error::{CoreError, Result};
use crate::pack::{self, manifest};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

/// Prefix for the staging directory `place` uses while swapping a pack in.
/// Dotted so it is plainly not a pack folder, and `pack::packs_in` skips it
/// anyway on the merits: it filters on whether `manifest.json` parses, not
/// on the name, so a staging directory with no manifest yet (or a corrupt
/// one) is simply invisible to it.
const STAGING_PREFIX: &str = ".constructcli-staging-";

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
///
/// The new pack is staged fully beside `root` before anything belonging to
/// the old install is touched. Only once the stage is complete — the new
/// pack copied in and the user's structures carried across from the still-
/// live original — is the old directory removed and the stage renamed into
/// its place. Every fallible step that does bulk I/O (the copy of the new
/// pack, the copy of preserved structures) happens while the original is
/// completely intact, so a failure there — disk full, most plausibly —
/// leaves the user with their original pack, not with neither copy.
pub fn place(root: &Path, src: &Path, force: bool) -> Result<Placed> {
    let incoming = manifest::read(src)?;

    // A staging directory left behind by a previous run that crashed or was
    // killed between the remove and the rename must not corrupt this run:
    // it would carry the same header UUID as the pack it was staging, and
    // `find_by_uuid` would otherwise happily match it instead of the real
    // installed copy.
    cleanup_stale_staging(root);

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

    // Stage the new pack beside the destination, on the same filesystem, so
    // the final swap is a rename rather than a copy. Nothing belonging to
    // `dest` has been touched yet.
    let staging = root.join(staging_name());
    if let Err(e) = copy_dir(src, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Carry the user's structures across, straight from the still-live
    // original into the stage — skipping anything the new version already
    // ships. No separate backup copy is needed: the original stays intact
    // until the swap below, so it *is* the backup for as long as one might
    // be needed.
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

    // The point of no return: from here it is a delete plus a same-directory
    // rename, no bulk I/O in between, and the destination directory only
    // ever moves — it is never simultaneously absent and unwritten.
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::rename(&staging, &dest).map_err(|e| CoreError::IncompleteInstall {
        dest: dest.clone(),
        staging,
        reason: e.to_string(),
    })?;

    Ok(Placed {
        dir: dest,
        from: existing.map(|e| e.manifest.version),
        to: incoming.version,
        preserved,
        changed: true,
    })
}

/// A staging directory name that cannot collide with a pack folder and is
/// unique to this run.
fn staging_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{STAGING_PREFIX}{}-{nanos}", std::process::id())
}

/// Removes any staging directories left behind under `root` by a previous
/// run that did not finish. Best-effort: a `root` that does not exist yet,
/// or a directory this process cannot remove, is not this function's
/// problem to solve.
fn cleanup_stale_staging(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Copies every file under `src` into `dst`, mirroring `src`'s relative
/// layout, skipping any file `dst` already has. Returns how many files were
/// actually copied.
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
