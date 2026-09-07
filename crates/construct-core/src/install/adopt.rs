//! Rescuing a Construct that was installed into the wrong folder.
//!
//! `behavior_packs` sits beside `development_behavior_packs`, and a by-hand
//! install readily lands in the first. That copy carries the same header UUID
//! as the one this tool installs, so it shadows it unpredictably while being
//! invisible to every command here.
//!
//! [`adopt`] folds it into the development root before an install places
//! anything. Nothing here deletes a byte it has not first confirmed is present
//! in the development copy.

use crate::error::{CoreError, Result};
use crate::pack::{self, structures};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

const MAX_SUFFIX: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptKind {
    /// The development root had no copy, so the misplaced one became it.
    Moved,
    /// The development root already had a copy, so the misplaced one's
    /// structures were folded into it and the misplaced copy removed.
    Merged,
}

/// A structure that existed in both copies under one name with different
/// contents, kept under a second name rather than dropped.
///
/// Paths are relative to the pack's `structures/`, which is what the user
/// reads as a structure id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct Adopted {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: AdoptKind,
    /// Structure files written into the development copy, including the
    /// [`Adopted::rescued`] ones. Always 0 for [`AdoptKind::Moved`], where the
    /// pack moved whole.
    pub merged: usize,
    pub rescued: Vec<Rescued>,
    /// Set when every structure arrived but the emptied misplaced directory
    /// could not be removed. Not an error — the data is in the development
    /// copy either way — but the duplicate remains and the caller should say
    /// so.
    pub left_behind: Option<String>,
}

/// Folds a pack carrying `uuid` out of `stray_root` into `dev_root`.
///
/// `Ok(None)`, the common answer, means `stray_root` holds no such pack and
/// nothing was touched.
///
/// With no copy in `dev_root` the pack moves wholesale — a rename, where the
/// roots share a filesystem. With a copy already there, that copy is
/// authoritative and only the misplaced `structures/` is folded in by
/// `merge_structures`; the misplaced directory is removed only once that
/// returns `Ok`.
pub fn adopt(dev_root: &Path, stray_root: &Path, uuid: &str) -> Result<Option<Adopted>> {
    if dev_root == stray_root {
        return Ok(None);
    }
    let Some(stray) = pack::find_by_uuid(stray_root, uuid) else {
        return Ok(None);
    };
    match pack::find_by_uuid(dev_root, uuid) {
        None => move_in(&stray.dir, dev_root),
        Some(installed) => merge_in(&stray.dir, &installed.dir),
    }
    .map(Some)
}

fn move_in(stray: &Path, dev_root: &Path) -> Result<Adopted> {
    let name = stray
        .file_name()
        .ok_or_else(|| CoreError::BadPack {
            path: stray.to_path_buf(),
            reason: "the misplaced pack has no folder name".to_string(),
        })?
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(dev_root)?;
    let dest = free_dir(dev_root, &name)?;

    let mut left_behind = None;
    if std::fs::rename(stray, &dest).is_err() {
        if let Err(e) = copy_dir(stray, &dest) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
        left_behind = std::fs::remove_dir_all(stray).err().map(|e| e.to_string());
    }

    Ok(Adopted {
        from: stray.to_path_buf(),
        to: dest,
        kind: AdoptKind::Moved,
        merged: 0,
        rescued: Vec::new(),
        left_behind,
    })
}

fn merge_in(stray: &Path, installed: &Path) -> Result<Adopted> {
    let src = structures::dir(stray);
    let mut merged = 0;
    let mut rescued = Vec::new();
    if src.is_dir() {
        let dst = structures::dir(installed);
        merge_structures(&src, &src, &dst, &mut merged, &mut rescued)?;
    }

    let left_behind = std::fs::remove_dir_all(stray).err().map(|e| e.to_string());

    Ok(Adopted {
        from: stray.to_path_buf(),
        to: installed.to_path_buf(),
        kind: AdoptKind::Merged,
        merged,
        rescued,
        left_behind,
    })
}

fn merge_structures(
    dir: &Path,
    base: &Path,
    dst_root: &Path,
    merged: &mut usize,
    rescued: &mut Vec<Rescued>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            merge_structures(&path, base, dst_root, merged, rescued)?;
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .expect("path was walked from base, so it is under base");
        let to = dst_root.join(relative);

        if to.exists() {
            if same_contents(&path, &to)? {
                continue;
            }
            let free = free_file(&to)?;
            std::fs::copy(&path, &free)?;
            *merged += 1;
            rescued.push(Rescued {
                from: id_path(relative),
                to: id_path(
                    free.strip_prefix(dst_root)
                        .expect("free_file returns a sibling of a path under dst_root"),
                ),
            });
            continue;
        }

        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &to)?;
        *merged += 1;
    }
    Ok(())
}

fn id_path(relative: &Path) -> String {
    relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn same_contents(a: &Path, b: &Path) -> Result<bool> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    Ok(std::fs::read(a)? == std::fs::read(b)?)
}

fn free_file(taken: &Path) -> Result<PathBuf> {
    let parent = taken.parent().unwrap_or(Path::new(""));
    let stem = taken
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = taken
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..MAX_SUFFIX {
        let candidate = parent.join(format!("{stem}-{n}{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(CoreError::BadPack {
        path: taken.to_path_buf(),
        reason: format!("no free name for it after {MAX_SUFFIX} tries"),
    })
}

fn free_dir(root: &Path, name: &str) -> Result<PathBuf> {
    let plain = root.join(name);
    if !plain.exists() {
        return Ok(plain);
    }
    for n in 2..MAX_SUFFIX {
        let candidate = root.join(format!("{name}-{n}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(CoreError::BadPack {
        path: plain,
        reason: format!("no free folder name for it after {MAX_SUFFIX} tries"),
    })
}
