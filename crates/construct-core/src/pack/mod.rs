//! Construct on disk: where its packs live and what is inside their `structures/`.

pub mod manifest;
pub mod shell;
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

pub fn shared_behavior_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_behavior_packs")
}

pub fn shared_resource_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_resource_packs")
}

/// The non-development sibling of [`shared_behavior_root`], where a hand-installed
/// Construct ends up when it is dropped into the wrong folder. Nothing this
/// tool writes belongs here; `install::adopt` is the one thing that reads it,
/// to move a misplaced copy out of it.
pub fn stray_behavior_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("behavior_packs")
}

/// The non-development sibling of [`shared_resource_root`]. See
/// [`stray_behavior_root`].
pub fn stray_resource_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("resource_packs")
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
    /// The world's own `behavior_packs/` copy — that world only.
    World,
    /// The shared `development_behavior_packs` copy — every world using it.
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
    let world_root = world_behavior_root(world);
    let shared_root = shared_behavior_root(&installation.dev_pack_root);
    let world_copy = find_by_uuid(&world_root, CONSTRUCT_BP_UUID);
    let shared_copy = find_by_uuid(&shared_root, CONSTRUCT_BP_UUID);

    match (world_copy, shared_copy) {
        (Some(pack), other) => Ok(Target {
            pack,
            scope: Scope::World,
            also_at: other.map(|p| p.dir),
        }),
        (None, Some(pack)) => Ok(Target {
            pack,
            scope: Scope::Shared,
            also_at: None,
        }),
        (None, None) => Err(CoreError::ConstructNotInstalled {
            searched: vec![world_root, shared_root],
        }),
    }
}

/// A pack whose `structures/` folder this tool reads or writes for one world.
#[derive(Debug, Clone)]
pub struct Home {
    pub dir: PathBuf,
    pub kind: HomeKind,
}

/// Which of the three places a world's structures can sit in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeKind {
    /// The world's own copy of Construct.
    WorldConstruct,
    /// The world's structures shell pack (`shell::DIR_NAME`).
    WorldStructuresPack,
    /// The shared copy of Construct, which serves every world that has no
    /// copy of its own.
    SharedConstruct,
}

impl HomeKind {
    /// Which `--source` this home's structures answer to: a pack serving one
    /// world, or the shared copy serving every world using it. This is the
    /// distinction commands report.
    pub fn source(self) -> crate::catalog::Source {
        match self {
            HomeKind::WorldConstruct | HomeKind::WorldStructuresPack => {
                crate::catalog::Source::WorldPack
            }
            HomeKind::SharedConstruct => crate::catalog::Source::SharedPack,
        }
    }
}

/// Where this tool writes structures for `world`, or `None` when the world has
/// nowhere yet and a shell pack has to be created first.
///
/// A world's own copy of Construct already has a per-world `structures/`, so
/// it is the home when present; splitting one world's structures across it and
/// a shell pack beside it would divide them for no gain. The shared copy is
/// never a home: writing there would put the structure in every world.
pub fn home(world: &World) -> Option<Home> {
    let world_root = world_behavior_root(world);
    if let Some(pack) = find_by_uuid(&world_root, CONSTRUCT_BP_UUID) {
        return Some(Home {
            dir: pack.dir,
            kind: HomeKind::WorldConstruct,
        });
    }
    find_by_uuid(&world_root, shell::UUID).map(|pack| Home {
        dir: pack.dir,
        kind: HomeKind::WorldStructuresPack,
    })
}

/// Every pack whose `structures/` the game loads for `world`, home first.
///
/// Wider than [`home`] on purpose: reads report what the world actually has,
/// and a world running the shared copy of Construct really does see that pack's
/// structures. Empty only when Construct is installed nowhere this world can
/// reach — the caller turns that into [`CoreError::ConstructNotInstalled`],
/// since an empty list and "no Construct at all" are different answers.
///
/// The two Construct copies never both appear: they carry the same header
/// UUID, so a world holding its own copy loads that one and never the shared
/// one. The shell pack has a UUID of its own and so is always additive.
pub fn serving(world: &World, installation: &Installation) -> Vec<Home> {
    let world_root = world_behavior_root(world);
    let mut out = Vec::new();

    match find_by_uuid(&world_root, CONSTRUCT_BP_UUID) {
        Some(pack) => out.push(Home {
            dir: pack.dir,
            kind: HomeKind::WorldConstruct,
        }),
        None => {
            if let Some(pack) = find_by_uuid(
                &shared_behavior_root(&installation.dev_pack_root),
                CONSTRUCT_BP_UUID,
            ) {
                out.push(Home {
                    dir: pack.dir,
                    kind: HomeKind::SharedConstruct,
                });
            }
        }
    }
    if let Some(pack) = find_by_uuid(&world_root, shell::UUID) {
        out.push(Home {
            dir: pack.dir,
            kind: HomeKind::WorldStructuresPack,
        });
    }
    out
}

/// The roots [`serving`] looked in, for the error when it found nothing.
pub fn searched_roots(world: &World, installation: &Installation) -> Vec<PathBuf> {
    vec![
        world_behavior_root(world),
        shared_behavior_root(&installation.dev_pack_root),
    ]
}

/// The shared copy of Construct, in the installation's `development_behavior_packs`.
pub fn for_installation(installation: &Installation) -> Result<Target> {
    let root = shared_behavior_root(&installation.dev_pack_root);
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
