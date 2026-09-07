use crate::cli::CopyArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::catalog as loader;
use crate::support::packs::{home_for_write, warn_if_another_pack_has_it};
use crate::support::phrasing::{pack_phrase, target_field};
use construct_core::Result;
use construct_core::catalog::{self, Source};
use construct_core::discovery::{Installation, World, installation};
use construct_core::error::CoreError;
use construct_core::pack::structures;
use construct_core::store::StructureStore;
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    from: String,
    to: String,
    target: &'static str,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    id: String,
    path: String,
    bytes: u64,
}

pub fn dispatch(args: &CopyArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    let src = ctx.world(&args.src_world)?;
    let dst = ctx.world(&args.dst_world)?;
    let opts = Options {
        structures: &args.structures,
        source: args.source.map(Into::into),
        force: args.force,
    };
    Ok(run(&src, &dst, &ctx.installations, &opts, out)?)
}

pub struct Options<'a> {
    pub structures: &'a [String],
    pub source: Option<Source>,
    pub force: bool,
}

fn run(
    src: &World,
    dst: &World,
    installations: &[Installation],
    opts: &Options,
    out: &mut Out,
) -> Result<()> {
    let loaded = loader::for_world(src, installations, opts.source, out)?;
    let store = loaded.store.as_ref().map(|s| s as &dyn StructureStore);

    let mut sources = Vec::new();
    for name in opts.structures {
        let entry = catalog::resolve(name, &loaded.entries, opts.source)?;
        let bytes = catalog::read_entry(&entry, store)?;
        sources.push((entry, bytes));
    }

    let dst_installation = installation::for_world(installations, dst)?;
    let home = home_for_write(dst, dst_installation, out)?;

    let mut plan = Vec::new();
    for (entry, bytes) in sources {
        let target = structures::path_for(&home.dir, &entry.id)?;
        plan.push((entry, bytes, target));
    }
    if !opts.force {
        for (_, _, target) in &plan {
            if target.exists() {
                return Err(CoreError::TargetExists {
                    path: target.clone(),
                });
            }
        }
    }

    let mut written = Vec::new();
    for (entry, bytes, _) in plan {
        warn_if_another_pack_has_it(dst, dst_installation, &home.dir, &entry.id, out);
        let path = structures::write(&home.dir, &entry.id, &bytes, opts.force)?;

        out.line(format!(
            "copied {} from {} to {}",
            entry.name, src.display_name, dst.display_name
        ));
        out.line(format!(
            "  into {}",
            pack_phrase(home.kind, Some(dst.display_name.as_str()))
        ));
        out.line(format!("  {}", path.display()));

        written.push(Written {
            name: entry.name,
            id: entry.id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    out.line("Reload the destination world before Construct sees it.");
    out.emit(Payload {
        from: src.qualified(),
        to: dst.qualified(),
        target: target_field(home.kind),
        written,
    });
    Ok(())
}
