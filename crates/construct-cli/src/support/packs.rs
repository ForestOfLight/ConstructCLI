use crate::output::Out;
use crate::support::phrasing::pack_phrase;
use construct_core::Result;
use construct_core::discovery::{Installation, World};
use construct_core::inuse;
use construct_core::pack;
use construct_core::worldpacks;
use std::path::Path;

pub fn refuse_if_in_use(world: &World, at_risk: inuse::AtRisk, out: &mut Out) -> Result<()> {
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

pub fn home_for_write(
    world: &World,
    installation: &Installation,
    out: &mut Out,
) -> Result<pack::Home> {
    if let Some(home) = pack::home(world) {
        return Ok(home);
    }
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
