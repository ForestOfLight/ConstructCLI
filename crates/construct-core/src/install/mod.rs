//! Installing Construct: place the packs, keep the user's structures.

pub mod mcaddon;
pub mod releases;

use crate::error::{CoreError, Result};
use crate::pack::{self, manifest};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

/// Prefix for the staging directory `place` uses while swapping a pack in.
/// Dotted so it is plainly not a pack folder. `find_installed_by_uuid`
/// additionally excludes any directory carrying this prefix by name: mid-run
/// (or after a crash, before the next run recovers or ignores it) a staging
/// directory can carry a manifest with the same header UUID as the pack it
/// is staging, and must never be mistaken for the installed copy.
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
///
/// The remove-then-rename swap at the end is not itself atomic, so a crash
/// between the two can leave a destination that is gone and a staging
/// directory that holds the complete replacement. `place` recovers that
/// state on its next run rather than ever deleting a staging directory it
/// did not create in this call — see `recover_finished_staging`.
pub fn place(root: &Path, src: &Path, force: bool) -> Result<Placed> {
    let incoming = manifest::read(src)?;

    // Complete any swap a previous run left half-finished, or leave it
    // strictly alone if this call cannot positively confirm it is safe to
    // move. Must run before the UUID lookup below: a recovered directory
    // needs to be either gone (renamed into place) or excluded from that
    // lookup, never left sitting there as a UUID-matching impostor.
    recover_finished_staging(root);

    let existing = find_installed_by_uuid(root, &incoming.uuid);

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
    let dest_name = dest
        .file_name()
        .expect("dest is always root joined with a folder name")
        .to_string_lossy()
        .into_owned();

    // Stage the new pack beside the destination, on the same filesystem, so
    // the final swap is a rename rather than a copy. Nothing belonging to
    // `dest` has been touched yet.
    let staging = root.join(staging_name(&dest_name));
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
    // rename, no bulk I/O in between. If either step fails, the staged copy
    // is left exactly where it is — named in the error — rather than being
    // cleaned up: it is a complete, ready-to-use replacement, and deleting
    // it here would be the same mistake this whole restructure exists to
    // avoid.
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

    Ok(Placed {
        dir: dest,
        from: existing.map(|e| e.manifest.version),
        to: incoming.version,
        preserved,
        changed: true,
    })
}

/// A staging directory name that records which destination folder it is
/// staging for — so a later run can recover it — plus a per-run,
/// collision-free suffix.
fn staging_name(dest_name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{STAGING_PREFIX}{}-{nanos}-{dest_name}", std::process::id())
}

/// The destination folder name encoded in a staging directory's name, if it
/// looks like one `place` created. `None` for anything else under `root`,
/// including a staging name too malformed to have a recorded destination.
fn staged_dest_name(staging_dir_name: &str) -> Option<&str> {
    let rest = staging_dir_name.strip_prefix(STAGING_PREFIX)?;
    let mut parts = rest.splitn(3, '-');
    parts.next()?; // pid
    parts.next()?; // nanos
    parts.next()
}

fn is_staging_name(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with(STAGING_PREFIX))
        .unwrap_or(false)
}

/// Like `pack::find_by_uuid`, but ignores this module's own staging
/// directories — see `STAGING_PREFIX`. Without this, a leftover or
/// in-progress staging directory carrying the same header UUID as the pack
/// it is staging could be matched instead of the real installed copy.
fn find_installed_by_uuid(root: &Path, uuid: &str) -> Option<pack::Pack> {
    pack::packs_in(root)
        .into_iter()
        .find(|p| p.manifest.uuid == uuid && !is_staging_name(&p.dir))
}

/// Completes any swap a previous run of `place` left interrupted between
/// `remove_dir_all(&dest)` and the `rename` that follows it — the one window
/// where `dest` is briefly absent and the staged replacement is the only
/// copy of the user's data left on disk.
///
/// For each staging directory found under `root`: if its recorded
/// destination does not exist and the staging directory holds a readable
/// manifest, the interrupted swap is completed by renaming it into place —
/// the subsequent install then usually proceeds as an idempotent no-op. A
/// staging directory this call cannot positively confirm is a finished,
/// abandoned swap — no readable manifest, or a destination that already
/// exists again (which is what another process's staging directory looks
/// like while that process is still writing it) — is left completely alone.
/// Leaked garbage is a housekeeping annoyance; deleting a directory that
/// might still be someone's only copy of the user's data is not a trade
/// worth making to avoid it.
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
        if manifest::read(&path).is_ok() {
            let _ = std::fs::rename(&path, &dest);
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
