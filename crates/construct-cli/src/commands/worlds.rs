use crate::cli::WorldsArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::format::{human_size, truncate};
use construct_core::discovery::World;
use construct_core::{Result, discovery};
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    worlds: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    installation: &'a str,
    account: Option<&'a str>,
    folder: &'a str,
    display_name: &'a str,
    qualified: String,
    path: String,
    size_bytes: u64,
    last_played: Option<i64>,
    last_played_source: &'static str,
}

pub fn dispatch(_args: &WorldsArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    if ctx.nothing_to_search() {
        return Err(ctx.no_installations().into());
    }
    Ok(run(&ctx.worlds, out)?)
}

fn run(worlds: &[World], out: &Out) -> Result<()> {
    if !out.is_json() {
        out.line(format!(
            "{:<28} {:<14} {:>8}  {}",
            "NAME", "INSTALLATION", "SIZE", "REFERENCE"
        ));
        for w in worlds {
            out.line(format!(
                "{:<28} {:<14} {:>8}  {}",
                truncate(&w.display_name, 28),
                w.installation,
                human_size(w.size_bytes),
                w.qualified()
            ));
        }
        if worlds.is_empty() {
            out.line("no worlds found");
        }
    }

    out.emit(Payload {
        worlds: worlds
            .iter()
            .map(|w| Row {
                installation: &w.installation,
                account: w.account.as_deref(),
                folder: &w.folder,
                display_name: &w.display_name,
                qualified: w.qualified(),
                path: w.path.display().to_string(),
                size_bytes: w.size_bytes,
                last_played: w.last_played,
                last_played_source: match w.last_played_source {
                    discovery::LastPlayedSource::LevelDat => "level.dat",
                    discovery::LastPlayedSource::DirMtime => "dir-mtime",
                },
            })
            .collect(),
    });
    Ok(())
}
