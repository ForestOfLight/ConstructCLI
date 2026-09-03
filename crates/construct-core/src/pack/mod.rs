//! Construct on disk: where its packs live and what is inside their `structures/`.

pub mod manifest;
pub mod structures;

use crate::discovery::World;
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
/// single corrupt pack must not hide every other pack on the machine.
pub fn packs_in(root: &Path) -> Vec<Pack> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Pack> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
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
