//! One command per module, plus the shared wording and pack resolution they
//! hand the user.

use crate::output::Out;
use construct_core::Result;
use construct_core::discovery::{Installation, World};
use construct_core::pack;
use construct_core::worldpacks;
use std::path::Path;

pub mod catalog;
pub mod copy;
pub mod delete;
pub mod add;
pub mod experiment;
pub mod export;
pub mod import;
pub mod install;
pub mod list;
pub mod status;
pub mod worlds;

/// Names the pack a command touched, and what that means for reach.
///
/// The full path a command prints technically answers this — it either holds
/// `development_behavior_packs` or sits inside the world folder — but it
/// answers it forty characters in, and this is the distinction that decides
/// whether a structure shows up in one world or in every world using the
/// shared pack. So it is stated in words, and in terms of that consequence.
///
/// The caller supplies the preposition: `import` and `copy` write *into* this
/// pack, `delete` removes *from* it. The consequence clause reads correctly
/// either way — a structure added to a world's own pack appears in that world
/// only, and one deleted from it disappears from that world only.
pub fn pack_phrase(kind: pack::HomeKind, world: Option<&str>) -> String {
    match (kind, world) {
        (pack::HomeKind::ConstructInWorld, Some(w)) => {
            format!("{w}'s own copy of Construct — that world only")
        }
        (pack::HomeKind::ConstructInWorld, None) => {
            "the world's own copy of Construct — that world only".to_string()
        }
        (pack::HomeKind::StructuresPack, Some(w)) => {
            format!("{w}'s structures pack — that world only")
        }
        (pack::HomeKind::StructuresPack, None) => {
            "the world's structures pack — that world only".to_string()
        }
        (pack::HomeKind::SharedConstruct, _) => {
            "the Construct in development_behavior_packs — shared by every world".to_string()
        }
    }
}

/// The same pack, named in a list where the reach clause would repeat on
/// every line — `status`'s cross-world view, where the shared row states it
/// once and the rest are per-world by construction.
pub fn pack_short(kind: pack::HomeKind, world: &str) -> String {
    match kind {
        pack::HomeKind::ConstructInWorld => format!("{world}'s own copy of Construct"),
        pack::HomeKind::StructuresPack => format!("{world}'s structures pack"),
        pack::HomeKind::SharedConstruct => "the shared pack".to_string(),
    }
}

/// The `scope` field `import`, `copy`, and `delete` all carry in JSON.
pub fn scope_field(kind: pack::HomeKind) -> &'static str {
    match kind.scope() {
        pack::Scope::WorldLocal => "world",
        pack::Scope::Shared => "shared",
    }
}

/// The pack a write for `world` belongs in, creating the world's structures
/// pack when it has no home yet.
///
/// Creating one here rather than only in `install --world` is what keeps a
/// world installed before structures packs existed working: it acquires one
/// the first time something is written for it, instead of failing with advice
/// to reinstall.
pub fn home_for_write(
    world: &World,
    installation: &Installation,
    out: &mut Out,
) -> Result<pack::Home> {
    if let Some(home) = pack::home(world) {
        return Ok(home);
    }
    // A shell pack accompanies a Construct install; with Construct nowhere in
    // reach there is nothing to accompany, and this is the error that says so.
    let construct = pack::for_world(world, installation)?;
    let created = create_structures_pack(world, &construct.pack.dir)?;
    out.line(format!(
        "created {} and enabled it in {}",
        created.dir.display(),
        world.display_name
    ));
    Ok(pack::Home {
        dir: created.dir,
        kind: pack::HomeKind::StructuresPack,
    })
}

/// Warns when `world` already sees this name in a pack other than the one
/// being written to.
///
/// Not a refusal: the file-level collision rule still refuses a write onto an
/// existing file, and these two are different files in different packs. But
/// the game loads both packs for this world and logs a conflict when two carry
/// one name, resolving it in a way this tool cannot predict — and a user with
/// structures already in the shared Construct will hit exactly this while
/// giving a world copies of its own, so it has to be said rather than
/// discovered in-game.
pub fn warn_if_another_pack_has_it(
    world: &World,
    installation: &Installation,
    writing_to: &Path,
    id: &str,
    out: &mut Out,
) {
    for home in pack::serving(world, installation) {
        if home.dir == writing_to {
            continue;
        }
        let Ok(other) = pack::structures::path_for(&home.dir, id) else {
            continue;
        };
        if other.is_file() {
            out.warn(format!(
                "{} already has a structure named {id} in {}; the game loads both packs for \
                 this world and will log a conflict between them",
                world.display_name,
                pack_phrase(home.kind, Some(world.display_name.as_str()))
            ));
        }
    }
}

/// Writes the structures pack into `world` and enables it, so the game loads
/// the structures written there. Shared by `install --world` and the
/// on-demand path above.
pub fn create_structures_pack(world: &World, icon_from: &Path) -> Result<pack::Pack> {
    let created = pack::shell::create(world, Some(icon_from))?;
    worldpacks::upsert(
        &worldpacks::behavior_path(world),
        worldpacks::PackRef {
            pack_id: pack::shell::UUID.to_string(),
            version: created.manifest.version,
        },
    )?;
    Ok(created)
}
