//! Reading a world that Minecraft currently has open.
//!
//! LevelDB's C++ API acquires `db/LOCK` on every open and offers no read-only
//! mode, so the only way to read a live world is to copy it. This is
//! unconditional, not gated behind a flag. There is no pre-flight free-space
//! check: if the filesystem runs out of space partway through the copy, that
//! copy fails with the OS's own error, which surfaces to the caller as
//! `CoreError::Io`. That is the only signal a caller gets today.
//!
//! TODO: a pre-flight check would turn a slow mid-copy failure on a
//! multi-gigabyte world into an immediate one, but it needs a portable
//! free-space API that `std` does not provide.

use crate::discovery::World;
use crate::error::Result;
use crate::store::OpenedStore;
use crate::store::bedrock::BedrockStore;
use std::path::Path;

/// Copies a directory tree, returning the number of bytes written.
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

/// Copies `db/` to a temp directory and opens the copy.
pub fn open_via_snapshot(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();

    let tmp = tempfile::tempdir()?;
    let copy = tmp.path().join("db");
    let copied = copy_dir(&db, &copy)?;

    // Enforces spec §8's central promise at the point it matters most: a read
    // opens only a copy, never the original. This is an unrecoverable
    // invariant violation, not a runtime error a caller could handle, so it
    // panics rather than returning a `Result` — do not "fix" that.
    crate::store::bedrock::guard_test_path(&copy);

    let store = BedrockStore::open(&copy)?;

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
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}
