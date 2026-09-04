//! §10: what is installed, what is available, and where it is turned on.
//!
//! `install` is idempotent by design, which only helps a user who can see
//! what they already have. This command reports it: the installed version
//! from the shared pack's own manifest, the latest release GitHub has (when
//! reachable), and which worlds have Construct's behaviour pack enabled.
//!
//! Reading "which worlds" is a few small `world_behavior_packs.json` files —
//! nothing here opens a world's `db/`.

use crate::output::Out;
use construct_core::discovery::{Installation, World};
use construct_core::install::releases::Releases;
use construct_core::pack::{self, manifest};
use construct_core::{CoreError, Result, worldpacks};
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    installation: String,
    installed: String,
    pack: String,
    latest: Option<String>,
    enabled_worlds: Vec<String>,
}

pub fn run(
    releases: &dyn Releases,
    installation: &Installation,
    worlds: &[World],
    out: &mut Out,
) -> Result<()> {
    let bp_root = pack::behavior_root(&installation.dev_pack_root);
    let installed = pack::find_by_uuid(&bp_root, pack::CONSTRUCT_BP_UUID).ok_or_else(|| {
        CoreError::ConstructNotInstalled {
            searched: vec![bp_root.clone()],
        }
    })?;
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

    out.emit(Payload {
        installation: installation.name.clone(),
        installed: version,
        pack: installed.dir.display().to_string(),
        latest,
        enabled_worlds: enabled,
    });
    Ok(())
}
