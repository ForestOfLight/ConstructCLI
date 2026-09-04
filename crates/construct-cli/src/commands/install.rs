//! Downloading Construct from GitHub and placing it, §10's sequence in order:
//! query the release, match the `.mcaddon` asset, download, extract, verify
//! it is actually Construct, place both packs, then (with `--world`) enable
//! them and flip Beta APIs on.

use crate::output::Out;
use construct_core::config::Backups;
use construct_core::discovery::{Installation, World};
use construct_core::install::releases::Releases;
use construct_core::install::{self, mcaddon, releases};
use construct_core::pack::{self, manifest};
use construct_core::{CoreError, Result, backup, leveldat, worldpacks};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Payload {
    version: String,
    tag: String,
    behavior: String,
    resource: String,
    preserved: usize,
    world: Option<String>,
    beta_apis: Option<bool>,
    level_dat_error: Option<String>,
}

/// Confirms an extracted pack really is Construct's, by header UUID rather
/// than by folder name or position in the archive.
///
/// `install::place` is generic over any pack — it has no idea what it is
/// installing. This command is the one that asked GitHub specifically for
/// Construct, so it is the one that has to check the answer: without this, a
/// `.mcaddon` that is not Construct would be installed under whatever UUID it
/// carries, and every later lookup by `CONSTRUCT_BP_UUID`/`CONSTRUCT_RP_UUID`
/// would miss it — the command would report success while achieving nothing.
fn verify_uuid(dir: &Path, expected: &str, kind: &str) -> Result<()> {
    let found = manifest::read(dir)?.uuid;
    if found != expected {
        return Err(CoreError::BadPack {
            path: dir.to_path_buf(),
            reason: format!(
                "not Construct's {kind} pack: expected header uuid {expected}, found {found}"
            ),
        });
    }
    Ok(())
}

pub fn run(
    releases: &dyn Releases,
    version: Option<&str>,
    world: Option<&World>,
    installation: &Installation,
    backups: &Backups,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let release = releases.release(version)?;
    let asset = releases::asset_for(&release)?;
    out.line(format!("Construct {} — {}", release.tag, asset.name));

    let staging = tempfile::tempdir()?;
    let archive = staging.path().join(&asset.name);
    releases.download(asset, &archive)?;
    let extracted = mcaddon::extract(&archive)?;

    // Refuse before anything is placed if the archive is not really Construct.
    verify_uuid(&extracted.behavior, pack::CONSTRUCT_BP_UUID, "behaviour")?;
    verify_uuid(&extracted.resource, pack::CONSTRUCT_RP_UUID, "resource")?;

    let bp = install::place(
        &pack::behavior_root(&installation.dev_pack_root),
        &extracted.behavior,
        force,
    )?;
    let rp = install::place(
        &pack::resource_root(&installation.dev_pack_root),
        &extracted.resource,
        force,
    )?;

    for placed in [&bp, &rp] {
        match (placed.changed, placed.from) {
            (false, _) => out.line(format!("  {} already current", placed.dir.display())),
            (true, Some(from)) => out.line(format!(
                "  {} {} → {}",
                placed.dir.display(),
                manifest::version_string(from),
                manifest::version_string(placed.to)
            )),
            (true, None) => out.line(format!(
                "  {} installed at {}",
                placed.dir.display(),
                manifest::version_string(placed.to)
            )),
        }
    }
    if bp.preserved > 0 {
        out.line(format!("  kept {} imported structure(s)", bp.preserved));
    }

    // Everything below this line is the `--world` half.
    let mut level_dat_error = None;
    let mut beta_apis = None;
    if let Some(world) = world {
        worldpacks::upsert(
            &worldpacks::behavior_path(world),
            worldpacks::PackRef {
                pack_id: pack::CONSTRUCT_BP_UUID.to_string(),
                version: bp.to,
            },
        )?;
        worldpacks::upsert(
            &worldpacks::resource_path(world),
            worldpacks::PackRef {
                pack_id: pack::CONSTRUCT_RP_UUID.to_string(),
                version: rp.to,
            },
        )?;
        out.line(format!("  enabled in {}", world.display_name));

        let level = world.path.join("level.dat");
        // The packs are already in place; from here a failure is partial, not
        // total. Back up before touching anything: a backup taken after a
        // bad write would preserve the bad write.
        match backup::file(&level, &world.qualified(), backups)
            .and_then(|_| leveldat::apply_beta_apis(&level, true))
        {
            Ok(change) => {
                beta_apis = Some(change.after);
                out.line("  Beta APIs on");
            }
            Err(e) => {
                out.warn(format!("could not turn Beta APIs on: {e}"));
                level_dat_error = Some(e.to_string());
            }
        }
    }
    out.line("Reload the world before Construct appears.");

    out.emit(Payload {
        version: manifest::version_string(bp.to),
        tag: release.tag,
        behavior: bp.dir.display().to_string(),
        resource: rp.dir.display().to_string(),
        preserved: bp.preserved,
        world: world.map(|w| w.display_name.clone()),
        beta_apis,
        level_dat_error: level_dat_error.clone(),
    });

    // §11: a failed level.dat write exits 5 with the packs installed, naming
    // the remaining manual step — not total failure. Exiting here rather than
    // returning an error keeps the success payload above intact, the same
    // way main.rs already handles the `-o` usage error.
    if level_dat_error.is_some() {
        eprintln!(
            "\nThe packs are installed. Turn Beta APIs on yourself, in the world's settings \
             under Experiments, or with:\n  construct experiment {} --beta-apis on",
            world.map(|w| w.display_name.as_str()).unwrap_or("<world>")
        );
        std::process::exit(5);
    }
    Ok(())
}
