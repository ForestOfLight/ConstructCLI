//! Where Minecraft keeps worlds and development packs, per platform.
//!
//! Windows moved from UWP to GDK in Minecraft 1.21.120. Under GDK, worlds live
//! per Xbox account while development packs live in `Users\Shared` — so worlds
//! and dev packs sit in *different* roots and there may be several world roots
//! on one machine. Legacy UWP is still probed for worlds.

use std::path::{Path, PathBuf};

/// A place worth probing, before existence is checked.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub name: String,
    pub dev_pack_root: PathBuf,
    /// Directories that may each contain a `minecraftWorlds`. More than one
    /// means the installation is per-account.
    pub world_root_parents: Vec<PathBuf>,
    /// True when `world_root_parents` was produced by expanding accounts, so a
    /// single surviving root still deserves an account label.
    pub per_account: bool,
}

/// One world root: a `minecraftWorlds` directory, optionally owned by an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRoot {
    pub account: Option<String>,
    pub path: PathBuf,
}

/// A resolved Minecraft installation that exists on disk.
#[derive(Debug, Clone)]
pub struct Installation {
    pub name: String,
    pub dev_pack_root: PathBuf,
    pub world_roots: Vec<WorldRoot>,
}

/// Every location worth probing on this platform.
///
/// Base directories are parameters rather than environment reads so that tests
/// can point the whole table at a temp directory.
pub fn candidates(
    home: &Path,
    appdata: Option<&Path>,
    localappdata: Option<&Path>,
) -> Vec<Candidate> {
    let mut out = Vec::new();

    // Windows GDK: release and preview.
    if let Some(appdata) = appdata {
        for (name, product) in [
            ("release", "Minecraft Bedrock"),
            ("preview", "Minecraft Bedrock Preview"),
        ] {
            let users = appdata.join(product).join("Users");
            out.push(Candidate {
                name: name.to_string(),
                dev_pack_root: users.join("Shared/games/com.mojang"),
                world_root_parents: account_dirs(&users),
                per_account: true,
            });
        }
    }

    // Windows legacy UWP.
    if let Some(local) = localappdata {
        let base =
            local.join("Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang");
        out.push(Candidate {
            name: "legacy".to_string(),
            dev_pack_root: base.clone(),
            world_root_parents: vec![base],
            per_account: false,
        });
    }

    // mcpelauncher: macOS then Linux. Both may be probed; only one will exist.
    for rel in [
        "Library/Application Support/mcpelauncher/games/com.mojang",
        ".local/share/mcpelauncher/games/com.mojang",
    ] {
        let base = home.join(rel);
        out.push(Candidate {
            name: "mcpelauncher".to_string(),
            dev_pack_root: base.clone(),
            world_root_parents: vec![base],
            per_account: false,
        });
    }

    out
}

/// Under GDK each Xbox account gets its own directory beside `Shared`.
fn account_dirs(users: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(users) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path().join("games/com.mojang"))
        .collect();
    dirs.sort();
    dirs
}

/// Keeps the candidates that exist on disk. A missing root is absent, not an error.
pub fn resolve(candidates: Vec<Candidate>) -> Vec<Installation> {
    let mut out = Vec::new();

    for c in candidates {
        let world_roots: Vec<WorldRoot> = c
            .world_root_parents
            .iter()
            .map(|p| p.join("minecraftWorlds"))
            .filter(|p| p.is_dir())
            .map(|path| {
                let account = c.per_account.then(|| account_label(&path)).flatten();
                WorldRoot { account, path }
            })
            .collect();

        // An installation is real if either half of it exists: dev packs without
        // worlds is a valid deployment target, and worlds without dev packs is
        // exactly the legacy UWP case.
        if c.dev_pack_root.is_dir() || !world_roots.is_empty() {
            out.push(Installation {
                name: c.name,
                dev_pack_root: c.dev_pack_root,
                world_roots,
            });
        }
    }

    dedupe_names(out)
}

/// Makes installation names unique by appending `-2`, `-3`, … to later
/// occurrences of a name already seen, in candidate order. Deterministic and
/// stable across repeated calls given the same input order.
///
/// Two candidates can legitimately resolve to the same name (e.g. the macOS
/// and Linux mcpelauncher probes are both named `mcpelauncher`, and only one
/// normally exists — but both can exist, e.g. under Wine or a shared home
/// directory). Downstream, `discovery/reference.rs` matches installations by
/// name, so duplicate names make qualified references ambiguous; this keeps
/// both installations (neither is dropped or merged) while giving each a
/// distinct, reproducible name.
fn dedupe_names(installations: Vec<Installation>) -> Vec<Installation> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    installations
        .into_iter()
        .map(|mut inst| {
            let count = seen.entry(inst.name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                inst.name = format!("{}-{}", inst.name, count);
            }
            inst
        })
        .collect()
}

/// The account directory name, e.g. `Shared` or `2533274801234567`, taken from
/// `<account>/games/com.mojang/minecraftWorlds`.
fn account_label(world_root: &Path) -> Option<String> {
    world_root
        .parent()? // games/com.mojang
        .parent()? // games
        .parent()? // <account>
        .file_name()?
        .to_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn touch_world(root: &Path, folder: &str, name: &str) {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("level.dat"), b"stub").unwrap();
        fs::write(dir.join("levelname.txt"), name).unwrap();
    }

    #[test]
    fn gdk_release_has_shared_dev_packs_and_per_account_world_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("AppData/Roaming");
        let base = appdata.join("Minecraft Bedrock/Users");
        fs::create_dir_all(base.join("Shared/games/com.mojang/development_behavior_packs"))
            .unwrap();
        touch_world(
            &base.join("Shared/games/com.mojang/minecraftWorlds"),
            "AAA=",
            "Shared World",
        );
        touch_world(
            &base.join("2533274801234567/games/com.mojang/minecraftWorlds"),
            "BBB=",
            "My World",
        );

        let found = resolve(candidates(tmp.path(), Some(&appdata), None));
        let release = found
            .iter()
            .find(|i| i.name == "release")
            .expect("release installation");
        assert!(
            release
                .dev_pack_root
                .ends_with("Users/Shared/games/com.mojang")
        );
        assert_eq!(release.world_roots.len(), 2, "one world root per account");
        // With several roots the account segment must be present, or qualified
        // references cannot address them.
        assert!(release.world_roots.iter().all(|r| r.account.is_some()));
    }

    #[test]
    fn a_single_world_root_carries_no_account_segment() {
        // macOS and Linux must never display an account segment.
        let tmp = tempfile::tempdir().unwrap();
        let com_mojang = tmp
            .path()
            .join("Library/Application Support/mcpelauncher/games/com.mojang");
        fs::create_dir_all(&com_mojang).unwrap();
        touch_world(
            &com_mojang.join("minecraftWorlds"),
            "Ssu8ww1SFbM=",
            "construct show",
        );

        let found = resolve(candidates(tmp.path(), None, None));
        let mcpe = found
            .iter()
            .find(|i| i.name == "mcpelauncher")
            .expect("mcpelauncher");
        assert_eq!(mcpe.world_roots.len(), 1);
        assert_eq!(mcpe.world_roots[0].account, None);
    }

    #[test]
    fn absent_roots_are_omitted_without_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve(candidates(tmp.path(), None, None)).is_empty());
    }

    #[test]
    fn preview_and_release_are_separate_installations() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("AppData/Roaming");
        for product in ["Minecraft Bedrock", "Minecraft Bedrock Preview"] {
            let base = appdata.join(product).join("Users/Shared/games/com.mojang");
            fs::create_dir_all(base.join("development_behavior_packs")).unwrap();
            touch_world(&base.join("minecraftWorlds"), "AAA=", "W");
        }
        let names: Vec<_> = resolve(candidates(tmp.path(), Some(&appdata), None))
            .into_iter()
            .map(|i| i.name)
            .collect();
        assert!(names.contains(&"release".to_string()));
        assert!(names.contains(&"preview".to_string()));
    }

    #[test]
    fn legacy_uwp_is_probed_for_worlds() {
        // Pre-migration worlds are exactly the ones worth mining.
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("AppData/Local");
        let base =
            local.join("Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang");
        fs::create_dir_all(&base).unwrap();
        touch_world(&base.join("minecraftWorlds"), "OLD=", "Old World");

        let found = resolve(candidates(tmp.path(), None, Some(&local)));
        assert!(found.iter().any(|i| i.name == "legacy"));
    }

    #[test]
    fn same_named_candidates_get_distinct_stable_names() {
        // e.g. both the macOS and Linux mcpelauncher probes exist on one machine.
        let tmp = tempfile::tempdir().unwrap();
        let base_a = tmp.path().join("a/com.mojang");
        let base_b = tmp.path().join("b/com.mojang");
        fs::create_dir_all(base_a.join("development_behavior_packs")).unwrap();
        fs::create_dir_all(base_b.join("development_behavior_packs")).unwrap();

        let make_candidates = || {
            vec![
                Candidate {
                    name: "mcpelauncher".to_string(),
                    dev_pack_root: base_a.clone(),
                    world_root_parents: vec![base_a.clone()],
                    per_account: false,
                },
                Candidate {
                    name: "mcpelauncher".to_string(),
                    dev_pack_root: base_b.clone(),
                    world_root_parents: vec![base_b.clone()],
                    per_account: false,
                },
            ]
        };

        let names = |installs: &[Installation]| -> Vec<String> {
            installs.iter().map(|i| i.name.clone()).collect()
        };

        let first = resolve(make_candidates());
        assert_eq!(names(&first), vec!["mcpelauncher", "mcpelauncher-2"]);
        assert_eq!(first.len(), 2, "neither installation is dropped");

        // Stable across repeated calls given the same input order.
        let second = resolve(make_candidates());
        assert_eq!(names(&second), names(&first));
    }

    #[test]
    fn a_root_without_a_minecraft_worlds_dir_still_counts_for_dev_packs() {
        let tmp = tempfile::tempdir().unwrap();
        let com_mojang = tmp
            .path()
            .join(".local/share/mcpelauncher/games/com.mojang");
        fs::create_dir_all(com_mojang.join("development_behavior_packs")).unwrap();
        let found = resolve(candidates(tmp.path(), None, None));
        let mcpe = found
            .iter()
            .find(|i| i.name == "mcpelauncher")
            .expect("mcpelauncher");
        assert!(mcpe.world_roots.is_empty());
    }
}
