//! Downloading Construct from GitHub and placing it, §10's sequence in order:
//! query the release, match the `.mcaddon` asset, download, extract, verify
//! it is actually Construct, place both packs, then (with `--world`) enable
//! them and flip Beta APIs on.

use crate::output::Out;
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

/// One pack rescued out of a non-development root, for `--json`.
#[derive(Serialize)]
struct Migrated {
    /// `moved` when the development root had no copy and the misplaced one
    /// became it, `merged` when it already had one and only structures were
    /// carried across.
    kind: &'static str,
    from: String,
    to: String,
    /// Structure files written into the development copy. Always 0 for a
    /// `moved`, where the pack arrived whole.
    merged: usize,
    rescued: Vec<Rescued>,
    /// Set when the structures all arrived but the emptied misplaced folder
    /// could not be removed.
    left_behind: Option<String>,
}

/// A structure that existed in both copies under one name with different
/// contents, and so was kept under a second name rather than dropped.
#[derive(Serialize)]
struct Rescued {
    from: String,
    to: String,
}

/// Folds a Construct sitting in `stray_root` — `behavior_packs` or
/// `resource_packs`, the non-development siblings a by-hand install is easy
/// to drop into — back into `dev_root`, and says so.
///
/// Runs before `install::place`, so a rescued pack is the one `place` then
/// upgrades and its structures are carried across the version bump by
/// `place`'s ordinary preservation rather than needing anything special here.
///
/// A failure is warned about, not returned: the misplaced copy is left
/// untouched by a failed `adopt`, which is exactly the state the user was
/// already in, and it is no reason to refuse to install Construct.
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
    // Refuse up front, before the network call and before anything is placed.
    // Two of the three things `--world` promises — enabling the packs in the
    // world and flipping Beta APIs — write files Minecraft holds in memory
    // and rewrites from memory on every save, so against a live world they
    // are discarded silently. Checking here rather than at the level.dat
    // write below means a refused `--world` install downloads nothing and
    // leaves nothing half-done; the user closes the world and re-runs.
    if let Some(world) = world {
        crate::commands::refuse_if_in_use(world, inuse::AtRisk::LevelDat, out)?;
    }

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

    // Before anything is placed: a Construct the player dropped into
    // `behavior_packs`/`resource_packs` instead of the `development_*`
    // sibling beside it. The game loads both roots, so the misplaced copy
    // shadows the one about to be installed, and no command here can see the
    // structures inside it.
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

    // Everything below this line is the `--world` half.
    let mut level_dat_error = None;
    let mut enable_error = None;
    let mut structures_error = None;
    let mut structures_pack = None;
    let mut beta_apis = None;
    if let Some(world) = world {
        // Placement above always writes into the shared
        // dev-pack root. But a world with its own `behavior_packs/Construct[BP]`
        // copy is governed by that copy, not the shared copy (`pack::for_world`'s
        // precedence — the same one `import`/`copy`/`delete`/`structures` resolve
        // through). Without this, install would report a version bump the
        // world never actually gets, and every later structure command would
        // keep writing into the untouched local copy.
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

        // The packs are already placed on disk; from here a failure is
        // partial, not total, same as the level.dat flip below. Try both
        // upserts rather than stopping at the first failure — they touch
        // independent files, so one failing is no reason to skip the other.
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

        // Give the world somewhere of its own to keep structures. A world
        // whose Construct copy is its own already has a per-world
        // `structures/` and needs no second pack; every other world would
        // otherwise share the installation's, which is what made "which
        // worlds have which structures" unanswerable.
        match pack::home(world) {
            Some(home) => {
                out.line(format!("  structures in {}", home.dir.display()));
                structures_pack = Some(home.dir.display().to_string());
            }
            None => match crate::commands::create_structures_pack(world, &bp.dir) {
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
    // Only worth saying without `--world`. A `--world` install refuses to run
    // at all while the world is open (the check at the top of this function),
    // so by the time it succeeds there is no live session to reload — the user
    // opens the world and Construct is already there. That also disposes of
    // the exit-5 branch below, which only `--world` can reach: what is left
    // there is finishing the enable and/or the flip, not reloading.
    if world.is_none() {
        out.line("Reload the world before Construct appears.");
    }

    out.emit(Payload {
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
    });

    // §11: the packs installed but a later step failing is partial, not
    // total, success — exit 5 and name the remaining manual step(s). Exiting
    // here rather than returning an error keeps the success payload above
    // intact, the same way main.rs already handles the `-n` usage error.
    if enable_error.is_some() || level_dat_error.is_some() || structures_error.is_some() {
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
        std::process::exit(5);
    }
    Ok(())
}
