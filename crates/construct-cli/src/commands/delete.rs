use crate::cli::DeleteArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::catalog as loader;
use crate::support::packs::refuse_if_in_use;
use crate::support::phrasing::pack_phrase;
use crate::support::usage::check_source_against_world;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::inuse;
use construct_core::pack::{self, structures};
use construct_core::store::bedrock::BedrockStore;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    world: Option<String>,
    deleted: Vec<Deleted>,
}

#[derive(Serialize)]
struct Deleted {
    name: String,
    id: String,
    source: &'static str,
    path: Option<String>,
}

pub fn dispatch(args: &DeleteArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    check_source_against_world(
        "delete <structure>",
        "delete from",
        args.world.as_ref(),
        args.source,
        true,
    )?;
    let opts = Options {
        structures: &args.structures,
        source: args.source.map(Into::into),
    };
    match &args.world {
        Some(reference) => {
            let world = ctx.world(reference)?;
            Ok(for_world(&world, &ctx.installations, &opts, out)?)
        }
        None => Ok(shared(ctx.installation()?, &opts, out)?),
    }
}

pub struct Options<'a> {
    pub structures: &'a [String],
    pub source: Option<Source>,
}

fn shared(installation: &Installation, opts: &Options, out: &mut Out) -> Result<()> {
    let pack = pack::for_installation(installation)?.pack;
    let entries = catalog::from_pack(&pack.dir, Source::SharedPack);

    let plan = resolve(opts.structures, &entries, opts.source, None)?;
    let mut deleted = Vec::new();
    for entry in &plan {
        let path = unlink(entry)?;
        out.line(format!("deleted {}", entry.name));
        out.line(format!(
            "  from {}",
            pack_phrase(pack::HomeKind::SharedConstruct, None)
        ));
        out.line(format!("  {}", path.display()));
        deleted.push(row(entry, Some(path.display().to_string())));
    }

    out.warn(
        "removed from the shared copy of Construct; every world using it loses these \
         structures",
    );
    out.line("Reload affected worlds before Construct stops showing them.");
    out.emit(Payload {
        world: None,
        deleted,
    });
    Ok(())
}

fn for_world(
    world: &World,
    installations: &[Installation],
    opts: &Options,
    out: &mut Out,
) -> Result<()> {
    let live = if opts.source.is_some_and(|s| s.is_pack()) {
        None
    } else {
        refuse_if_in_use(world, inuse::AtRisk::Database, out)?;
        Some(BedrockStore::open_live(world)?)
    };

    let world_entries = match &live {
        Some(store) => catalog::from_world(store)?,
        None => Vec::new(),
    };
    let (all_pack_entries, packs) =
        loader::packs_for_world(world, installations, opts.source, out)?;

    let pack_entries = loader::world_scoped(all_pack_entries);

    let entries = catalog::unify(world_entries, pack_entries);
    let plan = resolve(opts.structures, &entries, opts.source, Some(world))?;

    let mut deleted = Vec::new();
    for entry in plan.iter().filter(|e| e.source == Source::WorldDb) {
        let store = live
            .as_ref()
            .ok_or_else(|| internal(format!("world entry {} with no open database", entry.id)))?;
        if !store.remove(&entry.id)? {
            return Err(internal(format!(
                "{} was listed in the database but not there to remove",
                entry.id
            )));
        }
        out.line(format!("deleted {}", entry.name));
        out.line("  from this world's database");
        deleted.push(row(entry, None));
    }

    let wrote_to_db = !deleted.is_empty();
    drop(live);

    if wrote_to_db {
        construct_core::writemark::record(world);
    }

    for entry in plan.iter().filter(|e| e.source.is_pack()) {
        let path = unlink(entry)?;
        out.line(format!("deleted {}", entry.name));
        if let Some(home) = packs.iter().find(|h| path.starts_with(&h.dir)) {
            out.line(format!(
                "  from {}",
                pack_phrase(home.kind, Some(world.display_name.as_str()))
            ));
        }
        out.line(format!("  {}", path.display()));
        deleted.push(row(entry, Some(path.display().to_string())));
    }

    out.line("Reload the world before Construct stops showing it.");
    out.emit(Payload {
        world: Some(world.qualified()),
        deleted,
    });
    Ok(())
}

fn resolve(
    names: &[String],
    entries: &[Entry],
    source: Option<Source>,
    world: Option<&World>,
) -> Result<Vec<Entry>> {
    let mut plan = Vec::new();
    for name in names {
        plan.extend(
            catalog::resolve_all(name, entries, source)
                .map_err(|e| loader::explain_miss(e, world))?,
        );
    }
    Ok(plan)
}

fn unlink(entry: &Entry) -> Result<std::path::PathBuf> {
    let path = entry
        .path
        .clone()
        .ok_or_else(|| internal(format!("pack entry {} with no path", entry.id)))?;
    structures::remove(&path)?;
    Ok(path)
}

fn row(entry: &Entry, path: Option<String>) -> Deleted {
    Deleted {
        name: entry.name.clone(),
        id: entry.id.clone(),
        source: entry.source.as_str(),
        path,
    }
}

fn internal(what: String) -> CoreError {
    CoreError::Internal { what }
}
