//! §10: what is installed, what is available, and where it is turned on.
//!
//! `install` is idempotent by design, which only helps a user who can see
//! what they already have. This command reports it: the installed version
//! from the shared copy's own manifest, the latest release GitHub has (when
//! reachable), and which worlds have Construct's behaviour pack enabled.
//!
//! It also reports where this installation's worlds keep their structures.
//! `structures` answers that one world at a time; a structure in the shared copy is
//! in every world using it, and only a cross-world view shows that at a
//! glance.
//!
//! Reading "which worlds" is a few small `world_behavior_packs.json` files,
//! and counting structures is a directory listing per pack — nothing here
//! opens a world's `db/`.

use crate::output::Out;
use construct_core::discovery::{Installation, World};
use construct_core::install::releases::Releases;
use construct_core::catalog;
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

/// One pack that holds structures, and how many it holds.
#[derive(Serialize)]
struct PackRow {
    /// The world this pack belongs to, or null for the shared copy of
    /// Construct, which belongs to every world using it.
    world: Option<String>,
    /// Which place this pack is, spelled as the `--source` value that names
    /// it: `shared-pack`, or `world-pack` for a pack only one world sees.
    source: &'static str,
    path: String,
    count: usize,
}

pub fn run(
    releases: &dyn Releases,
    installation: &Installation,
    worlds: &[World],
    out: &mut Out,
) -> Result<()> {
    // The shared copy of Construct, not a world's own: `status` reports on
    // the installation, the same thing `install` writes to.
    let installed = pack::for_installation(installation)?.pack;
    let version = manifest::version_string(installed.manifest.version);

    // Offline is not a failure: report what is here and say what could not be
    // checked. Both channels get it — stderr now, payload for a caller.
    let latest = match releases.release(None) {
        Ok(r) => Some(r.tag),
        Err(e) => {
            out.warn(format!("could not check the latest version: {e}"));
            None
        }
    };

    // Which worlds have it enabled: a few small JSON files, no database.
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

    // Where structures live, the shared copy first. A world with no home yet
    // has no structures of its own and no row: the shared line above it
    // already says what that world sees.
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
            crate::commands::pack_short(home.kind, &world.display_name)
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
