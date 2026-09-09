//! Reading a world that Minecraft currently has open.
//!
//! LevelDB has no read-only mode: every open takes `db/LOCK` and runs
//! recovery, which rewrites the database. So every read snapshots `db/` first
//! and opens the snapshot. This is unconditional.
//!
//! Table files are written once and thereafter only read or unlinked, so a
//! snapshot hardlinks them and copies only what leveldb rewrites in place —
//! the log, the MANIFEST, and CURRENT. That split is the safety argument:
//! linking a file the game still appends to would let the next autosave bleed
//! into a snapshot mid-read.
//!
//! TODO: running out of space fails mid-copy. A pre-flight check needs a
//! portable free-space API `std` does not provide.
use crate::discovery::World;
use crate::error::Result;
use crate::store::OpenedStore;
use crate::store::bedrock::BedrockStore;
use std::path::Path;

/// Copies a directory tree, returning the bytes written.
///
/// Every file is a genuine independent copy, which the install paths rely on:
/// a staged pack sharing inodes with its source could not be edited. Snapshots
/// want [`link_or_copy_dir`].
pub fn copy_dir(src: &Path, dst: &Path) -> Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut total = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            total += copy_dir(&entry.path(), &to)?;
        } else {
            total += std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(total)
}

pub fn open_via_snapshot(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();

    let tmp = tempfile::tempdir()?;
    let copy = tmp.path().join("db");
    let copied = link_or_copy_dir(&db, &copy)?;

    let store = BedrockStore::open_copy(&copy)?;

    Ok(OpenedStore {
        inner: Box::new(store),
        _snapshot: Some(tmp),
        via_snapshot: Some(copied),
    })
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => std::fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

fn is_immutable_table(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".ldb") || name.ends_with(".sst")
}

/// Snapshots a leveldb directory, hardlinking what is safe to share.
///
/// Returns the bytes actually *copied*. Linked bytes cost nothing and are not
/// counted, so the figure reflects what the read cost.
///
/// A hardlink is only ever a second name for a file leveldb will not write to
/// again, so the snapshot's own recovery cannot reach the world. When linking
/// is impossible — usually a snapshot on another filesystem — the file is
/// copied instead: slower, identical in effect.
pub fn link_or_copy_dir(src: &Path, dst: &Path) -> Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copied += link_or_copy_dir(&entry.path(), &to)?;
            continue;
        }
        if is_immutable_table(&entry.file_name().to_string_lossy())
            && std::fs::hard_link(entry.path(), &to).is_ok()
        {
            continue;
        }
        copied += std::fs::copy(entry.path(), &to)?;
    }
    Ok(copied)
}
