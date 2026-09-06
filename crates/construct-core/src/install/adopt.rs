//! Rescuing a Construct that was installed into the wrong folder.
//!
//! `behavior_packs` and `development_behavior_packs` sit side by side under
//! `com.mojang`, and a player installing Construct by hand — the route the
//! README still describes — readily drops it into the first. The game loads
//! packs from both, so a copy there is not merely inert: it carries the same
//! header UUID as the copy this tool installs, and which of the two the game
//! ends up loading is not something either the player or this tool decides.
//! Meanwhile every command here resolves Construct through
//! `pack::behavior_root`, so the misplaced copy is invisible to them and the
//! structures inside it are unreachable.
//!
//! [`adopt`] folds that copy into the development root before an install
//! places anything, so the structures inside it are carried across the
//! version upgrade by `place`'s ordinary preservation and the duplicate stops
//! shadowing. Nothing here deletes a byte of the user's data that it has not
//! first confirmed is present in the development copy.

use crate::error::{CoreError, Result};
use crate::pack::{self, structures};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

/// How many suffixed names to try before giving up. Reaching this means
/// something is generating names in a loop, not that a user has a thousand
/// copies of one structure.
const MAX_SUFFIX: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptKind {
    /// The development root had no copy, so the misplaced one became it.
    Moved,
    /// The development root already had a copy, so the misplaced one's
    /// structures were folded into it and the misplaced copy removed.
    Merged,
}

/// A structure that existed in both copies under one name, with different
/// contents, and so was kept under a second name rather than dropped. Paths
/// are relative to the pack's `structures/`, which is what the user reads as
/// a structure id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct Adopted {
    /// Where the misplaced copy was.
    pub from: PathBuf,
    /// Where its contents are now.
    pub to: PathBuf,
    pub kind: AdoptKind,
    /// Structure files written into the development copy — every file that
    /// was carried across, including the [`Adopted::rescued`] ones. Always 0
    /// for a [`AdoptKind::Moved`], where the whole pack moved as it stood.
    pub merged: usize,
    pub rescued: Vec<Rescued>,
    /// Set when every structure arrived safely but the emptied misplaced
    /// directory could not be removed. Not an error — the user's data is in
    /// the development copy either way — but the duplicate is still there and
    /// the caller should say so.
    pub left_behind: Option<String>,
}

/// Folds a pack carrying `uuid` out of `stray_root` and into `dev_root`.
///
/// `Ok(None)` — the overwhelmingly common answer — means `stray_root` holds
/// no such pack and nothing was touched.
///
/// With no copy in `dev_root`, the pack is moved wholesale: a rename where
/// the two roots share a filesystem, which they do whenever they are the
/// usual siblings under `com.mojang`. With a copy already there, that copy is
/// authoritative — it is the one this tool installs and upgrades — and only
/// the misplaced copy's `structures/` is folded into it, by
/// [`merge_structures`]. The misplaced directory is removed only once that
/// has returned `Ok`.
pub fn adopt(dev_root: &Path, stray_root: &Path, uuid: &str) -> Result<Option<Adopted>> {
    // A caller passing one root twice would otherwise merge the development
    // copy into itself and then delete it.
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

/// Moves the whole misplaced pack into `dev_root` under a free folder name.
///
/// The rename is attempted first and is atomic when it succeeds. The copy
/// fallback exists for the case where the two roots turn out not to share a
/// filesystem — one of them a symlink onto another mount, say — and is
/// ordered so that a failure part-way through leaves the misplaced copy
/// exactly as it was: the partial destination is removed and the original
/// never touched.
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

/// Folds the misplaced copy's structures into the installed one, then removes
/// the misplaced copy — which by then holds nothing the installed copy does
/// not, and is otherwise a stale duplicate of Construct's own files.
fn merge_in(stray: &Path, installed: &Path) -> Result<Adopted> {
    let src = structures::dir(stray);
    let mut merged = 0;
    let mut rescued = Vec::new();
    if src.is_dir() {
        let dst = structures::dir(installed);
        merge_structures(&src, &src, &dst, &mut merged, &mut rescued)?;
    }

    // Only now, with every structure confirmed written.
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

/// Copies every file under `dir` (a subtree of `base`) into `dst_root`,
/// mirroring `base`'s layout.
///
/// Three cases per file, and the third is the whole reason this is not
/// `install::copy_missing`: the destination has no such file, so it is
/// copied; the destination has it with identical bytes, so there is nothing
/// to do; or the destination has it with *different* bytes, in which case the
/// installed copy keeps its own path — it is the one the user's worlds
/// already reference — and this one is written beside it under the first free
/// suffixed name. Skipping it instead would silently destroy a structure that
/// exists nowhere else.
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

/// A `structures/`-relative path as the user reads it: `/`-joined regardless
/// of platform separator, the same way `pack::structures` derives ids.
fn id_path(relative: &Path) -> String {
    relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether two files hold the same bytes. A length mismatch settles it
/// without reading either.
fn same_contents(a: &Path, b: &Path) -> Result<bool> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    Ok(std::fs::read(a)? == std::fs::read(b)?)
}

/// `stem-1.ext`, `stem-2.ext`, … beside `taken`, stopping at the first that
/// does not exist. The suffix goes on the stem so the extension survives:
/// `structures::list` only sees a file as a structure if it still ends in
/// `.mcstructure`, and the subfolder is untouched so the namespace half of
/// the id is unchanged too.
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

/// `name`, else `name-2`, `name-3`, … under `root`. Deliberately the shape
/// `discovery::platform::dedupe_names` already uses for the same problem: the
/// unsuffixed name is the one that is wanted, and a suffix appears only
/// because something else got there first.
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
