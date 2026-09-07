//! The grammar for naming a world on the command line.
//!
//! A reference is a display name, a folder name, a qualified
//! `<installation>/<account>/<world>` with each segment optional from the left,
//! or a filesystem path. The filesystem is checked first, so resolution is
//! deterministic.
//!
//! A name may itself contain slashes, so the whole input is tried as a name as
//! well as split into segments; a qualified reading wins only where it is the
//! one that matches. Ambiguity is always an error, never a silent pick.

use crate::discovery::worlds::{LastPlayedSource, World};
use crate::error::{CoreError, Result};
use crate::leveldat;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRef {
    pub installation: Option<String>,
    pub account: Option<String>,
    pub world: String,
    pub extra_segments: bool,
}

fn looks_like_path(input: &str) -> bool {
    let drive_letter = match input.as_bytes() {
        [d, b':', rest @ ..] => {
            d.is_ascii_alphabetic() && matches!(rest, [] | [b'/', ..] | [b'\\', ..])
        }
        _ => false,
    };
    input.starts_with('/') || input.starts_with('\\') || drive_letter
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
            extra_segments: false,
        },
        [installation, world] => WorldRef {
            installation: Some((*installation).to_string()),
            account: None,
            world: (*world).to_string(),
            extra_segments: false,
        },
        [installation, account, world] => WorldRef {
            installation: Some((*installation).to_string()),
            account: Some((*account).to_string()),
            world: (*world).to_string(),
            extra_segments: false,
        },
        [installation, account, world, ..] => WorldRef {
            installation: Some((*installation).to_string()),
            account: Some((*account).to_string()),
            world: (*world).to_string(),
            extra_segments: true,
        },
        [] => WorldRef {
            installation: None,
            account: None,
            world: String::new(),
            extra_segments: false,
        },
    }
}

/// Resolves a reference to exactly one world.
pub fn resolve(input: &str, worlds: &[World]) -> Result<World> {
    match as_path(input) {
        Ok(world) => return Ok(world),
        Err(err @ CoreError::UnreadableWorld { .. }) => return Err(err),
        Err(_) => {}
    }

    let r = parse(input);

    let matches_segments = |w: &World| {
        !r.extra_segments
            && r.installation.as_ref().is_none_or(|i| &w.installation == i)
            && r.account
                .as_ref()
                .is_none_or(|a| w.account.as_ref() == Some(a))
    };
    let tier = |field: fn(&World) -> &str| -> Vec<&World> {
        worlds
            .iter()
            .filter(|w| field(w) == input || (field(w) == r.world && matches_segments(w)))
            .collect()
    };

    let by_folder = tier(|w| &w.folder);
    let candidates = if by_folder.is_empty() {
        tier(|w| &w.display_name)
    } else {
        by_folder
    };

    let path_shaped = looks_like_path(input);

    match candidates.as_slice() {
        [one] => Ok((*one).clone()),
        [] if r.extra_segments || path_shaped => Err(CoreError::MalformedReference {
            reference: input.to_string(),
            looks_like_path: path_shaped,
        }),
        [] => Err(CoreError::WorldNotFound {
            reference: input.to_string(),
            near: match near_matches(input, worlds) {
                n if n.is_empty() => near_matches(&r.world, worlds),
                n => n,
            },
        }),
        many => Err(CoreError::AmbiguousWorld {
            reference: input.to_string(),
            candidates: many.iter().map(|w| w.qualified()).collect(),
        }),
    }
}

fn as_path(input: &str) -> Result<World> {
    let path = Path::new(input);

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CoreError::WorldNotFound {
                reference: input.to_string(),
                near: vec![],
            });
        }
        Err(e) => {
            return Err(CoreError::Io(e));
        }
    };

    if !metadata.is_dir() {
        return Err(CoreError::WorldNotFound {
            reference: input.to_string(),
            near: vec![],
        });
    }

    let level_dat_path = path.join("level.dat");

    match std::fs::File::open(&level_dat_path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CoreError::WorldNotFound {
                reference: input.to_string(),
                near: vec![],
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(CoreError::UnreadableWorld {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
        }
        Err(e) => {
            return Err(CoreError::UnreadableWorld {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
        }
    }

    let folder = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "world".to_string());
    let display_name = std::fs::read_to_string(path.join("levelname.txt"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| folder.clone());
    let (last_played, last_played_source) = match leveldat::read(&level_dat_path) {
        Ok(d) => match d.last_played() {
            Some(v) => (Some(v), LastPlayedSource::LevelDat),
            None => (None, LastPlayedSource::DirMtime),
        },
        Err(_) => (None, LastPlayedSource::DirMtime),
    };

    Ok(World {
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

fn near_matches(needle: &str, worlds: &[World]) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut matches: Vec<_> = worlds
        .iter()
        .filter(|w| {
            w.folder.to_lowercase().contains(&needle)
                || w.display_name.to_lowercase().contains(&needle)
        })
        .collect();

    matches.sort_by(|a, b| {
        let a_name = a.display_name.to_lowercase();
        let b_name = b.display_name.to_lowercase();
        let a_folder = a.folder.to_lowercase();
        let b_folder = b.folder.to_lowercase();

        let a_prefix = a_name.starts_with(&needle) || a_folder.starts_with(&needle);
        let b_prefix = b_name.starts_with(&needle) || b_folder.starts_with(&needle);

        match (b_prefix, a_prefix) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => match a_name.len().cmp(&b_name.len()) {
                std::cmp::Ordering::Equal => a_name.cmp(&b_name),
                ord => ord,
            },
        }
    });

    let out: Vec<String> = matches
        .iter()
        .take(5)
        .map(|w| format!("{} ({})", w.display_name, w.qualified()))
        .collect();
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
        assert!(!r.extra_segments);
    }

    #[test]
    fn parses_installation_and_world() {
        let r = parse("release/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account, None);
        assert_eq!(r.world, "Ssu8ww1SFbM=");
        assert!(!r.extra_segments);
    }

    #[test]
    fn parses_the_full_three_segment_form() {
        let r = parse("release/Shared/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account.as_deref(), Some("Shared"));
        assert_eq!(r.world, "Ssu8ww1SFbM=");
        assert!(!r.extra_segments);
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
    fn a_display_name_containing_slashes_resolves() {
        let worlds = vec![w(
            "release",
            Some("Shared"),
            "AdvancedAutomation-3-5-2024",
            "Advanced Automation 10/13/21 23:33:18",
        )];
        assert_eq!(
            resolve("Advanced Automation 10/13/21 23:33:18", &worlds)
                .unwrap()
                .folder,
            "AdvancedAutomation-3-5-2024"
        );
    }

    #[test]
    fn a_display_name_with_four_or_more_segments_resolves() {
        let worlds = vec![w("release", Some("Shared"), "abc=", "a/b/c/d/e")];
        assert_eq!(resolve("a/b/c/d/e", &worlds).unwrap().folder, "abc=");
    }

    #[test]
    fn a_folder_name_containing_slashes_resolves() {
        let worlds = vec![w("release", Some("Shared"), "odd/folder", "Some World")];
        assert_eq!(resolve("odd/folder", &worlds).unwrap().folder, "odd/folder");
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
            Err(CoreError::MalformedReference {
                looks_like_path: true,
                ..
            })
        ));
    }

    #[test]
    fn an_unreadable_level_dat_is_reported_as_unreadable_world() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("Amelix CMP");
            std::fs::create_dir_all(dir.join("db")).unwrap();
            std::fs::write(dir.join("level.dat"), b"x").unwrap();
            let level_dat = dir.join("level.dat");

            std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o000)).unwrap();

            if std::fs::read(&level_dat).is_ok() {
                std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o644)).ok();
                return;
            }

            let result = resolve(dir.to_str().unwrap(), &fixture());
            assert!(
                matches!(result, Err(CoreError::UnreadableWorld { .. })),
                "expected UnreadableWorld, got {result:?}"
            );

            std::fs::set_permissions(&level_dat, std::fs::Permissions::from_mode(0o644)).ok();
        }
        #[cfg(not(unix))]
        {}
    }

    #[test]
    fn a_reference_with_more_than_three_segments_is_rejected() {
        let err = resolve("a/b/c/d/e/f/g", &fixture()).unwrap_err();
        assert!(matches!(
            err,
            CoreError::MalformedReference {
                reference,
                looks_like_path: false
            } if reference == "a/b/c/d/e/f/g"
        ));
    }

    #[test]
    fn an_absolute_path_that_does_not_exist_reports_path_not_found() {
        let err = resolve("/nonexistent/world/path", &fixture()).unwrap_err();
        assert!(matches!(
            err,
            CoreError::MalformedReference {
                reference,
                looks_like_path: true
            } if reference == "/nonexistent/world/path"
        ));
    }

    #[test]
    fn a_windows_path_that_does_not_exist_reports_path_not_found() {
        for input in [
            r"C:\nonexistent\world\path",
            r"C:/nonexistent/world/path",
            r"\\server\share\world",
        ] {
            assert!(
                matches!(
                    resolve(input, &fixture()),
                    Err(CoreError::MalformedReference {
                        looks_like_path: true,
                        ..
                    })
                ),
                "{input} should read as a path"
            );
        }
    }

    #[test]
    fn a_name_carrying_a_colon_is_a_name_not_a_path() {
        for input in [
            "Advanced Automation 10/13/21 23:33:18",
            "23:33:18",
            "release/Shared/not:a:drive",
        ] {
            assert!(
                matches!(
                    resolve(input, &fixture()),
                    Err(CoreError::WorldNotFound { .. })
                ),
                "{input} should read as a name"
            );
        }
    }

    #[test]
    fn near_matches_are_sorted_by_prefix_then_length_then_alphabetical() {
        let worlds = vec![
            w("a", None, "test_a", "test alpha"),
            w("a", None, "test_b", "test beta"),
            w("a", None, "test_c", "testing"),
            w("a", None, "test_d", "te"),
            w("a", None, "test_e", "ten"),
            w("a", None, "test_f", "attest"),
        ];
        let near = near_matches("test", &worlds);
        assert_eq!(near.len(), 5);
        assert!(near[0].contains("test"));
        assert!(near[1].contains("test"));
        assert!(near[2].contains("test"));
    }

    #[test]
    fn world_not_found_near_contains_only_world_names() {
        let err = resolve("Amelx", &fixture()).unwrap_err();
        let CoreError::WorldNotFound { near, .. } = err else {
            panic!("expected WorldNotFound, got {err:?}");
        };
        for entry in &near {
            assert!(
                !entry.contains("Expected"),
                "near should not contain guidance text: {entry}"
            );
            assert!(
                !entry.contains("Path not found"),
                "near should not contain guidance text: {entry}"
            );
            assert!(
                entry.contains("(") && entry.contains(")"),
                "near entry should have format 'name (qualified)': {entry}"
            );
        }
    }
}
