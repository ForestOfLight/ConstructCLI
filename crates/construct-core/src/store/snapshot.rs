//! Reading a world that Minecraft currently has open.
//!
//! LevelDB's C++ API acquires `db/LOCK` on every open and offers no read-only
//! mode — opening a database runs recovery, which replays the log into new
//! tables, rewrites the MANIFEST, and deletes what it considers obsolete. So
//! the only way to read a live world is to open something that is not the live
//! world. This is unconditional, not gated behind a flag.
//!
//! Snapshotting is not a wholesale copy. A leveldb table file is written once
//! and thereafter only read or unlinked, so the snapshot takes a hardlink to it
//! rather than duplicating the bytes — and on a real world the tables are very
//! nearly all of the bytes. Only the files leveldb writes in place are copied:
//! the log and the MANIFEST, which are appended to, and CURRENT, which is
//! replaced. See [`link_or_copy_dir`].
//!
//! That distinction is the whole safety argument. Linking a file the game still
//! appends to would let its next autosave bleed into a snapshot mid-read, which
//! is the tearing a snapshot exists to prevent; linking one it will never write
//! to again cannot. Nothing the snapshot's own recovery does can reach back
//! through a link, either — deleting the snapshot drops a name rather than a
//! file, and a compaction inside it writes new files under new numbers.
//!
//! There is no pre-flight free-space check: if the filesystem runs out of space
//! partway through, the copy fails with the OS's own error, which surfaces to
//! the caller as `CoreError::Io`. That is the only signal a caller gets today.
//! Linking has made this much less likely to matter, since what gets written is
//! now the log and the MANIFEST rather than the whole world.
//!
//! TODO: a pre-flight check would turn a slow mid-copy failure into an
//! immediate one, but it needs a portable free-space API that `std` does not
//! provide.

use crate::discovery::World;
use crate::error::Result;
use crate::store::OpenedStore;
use crate::store::bedrock::BedrockStore;
use std::path::Path;

/// Copies a directory tree, returning the number of bytes written.
///
/// Every file is a genuine independent copy. The install paths rely on that —
/// a staged pack that shared inodes with its source would not be a pack that
/// could be edited. Snapshots want [`link_or_copy_dir`] instead.
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

/// Snapshots `db/` into a temp directory and opens the snapshot.
pub fn open_via_snapshot(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();

    let tmp = tempfile::tempdir()?;
    let copy = tmp.path().join("db");
    let copied = link_or_copy_dir(&db, &copy)?;

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

/// Whether leveldb ever writes to a file again after creating it.
///
/// Table files are written once and thereafter only read or unlinked, so a
/// snapshot can share the inode rather than duplicate the bytes — and on a real
/// world they are very nearly all of the bytes. Everything else in a leveldb
/// directory is written in place: the write-ahead log and the MANIFEST are
/// appended to, CURRENT is replaced. Those must be copied, or a running game's
/// next autosave would appear inside a snapshot already being read.
///
/// `.sst` is the extension older leveldb builds used for the same thing; the
/// worlds this tool reads can predate the rename.
fn is_immutable_table(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".ldb") || name.ends_with(".sst")
}

/// Snapshots a leveldb directory, hardlinking what is safe to share.
///
/// Returns the number of bytes actually *copied*; linked bytes cost nothing and
/// are deliberately not counted, so the figure reflects what the read cost.
///
/// A hardlink is only ever a second name for a file leveldb will not write to
/// again, so nothing the snapshot's own recovery does can reach the world:
/// deleting the snapshot drops a name, and a compaction inside it writes new
/// files under new numbers. When linking is impossible — a snapshot on a
/// different filesystem is the usual reason — the file is copied instead, which
/// is slower but identical in effect.
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
        // `is_ok` rather than `?`: a link that cannot be made is not an error,
        // it is a copy. This is also the cross-filesystem fallback.
        if is_immutable_table(&entry.file_name().to_string_lossy())
            && std::fs::hard_link(entry.path(), &to).is_ok()
        {
            continue;
        }
        copied += std::fs::copy(entry.path(), &to)?;
    }
    Ok(copied)
}
