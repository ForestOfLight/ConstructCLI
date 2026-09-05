//! The per-world structures pack: a behaviour pack holding structures and
//! nothing else.
//!
//! A structure in the shared `development_behavior_packs` copy of Construct is
//! shared by every world that copy serves, which makes "which worlds have
//! which structures" unanswerable per world. This pack is the fix: each world
//! gets its own, so a structure written for a world belongs to that world.
//!
//! It needs no code. The game registers structures from *any* enabled
//! behaviour pack — a file directly in `structures/` becomes
//! `mystructure:<stem>`, which is the namespace Construct's in-game list
//! shows — so a manifest, an icon, and a folder are the whole pack.
//!
//! Worlds whose Construct copy is their own already have a per-world
//! `structures/`, and get no shell pack: a second folder there would divide
//! one world's structures across two packs for no gain.

use super::{Pack, manifest};
use crate::discovery::World;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// The shell pack's header UUID. Fixed, so the pack is found by UUID the way
/// Construct is and never by folder name. One UUID serves every world: two
/// worlds each holding a copy is no more a conflict than two worlds each
/// holding their own copy of Construct.
///
/// Both ids here are random v4, generated once with `uuidgen` and never
/// derived from anything — a value another pack could arrive at
/// independently (a name hash, a zero UUID, a copied example) is the one way
/// this collides, and a collision is not a thing this tool can detect from
/// inside one world.
pub const UUID: &str = "9f7d83af-309e-4997-840e-e9c350435e83";
/// The data module's UUID, distinct from the header's as Bedrock requires.
pub const MODULE_UUID: &str = "bb8e1f57-5e69-42f6-b1e1-cd4a2a717c49";
/// The folder name inside `<world>/behavior_packs/`.
pub const DIR_NAME: &str = "ConstructStructures";
/// What the pack calls itself in the world's pack list.
pub const NAME: &str = "Construct Structures";

/// `min_engine_version` is deliberately low: the pack has no scripts and no
/// API surface to be incompatible with, and a floor *above* the running engine
/// is the only value the game rejects.
fn manifest_json() -> String {
    format!(
        r#"{{
  "format_version": 2,
  "header": {{
    "name": "{NAME}",
    "description": "Structures for this world, managed by ConstructCLI.",
    "uuid": "{UUID}",
    "min_engine_version": [1, 21, 0],
    "version": [1, 0, 0]
  }},
  "modules": [
    {{
      "description": "Structures",
      "type": "data",
      "uuid": "{MODULE_UUID}",
      "version": [1, 0, 0]
    }}
  ]
}}
"#
    )
}

/// Where the shell pack lives for `world`, whether or not it exists.
pub fn dir(world: &World) -> PathBuf {
    super::world_behavior_root(world).join(DIR_NAME)
}

/// Creates the shell pack in `world`, copying the icon from `icon_from` — the
/// Construct pack this one accompanies — so the two read as a pair in the
/// game's pack list.
///
/// Creating an existing pack is not an error: the manifest is rewritten from
/// the same constants it was written from, and `structures/` is left alone. A
/// half-created pack from an interrupted run therefore repairs itself on the
/// next command rather than needing a reinstall.
pub fn create(world: &World, icon_from: Option<&Path>) -> Result<Pack> {
    let dir = dir(world);
    std::fs::create_dir_all(super::structures::dir(&dir))?;
    std::fs::write(dir.join("manifest.json"), manifest_json())?;

    // A missing icon is cosmetic — the game shows a placeholder — so a pack
    // without one is still a working pack and this never fails the command.
    if let Some(icon) = icon_from.map(|p| p.join("pack_icon.png"))
        && icon.is_file()
    {
        let _ = std::fs::copy(&icon, dir.join("pack_icon.png"));
    }

    let manifest = manifest::read(&dir)?;
    Ok(Pack { dir, manifest })
}
