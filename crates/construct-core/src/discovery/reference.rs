//! The grammar for naming a world on the command line.
//!
//! A reference is a display name, a folder name, a qualified
//! `<installation>/<account>/<world>` with each segment optional from the left,
//! or a filesystem path. The filesystem is checked first so resolution is
//! deterministic. Ambiguity is always an error, never a silent pick.

use crate::discovery::worlds::{LastPlayedSource, World};
use crate::error::{CoreError, Result};
use crate::leveldat;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRef {
    pub installation: Option<String>,
    pub account: Option<String>,
    pub world: String,
}

/// Splits a reference into its segments. Never fails — an unparseable reference
/// is simply a world name that will not match.
pub fn parse(input: &str) -> WorldRef {
    let parts: Vec<&str> = input.split('/').collect();
    match parts.as_slice() {
        [world] => WorldRef {
            installation: None,
            account: None,
            world: (*world).to_string(),
        },
        [installation, world] => WorldRef {
            installation: Some((*installation).to_string()),
            account: None,
            world: (*world).to_string(),
        },
        [installation, account, world, ..] => WorldRef {
            installation: Some((*installation).to_string()),
            account: Some((*account).to_string()),
            world: (*world).to_string(),
        },
        [] => WorldRef {
            installation: None,
            account: None,
            world: String::new(),
        },
    }
}

/// Resolves a reference to exactly one world.
pub fn resolve(input: &str, worlds: &[World]) -> Result<World> {
    // Filesystem first. This is why there is no --path flag: a single global
    // flag could only ever describe one world, and `copy` takes two.
    if let Some(world) = as_path(input) {
        return Ok(world);
    }

    let r = parse(input);
    let matches_segments = |w: &World| {
        r.installation.as_ref().is_none_or(|i| &w.installation == i)
            && r.account
                .as_ref()
                .is_none_or(|a| w.account.as_ref() == Some(a))
    };

    // Folder names are matched before display names.
    let by_folder: Vec<&World> = worlds
        .iter()
        .filter(|w| w.folder == r.world && matches_segments(w))
        .collect();
    let candidates = if by_folder.is_empty() {
        worlds
            .iter()
            .filter(|w| w.display_name == r.world && matches_segments(w))
            .collect::<Vec<_>>()
    } else {
        by_folder
    };

    match candidates.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(CoreError::WorldNotFound {
            reference: input.to_string(),
            near: near_matches(&r.world, worlds),
        }),
        many => Err(CoreError::AmbiguousWorld {
            reference: input.to_string(),
            candidates: many.iter().map(|w| w.qualified()).collect(),
        }),
    }
}

/// A directory containing `level.dat` is a world, wherever it sits.
fn as_path(input: &str) -> Option<World> {
    let path = Path::new(input);
    if !path.join("level.dat").is_file() {
        return None;
    }
    let folder = path.file_name()?.to_string_lossy().into_owned();
    let display_name = std::fs::read_to_string(path.join("levelname.txt"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| folder.clone());
    let (last_played, last_played_source) = match leveldat::read(&path.join("level.dat")) {
        Ok(d) => match d.last_played() {
            Some(v) => (Some(v), LastPlayedSource::LevelDat),
            None => (None, LastPlayedSource::DirMtime),
        },
        Err(_) => (None, LastPlayedSource::DirMtime),
    };

    Some(World {
        // A path-referenced world belongs to no installation. The label keeps
        // `qualified()` total rather than introducing an Option.
        installation: "path".to_string(),
        account: None,
        folder,
        display_name,
        path: path.to_path_buf(),
        last_played,
        last_played_source,
        size_bytes: 0,
    })
}

/// Case-insensitive substring matches, for "did you mean".
fn near_matches(needle: &str, worlds: &[World]) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut out: Vec<String> = worlds
        .iter()
        .filter(|w| {
            w.folder.to_lowercase().contains(&needle)
                || w.display_name.to_lowercase().contains(&needle)
        })
        .map(|w| format!("{} ({})", w.display_name, w.qualified()))
        .collect();
    out.truncate(5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::worlds::LastPlayedSource;
    use std::path::PathBuf;

    fn w(installation: &str, account: Option<&str>, folder: &str, name: &str) -> World {
        World {
            installation: installation.to_string(),
            account: account.map(str::to_string),
            folder: folder.to_string(),
            display_name: name.to_string(),
            path: PathBuf::from("/tmp").join(folder),
            last_played: Some(0),
            last_played_source: LastPlayedSource::LevelDat,
            size_bytes: 0,
        }
    }

    fn fixture() -> Vec<World> {
        vec![
            w("release", Some("Shared"), "Ssu8ww1SFbM=", "Test World"),
            w("release", Some("2533274801234567"), "aBc0=", "Test World"),
            w("preview", Some("Shared"), "XyZ9=", "Test World"),
            w("mcpelauncher", None, "Amelix CMP", "Amelix CMP"),
        ]
    }

    #[test]
    fn parses_a_bare_name() {
        let r = parse("Amelix CMP");
        assert_eq!(r.installation, None);
        assert_eq!(r.account, None);
        assert_eq!(r.world, "Amelix CMP");
    }

    #[test]
    fn parses_installation_and_world() {
        let r = parse("release/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account, None);
        assert_eq!(r.world, "Ssu8ww1SFbM=");
    }

    #[test]
    fn parses_the_full_three_segment_form() {
        let r = parse("release/Shared/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account.as_deref(), Some("Shared"));
        assert_eq!(r.world, "Ssu8ww1SFbM=");
    }

    #[test]
    fn resolves_an_unambiguous_display_name() {
        assert_eq!(
            resolve("Amelix CMP", &fixture()).unwrap().folder,
            "Amelix CMP"
        );
    }

    #[test]
    fn a_folder_name_beats_a_display_name() {
        let worlds = vec![
            w("mcpelauncher", None, "target", "decoy"),
            w("mcpelauncher", None, "other", "target"),
        ];
        assert_eq!(resolve("target", &worlds).unwrap().folder, "target");
    }

    #[test]
    fn an_ambiguous_name_is_an_error_listing_qualified_forms() {
        let err = resolve("Test World", &fixture()).unwrap_err();
        let CoreError::AmbiguousWorld { candidates, .. } = err else {
            panic!("expected AmbiguousWorld, got {err:?}");
        };
        assert_eq!(candidates.len(), 3);
        assert!(candidates.contains(&"release/Shared/Ssu8ww1SFbM=".to_string()));
        assert!(candidates.contains(&"preview/Shared/XyZ9=".to_string()));
    }

    #[test]
    fn qualifying_disambiguates() {
        assert_eq!(
            resolve("release/Shared/Ssu8ww1SFbM=", &fixture())
                .unwrap()
                .folder,
            "Ssu8ww1SFbM="
        );
    }

    #[test]
    fn the_installation_segment_alone_can_disambiguate() {
        assert_eq!(
            resolve("preview/Test World", &fixture()).unwrap().folder,
            "XyZ9="
        );
    }

    #[test]
    fn a_missing_world_suggests_near_matches() {
        let err = resolve("Amelix", &fixture()).unwrap_err();
        let CoreError::WorldNotFound { near, .. } = err else {
            panic!("expected WorldNotFound, got {err:?}");
        };
        assert!(near.iter().any(|n| n.contains("Amelix CMP")));
    }

    #[test]
    fn a_filesystem_path_wins_over_a_name() {
        // Filesystem check first, so resolution is deterministic.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Amelix CMP");
        std::fs::create_dir_all(dir.join("db")).unwrap();
        std::fs::write(dir.join("level.dat"), b"x").unwrap();

        let got = resolve(dir.to_str().unwrap(), &fixture()).unwrap();
        assert_eq!(got.path, dir);
        assert_eq!(got.installation, "path");
    }

    #[test]
    fn a_path_without_level_dat_is_not_a_path_reference() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            resolve(dir.to_str().unwrap(), &fixture()),
            Err(CoreError::WorldNotFound { .. })
        ));
    }
}
