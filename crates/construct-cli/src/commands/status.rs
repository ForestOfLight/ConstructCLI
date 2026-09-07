use crate::cli::StatusArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use crate::support::github;
use crate::support::phrasing::pack_short;
use construct_core::catalog;
use construct_core::discovery::{Installation, World};
use construct_core::install::releases::Releases;
use construct_core::pack::{self, manifest};
use construct_core::{Result, worldpacks};
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    installation: String,
    installed: String,
    pack: String,
    latest: Option<String>,
    enabled_worlds: Vec<String>,
    structures: Vec<PackRow>,
}

#[derive(Serialize)]
struct PackRow {
    world: Option<String>,
    source: &'static str,
    path: String,
    count: usize,
}

pub fn dispatch(_args: &StatusArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    Ok(run(
        &github::client(),
        ctx.installation()?,
        &ctx.worlds,
        out,
    )?)
}

fn run(
    releases: &dyn Releases,
    installation: &Installation,
    worlds: &[World],
    out: &mut Out,
) -> Result<()> {
    let installed = pack::for_installation(installation)?.pack;
    let version = manifest::version_string(installed.manifest.version);

    let latest = match releases.release(None) {
        Ok(r) => Some(r.tag),
        Err(e) => {
            out.warn(format!("could not check the latest version: {e}"));
            None
        }
    };

    let enabled: Vec<String> = worlds
        .iter()
        .filter(|w| w.installation == installation.name)
        .filter(|w| {
            worldpacks::read(&worldpacks::behavior_path(w))
                .map(|packs| packs.iter().any(|p| p.pack_id == pack::CONSTRUCT_BP_UUID))
                .unwrap_or(false)
        })
        .map(|w| w.display_name.clone())
        .collect();

    let mut structures = vec![PackRow {
        world: None,
        source: catalog::Source::SharedPack.as_str(),
        path: installed.dir.display().to_string(),
        count: pack::structures::list(&installed.dir).len(),
    }];
    let mut structure_lines = vec![format!(
        "{} in the shared copy of Construct, which every world using it sees",
        structures[0].count
    )];
    for world in worlds
        .iter()
        .filter(|w| w.installation == installation.name)
    {
        let Some(home) = pack::home(world) else {
            continue;
        };
        structures.push(PackRow {
            world: Some(world.display_name.clone()),
            source: home.kind.source().as_str(),
            path: home.dir.display().to_string(),
            count: pack::structures::list(&home.dir).len(),
        });
        let last = structures.last().expect("just pushed");
        structure_lines.push(format!(
            "{} in {}",
            last.count,
            pack_short(home.kind, &world.display_name)
        ));
    }

    out.line(format!(
        "Construct {version} at {}",
        installed.dir.display()
    ));
    match &latest {
        Some(tag) if tag.trim_start_matches('v') == version => out.line("  up to date"),
        Some(tag) => out.line(format!("  {tag} is available: construct install")),
        None => {}
    }
    if enabled.is_empty() {
        out.line("  enabled in no worlds");
    } else {
        out.line(format!("  enabled in: {}", enabled.join(", ")));
    }
    out.line("structures:");
    for line in &structure_lines {
        out.line(format!("  {line}"));
    }

    out.emit(Payload {
        installation: installation.name.clone(),
        installed: version,
        pack: installed.dir.display().to_string(),
        latest,
        enabled_worlds: enabled,
        structures,
    });
    Ok(())
}
