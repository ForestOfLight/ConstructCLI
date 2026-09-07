use crate::cli::EnableBetaApisArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::packs::refuse_if_in_use;
use construct_core::Result;
use construct_core::config::Backups;
use construct_core::discovery::World;
use construct_core::{backup, inuse, leveldat};
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    world: String,
    beta_apis: bool,
    changed: bool,
    backup: Option<String>,
}

pub fn dispatch(args: &EnableBetaApisArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    let world = ctx.world(&args.world)?;
    Ok(run(&world, &ctx.settings.backups, out)?)
}

fn run(world: &World, backups: &Backups, out: &mut Out) -> Result<()> {
    let path = world.path.join("level.dat");

    refuse_if_in_use(world, inuse::AtRisk::LevelDat, out)?;

    if leveldat::read(&path)?.beta_apis() == Some(true) {
        out.line("Beta APIs already on; nothing to do");
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: true,
            changed: false,
            backup: None,
        });
        return Ok(());
    }

    let backup = backup::file(&path, &world.qualified(), backups)?;
    let change = leveldat::apply_beta_apis(&path, true).inspect_err(|_| {
        eprintln!("backup taken before the attempt: {}", backup.display());
    })?;

    if change.changed {
        out.line(format!("Beta APIs: {} → on", describe(change.before)));
        out.line(format!("  backup: {}", backup.display()));
    } else {
        out.line("Beta APIs already on; nothing to do");
    }
    out.emit(Payload {
        world: world.qualified(),
        beta_apis: change.after,
        changed: change.changed,
        backup: Some(backup.display().to_string()),
    });
    Ok(())
}

fn describe(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "on",
        Some(false) => "off",
        None => "off (this world has no experiments)",
    }
}
