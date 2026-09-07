use crate::cli::InstallArgs;
use crate::context::Context;
use crate::failure::{self, Failure};
use crate::output::Out;
use crate::support::github;
use crate::support::packs::{create_structures_pack, refuse_if_in_use};
use construct_core::config::Backups;
use construct_core::discovery::{Installation, World};
use construct_core::install::adopt::{self, AdoptKind};
use construct_core::install::releases::Releases;
use construct_core::install::{self, mcaddon, releases};
use construct_core::pack::{self, manifest};
use construct_core::{CoreError, Result, backup, inuse, leveldat, worldpacks};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Payload {
    version: String,
    tag: String,
    behavior: String,
    resource: String,
    preserved: usize,
    migrated: Vec<Migrated>,
    world: Option<String>,
    beta_apis: Option<bool>,
    structures_pack: Option<String>,
    enable_error: Option<String>,
    level_dat_error: Option<String>,
    structures_error: Option<String>,
}

#[derive(Serialize)]
struct Migrated {
    kind: &'static str,
    from: String,
    to: String,
    merged: usize,
    rescued: Vec<Rescued>,
    left_behind: Option<String>,
}

#[derive(Serialize)]
struct Rescued {
    from: String,
    to: String,
}

fn migrate_stray(
    dev_root: &Path,
    stray_root: &Path,
    uuid: &str,
    out: &mut Out,
) -> Option<Migrated> {
    let folder = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let adopted = match adopt::adopt(dev_root, stray_root, uuid) {
        Ok(Some(adopted)) => adopted,
        Ok(None) => return None,
        Err(e) => {
            out.warn(format!(
                "found Construct in {} but could not move it into {}: {e}",
                stray_root.display(),
                folder(dev_root)
            ));
            return None;
        }
    };

    let verb = match adopted.kind {
        AdoptKind::Moved => "moved",
        AdoptKind::Merged => "merged",
    };
    out.line(format!(
        "  {verb} the copy in {} into {}",
        folder(stray_root),
        folder(dev_root)
    ));
    out.line(format!(
        "    {} → {}",
        adopted.from.display(),
        adopted.to.display()
    ));
    if adopted.merged > 0 {
        out.line(format!("    kept {} structure(s) from it", adopted.merged));
    }
    for r in &adopted.rescued {
        out.line(format!(
            "    {} was already there and differed; kept as {}",
            r.from, r.to
        ));
    }
    if let Some(reason) = &adopted.left_behind {
        out.warn(format!(
            "every structure was carried across, but {} could not be removed: {reason}",
            adopted.from.display()
        ));
    }

    Some(Migrated {
        kind: verb,
        from: adopted.from.display().to_string(),
        to: adopted.to.display().to_string(),
        merged: adopted.merged,
        rescued: adopted
            .rescued
            .into_iter()
            .map(|r| Rescued {
                from: r.from,
                to: r.to,
            })
            .collect(),
        left_behind: adopted.left_behind,
    })
}

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

pub fn dispatch(args: &InstallArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    let world = ctx.optional_world(args.world.as_deref())?;
    let installation = ctx.installation_for(world.as_ref())?;
    run(
        &github::client(),
        args.version.as_deref(),
        world.as_ref(),
        installation,
        &ctx.settings.backups,
        args.force,
        out,
    )
}

fn run(
    releases: &dyn Releases,
    version: Option<&str>,
    world: Option<&World>,
    installation: &Installation,
    backups: &Backups,
    force: bool,
    out: &mut Out,
) -> failure::Result {
    if let Some(world) = world {
        refuse_if_in_use(world, inuse::AtRisk::LevelDat, out)?;
    }

    let release = releases.release(version)?;
    let asset = releases::asset_for(&release)?;
    out.line(format!("Construct {} — {}", release.tag, asset.name));

    let staging = tempfile::tempdir()?;
    let archive = staging.path().join(&asset.name);
    releases.download(asset, &archive)?;
    let extracted = mcaddon::extract(&archive)?;

    verify_uuid(&extracted.behavior, pack::CONSTRUCT_BP_UUID, "behaviour")?;
    verify_uuid(&extracted.resource, pack::CONSTRUCT_RP_UUID, "resource")?;

    let root = &installation.dev_pack_root;
    let migrated: Vec<Migrated> = [
        (
            pack::shared_behavior_root(root),
            pack::stray_behavior_root(root),
            pack::CONSTRUCT_BP_UUID,
        ),
        (
            pack::shared_resource_root(root),
            pack::stray_resource_root(root),
            pack::CONSTRUCT_RP_UUID,
        ),
    ]
    .iter()
    .filter_map(|(dev, stray, uuid)| migrate_stray(dev, stray, uuid, out))
    .collect();

    let bp = install::place(
        &pack::shared_behavior_root(&installation.dev_pack_root),
        &extracted.behavior,
        force,
    )?;
    let rp = install::place(
        &pack::shared_resource_root(&installation.dev_pack_root),
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

    let mut level_dat_error = None;
    let mut enable_error = None;
    let mut structures_error = None;
    let mut structures_pack = None;
    let mut beta_apis = None;
    if let Some(world) = world {
        if let Ok(target) = pack::for_world(world, installation)
            && let Some(shared) = &target.also_at
        {
            out.warn(format!(
                "two copies of Construct in {}; the world's own copy at {} shadows the one \
                 just installed at {}",
                world.display_name,
                target.pack.dir.display(),
                shared.display()
            ));
        }

        let bp_upsert = worldpacks::upsert(
            &worldpacks::behavior_path(world),
            worldpacks::PackRef {
                pack_id: pack::CONSTRUCT_BP_UUID.to_string(),
                version: bp.to,
            },
        );
        let rp_upsert = worldpacks::upsert(
            &worldpacks::resource_path(world),
            worldpacks::PackRef {
                pack_id: pack::CONSTRUCT_RP_UUID.to_string(),
                version: rp.to,
            },
        );
        match (&bp_upsert, &rp_upsert) {
            (Ok(_), Ok(_)) => out.line(format!("  enabled in {}", world.display_name)),
            _ => {
                let mut reasons = Vec::new();
                if let Err(e) = &bp_upsert {
                    reasons.push(format!("behaviour pack: {e}"));
                }
                if let Err(e) = &rp_upsert {
                    reasons.push(format!("resource pack: {e}"));
                }
                let reason = reasons.join("; ");
                out.warn(format!(
                    "could not enable Construct in {}: {reason}",
                    world.display_name
                ));
                enable_error = Some(reason);
            }
        }

        match pack::home(world) {
            Some(home) => {
                out.line(format!("  structures in {}", home.dir.display()));
                structures_pack = Some(home.dir.display().to_string());
            }
            None => match create_structures_pack(world, &bp.dir) {
                Ok(created) => {
                    out.line(format!("  structures pack at {}", created.dir.display()));
                    structures_pack = Some(created.dir.display().to_string());
                }
                Err(e) => {
                    out.warn(format!("could not create the structures pack: {e}"));
                    structures_error = Some(e.to_string());
                }
            },
        }

        let level = world.path.join("level.dat");
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
    if world.is_none() {
        out.line("Reload the world before Construct appears.");
    }

    let partial = enable_error.is_some() || level_dat_error.is_some() || structures_error.is_some();
    let payload = Payload {
        version: manifest::version_string(bp.to),
        tag: release.tag,
        behavior: bp.dir.display().to_string(),
        resource: rp.dir.display().to_string(),
        preserved: bp.preserved,
        migrated,
        world: world.map(|w| w.display_name.clone()),
        beta_apis,
        structures_pack,
        enable_error: enable_error.clone(),
        level_dat_error: level_dat_error.clone(),
        structures_error: structures_error.clone(),
    };
    if !partial {
        out.emit(payload);
        return Ok(());
    }

    out.emit_with_error(
        payload,
        "partial-install",
        "the packs are installed, but a later step did not finish",
    );

    let world_name = world.map(|w| w.display_name.as_str()).unwrap_or("<world>");
    eprintln!("\nThe packs are installed.");
    if enable_error.is_some() {
        eprintln!(
            "Re-run to finish enabling Construct in the world — install is safe to \
             repeat; already-placed packs are left alone:\n  construct install --world {world_name}{}",
            version
                .map(|v| format!(" --version {v}"))
                .unwrap_or_default()
        );
    }
    if level_dat_error.is_some() {
        eprintln!(
            "Turn Beta APIs on yourself, in the world's settings under Experiments, \
             or with:\n  construct enable-beta-apis {world_name}"
        );
    }
    if structures_error.is_some() {
        eprintln!(
            "The world has nowhere of its own to keep structures. Re-run to try again \
             — `import` and `copy` also create it when they need it:\n  \
             construct install --world {world_name}"
        );
    }
    Err(Failure::AlreadyReported)
}
