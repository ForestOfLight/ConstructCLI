//! A `.mcaddon` is a zip holding one behaviour pack and one resource pack.

use crate::error::{CoreError, Result};
use crate::pack::manifest::{self, PackKind};
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub struct Extracted {
    /// Held so the extraction outlives the paths below.
    pub dir: tempfile::TempDir,
    pub behavior: PathBuf,
    pub resource: PathBuf,
}

/// Rejects any archive entry that would write outside the extraction directory.
///
/// Zip slip: an entry named `../../.bashrc` escapes wherever you unpack it. One
/// bad entry refuses the whole archive rather than being skipped, because a
/// partial extraction of a hostile archive is not something to carry on with.
fn safe_entry(name: &str) -> Result<PathBuf> {
    let bad = |reason: &str| CoreError::BadPack {
        path: PathBuf::from(name),
        reason: reason.to_string(),
    };
    if name.contains('\\') {
        return Err(bad("a backslash in an archive path"));
    }
    let path = Path::new(name);
    for c in path.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(bad("a path that would escape the extraction directory")),
        }
    }
    Ok(path.to_path_buf())
}

pub fn extract(archive: &Path) -> Result<Extracted> {
    let bad = |reason: String| CoreError::BadPack {
        path: archive.to_path_buf(),
        reason,
    };

    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    let dir = tempfile::tempdir()?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| bad(e.to_string()))?;
        let name = entry.name().to_string();
        let relative = safe_entry(&name)?;
        let out = dir.path().join(&relative);

        if name.ends_with('/') {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Entries carrying a symlink's unix mode are never turned into real
        // symlinks here — they fall through to this same `File::create` and
        // are written as an ordinary file holding the link-target text as its
        // content. That sidesteps the classic zip-slip-via-symlink chain
        // (link a name outside the tree, then a later entry writes "through"
        // it) without needing to special-case it.
        let mut writer = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut writer)?;
    }

    // Which is which comes from the manifests, never from the folder names.
    let mut behavior = None;
    let mut resource = None;
    for e in std::fs::read_dir(dir.path())?.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(m) = manifest::read(&path) else {
            continue;
        };
        match m.kind {
            PackKind::Behavior => behavior = Some(path),
            PackKind::Resource => resource = Some(path),
        }
    }

    let behavior = behavior.ok_or_else(|| bad("no behaviour pack in this .mcaddon".into()))?;
    let resource = resource.ok_or_else(|| bad("no resource pack in this .mcaddon".into()))?;
    Ok(Extracted {
        dir,
        behavior,
        resource,
    })
}
