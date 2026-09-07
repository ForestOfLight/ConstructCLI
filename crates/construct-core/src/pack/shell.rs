//! The per-world structures pack: a behaviour pack holding structures and
//! nothing else.
//!
//! A structure in the shared copy of Construct is seen by every world using it,
//! which makes "which worlds have which structures" unanswerable. Each world
//! gets one of these instead.
//!
//! It needs no code — the game registers structures from any enabled behaviour
//! pack — so a manifest, an icon, and a folder are the whole pack. Worlds whose
//! Construct copy is their own already have a per-world `structures/` and get
//! none.

use super::{Pack, manifest};
use crate::discovery::World;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// The shell pack's header UUID, fixed so the pack is found by UUID the way
/// Construct is, never by folder name. One UUID serves every world: two worlds
/// each holding a copy is no more a conflict than two worlds each holding
/// their own copy of Construct.
///
/// Both ids here are random v4, generated once and derived from nothing. A
/// derived value (a name hash, a zero UUID, a copied example) is the one way
/// this collides, and a collision is undetectable from inside one world.
pub const UUID: &str = "9f7d83af-309e-4997-840e-e9c350435e83";
pub const MODULE_UUID: &str = "bb8e1f57-5e69-42f6-b1e1-cd4a2a717c49";
pub const DIR_NAME: &str = "ConstructStructures";
pub const NAME: &str = "Construct Structures";

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

/// Creates the shell pack in `world`, copying its icon from `icon_from` — the
/// Construct pack it accompanies — so the two read as a pair in the game's
/// pack list.
///
/// Creating an existing pack is not an error: the manifest is rewritten from
/// the same constants and `structures/` is left alone. A half-created pack from
/// an interrupted run repairs itself on the next command.
pub fn create(world: &World, icon_from: Option<&Path>) -> Result<Pack> {
    let dir = dir(world);
    std::fs::create_dir_all(super::structures::dir(&dir))?;
    std::fs::write(dir.join("manifest.json"), manifest_json())?;

    if let Some(icon) = icon_from.map(|p| p.join("pack_icon.png"))
        && icon.is_file()
    {
        let _ = std::fs::copy(&icon, dir.join("pack_icon.png"));
    }

    let manifest = manifest::read(&dir)?;
    Ok(Pack { dir, manifest })
}
