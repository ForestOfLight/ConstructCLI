use crate::cli::StructuresArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::catalog;
use crate::support::format::human_size;
use crate::support::usage::check_source_against_world;
use construct_core::Result;
use construct_core::catalog::{Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::pack;
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    world: Option<String>,
    structures: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    name: &'a str,
    id: &'a str,
    source: &'static str,
    size_bytes: u64,
}

pub fn dispatch(args: &StructuresArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    check_source_against_world(
        "structures",
        "read from",
        args.world.as_ref(),
        args.source,
        false,
    )?;
    match &args.world {
        Some(reference) => {
            let world = ctx.world(reference)?;
            Ok(run(
                &world,
                &ctx.installations,
                args.source.map(Into::into),
                out,
            )?)
        }
        None => Ok(shared(ctx.installation()?, out)?),
    }
}

fn run(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    let loaded = catalog::for_world(world, installations, source, out)?;
    let entries: Vec<Entry> = loaded
        .entries
        .into_iter()
        .filter(|e| source.is_none_or(|s| e.source == s))
        .collect();
    render(Some(world.qualified()), &entries, out);
    Ok(())
}

fn shared(installation: &Installation, out: &mut Out) -> Result<()> {
    let pack = pack::for_installation(installation)?.pack;
    let entries = construct_core::catalog::from_pack(&pack.dir, Source::SharedPack);
    out.line(format!("{}", pack.dir.display()));
    render(None, &entries, out);
    Ok(())
}

fn render(world: Option<String>, entries: &[Entry], out: &mut Out) {
    if !out.is_json() {
        if entries.is_empty() {
            out.line("no structures");
        } else {
            out.line(format!("{:<24} {:<12} {:>9}", "NAME", "SOURCE", "SIZE"));
            for e in entries {
                out.line(format!(
                    "{:<24} {:<12} {:>9}",
                    e.name,
                    e.source.as_str(),
                    human_size(e.size_bytes)
                ));
            }
        }
    }

    out.emit(Payload {
        world,
        structures: entries
            .iter()
            .map(|e| Row {
                name: &e.name,
                id: &e.id,
                source: e.source.as_str(),
                size_bytes: e.size_bytes,
            })
            .collect(),
    });
}
