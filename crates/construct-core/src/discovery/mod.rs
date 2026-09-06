pub mod installation;
pub mod platform;
pub mod reference;
pub mod worlds;

pub use platform::{Candidate, Installation, WorldRoot};
pub use worlds::{LastPlayedSource, PATH_INSTALLATION, World, enumerate};

/// What a directory named on the command line turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// A `com.mojang` directory: probe it for `minecraftWorlds` and dev packs.
    Root,
    /// A world directory, sitting anywhere at all.
    World,
}

/// Sorts a `--path` value into the two things it may be.
///
/// A `level.dat` is what makes a directory a world, and is the same test
/// `enumerate` applies. Everything else is treated as a `com.mojang` root
/// without insisting on the name or on the directory existing: a root that
/// turns out to hold nothing is simply absent from discovery, which is how an
/// unplugged external drive has always behaved.
pub fn classify(path: &std::path::Path) -> PathKind {
    if path.join("level.dat").is_file() {
        PathKind::World
    } else {
        PathKind::Root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_directory_holding_level_dat_is_a_world() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("level.dat"), b"x").unwrap();
        assert_eq!(classify(tmp.path()), PathKind::World);
    }

    #[test]
    fn a_com_mojang_directory_is_a_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("games/com.mojang");
        fs::create_dir_all(root.join("minecraftWorlds")).unwrap();
        assert_eq!(classify(&root), PathKind::Root);
    }

    #[test]
    fn a_root_is_not_required_to_be_named_com_mojang() {
        // Renamed backups and bind mounts are common; the name is not the test.
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("minecraftWorlds")).unwrap();
        assert_eq!(classify(tmp.path()), PathKind::Root);
    }

    #[test]
    fn a_path_that_does_not_exist_is_a_root() {
        // Not an error: an absent root drops out of discovery on its own.
        assert_eq!(
            classify(std::path::Path::new("/nonexistent/com.mojang")),
            PathKind::Root
        );
    }
}
