//! Construct on disk: where its packs live and what is inside their `structures/`.

pub mod manifest;
pub mod structures;

use crate::discovery::{Installation, World};
use crate::error::{CoreError, Result};
use manifest::Manifest;
use std::path::{Path, PathBuf};

/// Construct's behaviour-pack header UUID. Packs are matched on this, never on
/// folder name — a renamed folder is still the same pack, and two folders
/// carrying this UUID are two copies of one pack.
pub const CONSTRUCT_BP_UUID: &str = "8c0c0153-d8b9-482a-889f-aef922b8fe58";
/// Construct's resource-pack header UUID.
pub const CONSTRUCT_RP_UUID: &str = "375ec465-3dc1-429f-8b4c-a337889e1ed4";

#[derive(Debug, Clone)]
pub struct Pack {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub fn behavior_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_behavior_packs")
}

pub fn resource_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_resource_packs")
}

/// A world's own copy of its packs, which takes precedence over the shared root.
pub fn world_behavior_root(world: &World) -> PathBuf {
    world.path.join("behavior_packs")
}

/// Every readable pack directly under `root`.
///
/// A directory with no manifest, or one that will not parse, is skipped: a
/// single corrupt pack must not hide every other pack on the machine. A
/// directory whose name begins with `.` is skipped outright, without even
/// trying to read a manifest from it: no Minecraft pack is named that way,
/// but `install::place` stages a pack under such a name while swapping it
/// in, and mid-swap (or after a crash, before the next run recovers or
/// abandons it) that staging directory can carry a fully valid manifest
/// with the same header UUID as the pack it is staging. Every caller of
/// `packs_in`/`find_by_uuid` — this module's own `for_world`,
/// `for_installation`, and `install::place` alike — must never mistake it
/// for the installed copy.
pub fn packs_in(root: &Path) -> Vec<Pack> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Pack> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let dir = e.path();
            manifest::read(&dir)
                .ok()
                .map(|manifest| Pack { dir, manifest })
        })
        .collect();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

pub fn find_by_uuid(root: &Path, uuid: &str) -> Option<Pack> {
    packs_in(root).into_iter().find(|p| p.manifest.uuid == uuid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The world's own `behavior_packs/` copy.
    WorldLocal,
    /// The installation's shared `development_behavior_packs` copy.
    Shared,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub pack: Pack,
    pub scope: Scope,
    /// The copy that was found and *not* used, if there was one. §11 asks the
    /// command to state which of two copies it chose.
    pub also_at: Option<PathBuf>,
}

/// The Construct copy that governs a world: its own, else the installation's.
pub fn for_world(world: &World, installation: &Installation) -> Result<Target> {
    let local_root = world_behavior_root(world);
    let shared_root = behavior_root(&installation.dev_pack_root);
    let local = find_by_uuid(&local_root, CONSTRUCT_BP_UUID);
    let shared = find_by_uuid(&shared_root, CONSTRUCT_BP_UUID);

    match (local, shared) {
        (Some(pack), other) => Ok(Target {
            pack,
            scope: Scope::WorldLocal,
            also_at: other.map(|p| p.dir),
        }),
        (None, Some(pack)) => Ok(Target {
            pack,
            scope: Scope::Shared,
            also_at: None,
        }),
        (None, None) => Err(CoreError::ConstructNotInstalled {
            searched: vec![local_root, shared_root],
        }),
    }
}

/// The Construct copy in an installation's shared root.
pub fn for_installation(installation: &Installation) -> Result<Target> {
    let root = behavior_root(&installation.dev_pack_root);
    find_by_uuid(&root, CONSTRUCT_BP_UUID)
        .map(|pack| Target {
            pack,
            scope: Scope::Shared,
            also_at: None,
        })
        .ok_or(CoreError::ConstructNotInstalled {
            searched: vec![root],
        })
}
