//! One command per module, plus the shared wording and pack resolution they
//! hand the user.

use crate::output::Out;
use construct_core::Result;
use construct_core::discovery::{Installation, World};
use construct_core::inuse;
use construct_core::pack;
use construct_core::worldpacks;
use std::path::Path;

pub mod catalog;
pub mod add;
pub mod completions;
pub mod copy;
pub mod delete;
pub mod enable_beta_apis;
pub mod export;
pub mod import;
pub mod install;
pub mod status;
pub mod structures;
pub mod worlds;

/// Names the pack a command touched, and what that means for reach.
///
/// The full path a command prints technically answers this — it either holds
/// `development_behavior_packs` or sits inside the world folder — but it
/// answers it forty characters in, and this is the distinction that decides
/// whether a structure shows up in one world or in every world using the
/// shared copy. So it is stated in words, and in terms of that consequence.
///
/// Every phrase here names a *copy of Construct* (or the world's structures
/// pack beside it) and prefixes it with its scope: the shared copy, or the
/// world that owns it. `pack_short` shortens the same names without changing
/// that vocabulary.
///
/// The caller supplies the preposition: `import` and `copy` write *into* this
/// pack, `delete` removes *from* it. The consequence clause reads correctly
/// either way — a structure added to a world's own pack appears in that world
/// only, and one deleted from it disappears from that world only.
/// Refuses a world Minecraft appears to have open, saying so if it has to wait.
///
/// `inuse`'s second phase watches `db/` for up to `CONFIRM_WATCH` seconds to
/// tell a live world from a command that just finished writing one. That wait
/// is silent from the outside and long enough to read as a hang, so this
/// announces it — but only when it is actually going to happen, which is the
/// reason phase one is checked here as well as inside `refuse_if_in_use`. The
/// common case is quiet, instant, and prints nothing.
pub fn refuse_if_in_use(world: &World, at_risk: inuse::AtRisk, out: &mut Out) -> Result<()> {
    // Mirrors the phases inside `refuse_if_in_use`, so the notice appears only
    // when a watch is genuinely about to happen. A recent write this tool made
    // itself is answered from the mark instantly and must not announce a wait
    // it is not going to take.
    if inuse::looks_in_use(&world.db_path()) && !construct_core::writemark::left_by_us(world) {
        out.line(format!(
            "{} was written in the last {}s and not by us; watching up to {}s to tell \
             a live world from a command that just finished\u{2026}",
            world.db_path().display(),
            inuse::ACTIVITY_WINDOW.as_secs(),
            inuse::CONFIRM_WATCH.as_secs(),
        ));
    }
    inuse::refuse_if_in_use(world, at_risk)
}

pub fn pack_phrase(kind: pack::HomeKind, world: Option<&str>) -> String {
    match (kind, world) {
        (pack::HomeKind::WorldConstruct, Some(w)) => {
            format!("{w}'s own copy of Construct — that world only")
        }
        (pack::HomeKind::WorldConstruct, None) => {
            "the world's own copy of Construct — that world only".to_string()
        }
        (pack::HomeKind::WorldStructuresPack, Some(w)) => {
            format!("{w}'s structures pack — that world only")
        }
        (pack::HomeKind::WorldStructuresPack, None) => {
            "the world's structures pack — that world only".to_string()
        }
        (pack::HomeKind::SharedConstruct, _) => {
            "the shared copy of Construct in development_behavior_packs — every world using it"
                .to_string()
        }
    }
}

/// The same pack, named in a list where the reach clause would repeat on
/// every line — `status`'s cross-world view, where the shared row states it
/// once and the rest are per-world by construction.
pub fn pack_short(kind: pack::HomeKind, world: &str) -> String {
    match kind {
        pack::HomeKind::WorldConstruct => format!("{world}'s own copy of Construct"),
        pack::HomeKind::WorldStructuresPack => format!("{world}'s structures pack"),
        pack::HomeKind::SharedConstruct => "the shared copy of Construct".to_string(),
    }
}

/// The `target` field `import` and `copy` carry in JSON: which pack took the
/// writes, spelled as the `--source` value that reads it back.
pub fn target_field(kind: pack::HomeKind) -> &'static str {
    kind.source().as_str()
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
        kind: pack::HomeKind::WorldStructuresPack,
    })
}

/// Warns when `world` already sees this name in a pack other than the one
/// being written to.
///
/// Not a refusal: the file-level collision rule still refuses a write onto an
/// existing file, and these two are different files in different packs. But
/// the game loads both packs for this world and logs a conflict when two carry
/// one name, resolving it in a way this tool cannot predict — and a user with
/// structures already in the shared copy of Construct will hit exactly this while
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
