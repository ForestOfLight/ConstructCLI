mod cli;
mod commands;
mod complete;
mod output;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use construct_core::error::CoreError;
use construct_core::install::releases;
use construct_core::{config, discovery};
use output::Out;
use std::path::{Path, PathBuf};

fn main() {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let mut out = Out::new(cli.json);

    match run(&cli, &mut out) {
        Ok(()) => {}
        Err(err) => {
            report(&err);
            std::process::exit(exit_code(&err));
        }
    }
}

fn run(cli: &Cli, out: &mut Out) -> construct_core::Result<()> {
    let loaded = config::load(None, &|k| std::env::var(k).ok())?;
    for w in &loaded.warnings {
        out.warn(w.clone());
    }

    // Precedence: CLI flag beats everything below it.
    //
    // Config roots keep their configured names — that is the entire reason §7 makes
    // `name` mandatory, and `config::parse` has already rejected duplicates and any
    // name that would shadow a built-in installation. Roots from `--com-mojang` have
    // no name to carry, so they are numbered.
    let mut extra_roots: Vec<(String, std::path::PathBuf)> = loaded
        .config
        .roots
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();
    for (i, path) in cli.com_mojang.iter().enumerate() {
        extra_roots.push((format!("flag{}", i + 1), path.clone()));
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let appdata = std::env::var("APPDATA").ok().map(std::path::PathBuf::from);
    let localappdata = std::env::var("LOCALAPPDATA")
        .ok()
        .map(std::path::PathBuf::from);

    let mut candidates = discovery::platform::candidates(
        std::path::Path::new(&home),
        appdata.as_deref(),
        localappdata.as_deref(),
    );
    for (name, root) in &extra_roots {
        candidates.push(discovery::platform::Candidate {
            name: name.clone(),
            dev_pack_root: root.clone(),
            world_root_parents: vec![root.clone()],
            per_account: false,
        });
    }
    let probed: Vec<std::path::PathBuf> =
        candidates.iter().map(|c| c.dev_pack_root.clone()).collect();
    // A configured root must be addressable by its name, so assert the wiring holds:
    // every configured root name appears among the candidates.
    debug_assert!(
        loaded
            .config
            .roots
            .iter()
            .all(|r| candidates.iter().any(|c| c.name == r.name)),
        "a configured root lost its name before discovery"
    );

    let installations = discovery::platform::resolve(candidates);
    let worlds = discovery::enumerate(&installations);

    // Deliberately NOT an early return. A world reference may be a filesystem
    // path, which resolves with zero installations — §6 promises that, and every
    // CI runner depends on it. `no_installations` is only reported when it is
    // genuinely the explanation: a plain `WorldNotFound` with no installations
    // present is best explained as "there's nothing to search." A
    // `MalformedReference` (bad syntax) or `UnreadableWorld` (found it, can't
    // read it) is true regardless of how many installations exist, and must
    // pass through untouched rather than being overwritten.
    let no_installations = || CoreError::NoInstallations {
        probed: probed.clone(),
    };
    let resolve_world = |r: &str| {
        discovery::reference::resolve(r, &worlds).map_err(|e| {
            if installations.is_empty() && matches!(e, CoreError::WorldNotFound { .. }) {
                no_installations()
            } else {
                e
            }
        })
    };

    match &cli.command {
        Command::Add { path } => commands::add::run(path, out),
        Command::Worlds if installations.is_empty() => Err(no_installations()),
        Command::Worlds => commands::worlds::run(&worlds, out),
        Command::List { world: Some(world) } => {
            let w = resolve_world(world)?;
            commands::list::run(
                &w,
                &installations,
                cli.source.map(Into::into),
                cli.pack.map(Into::into),
                out,
            )
        }
        Command::List { world: None } => {
            if cli.source == Some(cli::SourceArg::World) {
                // Nothing to read: a world's structures live in a world's
                // database, and no world was named. Usage error, not a
                // failure — nothing was attempted.
                eprintln!(
                    "error: --source world needs a world to read from\n\n\
                     construct list <world> --source world"
                );
                std::process::exit(2);
            }
            let installation = discovery::installation::choose(
                &installations,
                std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                loaded.config.default_installation.as_deref(),
            )?;
            commands::list::shared(installation, out)
        }
        Command::Export {
            world,
            structures,
            output,
            merge,
            on_overlap,
        } => {
            if *merge && output.is_none() {
                // --merge produces one file, and there is no structure name to
                // derive it from — the result is not any one of the inputs.
                eprintln!(
                    "error: --merge writes a single file and needs -o to name it\n\n\
                     construct export <world> <s1> <s2>... --merge -o merged.mcstructure"
                );
                std::process::exit(2);
            }
            // `-o` names the file this writes, and the only file Minecraft
            // loads is a `.mcstructure`. A missing extension is completed
            // rather than refused — `-o castle` is unambiguous — but a
            // different one is a mistake worth stopping: the bytes would be
            // fine and the file would be one the game never offers to load.
            let output = match output.as_deref().map(mcstructure_path) {
                Some(Err(found)) => {
                    eprintln!(
                        "error: -o writes a .mcstructure file, but {found} was given\n\n\
                         construct export <world> <structure> -o {}.mcstructure",
                        output
                            .as_deref()
                            .and_then(|p| p.file_stem())
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "out".to_string())
                    );
                    std::process::exit(2);
                }
                other => other.map(|r| r.expect("Err handled above")),
            };
            let output = output.as_deref();
            if !*merge && structures.len() > 1 && output.is_some() {
                // -o names a single file and cannot name several. Usage error,
                // not a failure: nothing was attempted.
                eprintln!(
                    "error: -o takes a single output file, but {} structures were given\n\n\
                     Drop -o to write one file per structure, or add --merge to combine them \
                     into one.",
                    structures.len()
                );
                std::process::exit(2);
            }
            let w = resolve_world(world)?;
            commands::export::run(
                &w,
                &installations,
                structures,
                output,
                cli.source.map(Into::into),
                cli.pack.map(Into::into),
                cli.force,
                *merge,
                (*on_overlap).into(),
                out,
            )
        }
        Command::Import { files, world, name } => {
            if name.is_some() && files.len() > 1 {
                // --name renames one import and cannot name several, exactly
                // as -o names one output file. Usage error, not a failure:
                // nothing was attempted.
                eprintln!(
                    "error: --name renames a single import, but {} files were given\n\n\
                     Drop --name to derive each name from its file stem, or import them \
                     one at a time.",
                    files.len()
                );
                std::process::exit(2);
            }
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
                    std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                    loaded.config.default_installation.as_deref(),
                )?,
            };
            commands::import::run(
                files,
                w.as_ref(),
                installation,
                name.as_deref(),
                cli.force,
                out,
            )
        }
        Command::Copy {
            src_world,
            dst_world,
            structures,
        } => {
            let src = resolve_world(src_world)?;
            let dst = resolve_world(dst_world)?;
            commands::copy::run(
                &src,
                &dst,
                structures,
                &installations,
                cli.source.map(Into::into),
                cli.pack.map(Into::into),
                cli.force,
                out,
            )
        }
        Command::Delete { world, structures } => {
            let w = resolve_world(world)?;
            commands::delete::run(
                &w,
                structures,
                &installations,
                cli.source.map(Into::into),
                cli.pack.map(Into::into),
                out,
            )
        }
        Command::Experiment { world, beta_apis } => {
            let w = resolve_world(world)?;
            commands::experiment::run(
                &w,
                beta_apis.map(cli::OnOff::as_bool),
                &loaded.config.backups,
                out,
            )
        }
        Command::Install { version, world } => {
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
                    std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                    loaded.config.default_installation.as_deref(),
                )?,
            };
            let client = github_client();
            commands::install::run(
                &client,
                version.as_deref(),
                w.as_ref(),
                installation,
                &loaded.config.backups,
                cli.force,
                out,
            )
        }
        Command::Status => {
            let installation = discovery::installation::choose(
                &installations,
                std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                loaded.config.default_installation.as_deref(),
            )?;
            let client = github_client();
            commands::status::run(&client, installation, &worlds, out)
        }
        Command::Completions { shell } => commands::completions::run(*shell),
    }
}

/// `-o`'s path with the `.mcstructure` extension it must have, or the
/// extension that was given instead.
///
/// Case-insensitive on the way in, because the filesystems this runs on are:
/// refusing `-o CASTLE.MCSTRUCTURE` would refuse a name that already works.
/// A path with no file name at all (`.`, `/`) is returned untouched — it is
/// not a file this could correct, and the write below fails on its own terms
/// with the real reason.
fn mcstructure_path(path: &Path) -> std::result::Result<PathBuf, String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(construct_core::pack::structures::EXTENSION) => {
            Ok(path.to_path_buf())
        }
        Some(other) => Err(format!(".{other}")),
        None => Ok(path.with_extension(construct_core::pack::structures::EXTENSION)),
    }
}

/// The GitHub releases client `install` and `status` both need, pointed at a
/// stub server under `CONSTRUCT_GITHUB_API` in tests, the real API otherwise.
fn github_client() -> releases::GitHub {
    let token = std::env::var("CONSTRUCT_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok();
    match std::env::var("CONSTRUCT_GITHUB_API") {
        Ok(base) => releases::GitHub::with_base(base, token),
        Err(_) => releases::GitHub::new(token),
    }
}

/// Every message names the thing, says why, and gives the next action.
fn report(err: &CoreError) {
    eprintln!("error: {err}");
    match err {
        CoreError::NoInstallations { probed } => {
            eprintln!("\nprobed:");
            for p in probed {
                eprintln!("  {}", p.display());
            }
            eprintln!("\nPoint at one explicitly:\n  construct worlds --com-mojang <path>");
        }
        CoreError::MalformedReference {
            looks_like_path, ..
        } if *looks_like_path => {
            eprintln!("\nThat looks like a filesystem path, but it was not found on disk.");
        }
        CoreError::MalformedReference { .. } => {
            eprintln!(
                "\nExpected a world name, or a qualified <installation>/<account>/<world> reference."
            );
        }
        CoreError::UnreadableWorld { path, reason } => {
            eprintln!("\ncould not read {}: {reason}", path.display());
            eprintln!("Check that you have permission to read this directory.");
        }
        CoreError::AmbiguousWorld { candidates, .. } => {
            eprintln!("\nUse a qualified reference:");
            for c in candidates {
                eprintln!("  {c}");
            }
        }
        CoreError::WorldNotFound { near, .. } if !near.is_empty() => {
            eprintln!("\nDid you mean:");
            for n in near {
                eprintln!("  {n}");
            }
        }
        CoreError::StructureNotFound { near, .. } if !near.is_empty() => {
            eprintln!("\nDid you mean:");
            for n in near {
                eprintln!("  {n}");
            }
        }
        CoreError::TargetExists { .. } => {
            eprintln!("\nPass --force to overwrite.");
        }
        CoreError::MergeRefused { .. } => {
            eprintln!(
                "\nNothing was written. Check that the structures were saved at different \
                 places in the world — merge reassembles them at their recorded positions."
            );
        }
        CoreError::BadStructureFile { .. } => {
            eprintln!("\nThe file is not a readable .mcstructure.");
        }
        CoreError::AmbiguousStructure { name, sources } => {
            eprintln!("\n{name} exists in: {}", sources.join(", "));
            // Two packs need `--pack`; `--source` cannot separate them, since
            // both matches *are* pack entries. Saying "--source" there would
            // send the user round a loop that never resolves.
            if sources.iter().all(|s| s.starts_with("pack:")) {
                eprintln!("\nBoth are packs this world sees. Pick one with --pack:");
                eprintln!("  construct <command> ... --pack world   # or: --pack shared");
            } else {
                eprintln!("\nDisambiguate with --source:");
                eprintln!("  construct list <world> --source world   # or: --source pack");
            }
            // Construct's own list resolves this by letting the pack copy win
            // (§17). The CLI refuses instead — but the user is usually asking
            // which one the game shows, so answer it.
            // `starts_with`, not equality: a pack entry is labelled by which
            // pack it is in (`pack:world`, `pack:shared`), so matching the
            // bare word would never fire.
            if sources.iter().any(|s| s.starts_with("pack")) {
                eprintln!("\nConstruct shows the pack copy in-game.");
            }
        }
        CoreError::InstallationNotFound { available, .. } => {
            eprintln!("\navailable:");
            for a in available {
                eprintln!("  {a}");
            }
            eprintln!("\nSet one in config.toml:\n  default_installation = \"<name>\"");
        }
        CoreError::AmbiguousInstallation { candidates } => {
            eprintln!("\ncandidates:");
            for c in candidates {
                eprintln!("  {c}");
            }
            eprintln!("\nSet one in config.toml:\n  default_installation = \"<name>\"");
        }
        CoreError::ConstructNotInstalled { searched } => {
            eprintln!("\nsearched:");
            for s in searched {
                eprintln!("  {}", s.display());
            }
            eprintln!("\nInstall it:\n  construct install");
        }
        CoreError::BadStructureName { .. } => {
            eprintln!("\nChoose a name explicitly:\n  construct import <file> --name <name>");
        }
        CoreError::NotImplemented { .. } => {
            eprintln!("\nUse --source pack to delete an imported structure.");
        }
        CoreError::UnwritableLevelDat { written: false, .. } => {
            eprintln!("\nThe world was not modified.");
        }
        CoreError::WorldInUse { .. } => {
            eprintln!(
                "\nMinecraft appears to have this world open — its database was written \
                 in the last {} seconds.\n\
                 The game keeps level.dat in memory and rewrites it whenever it saves, so \
                 a change made now would be silently discarded.\n\n\
                 Close the world in Minecraft (returning to the main menu is enough), then \
                 run this again. The world was not modified.",
                construct_core::inuse::ACTIVITY_WINDOW.as_secs()
            );
        }
        CoreError::RateLimited => {
            eprintln!(
                "\nGitHub allows {} requests an hour unauthenticated.\n\
                 Set a token to raise it:\n  export CONSTRUCT_GITHUB_TOKEN=<token>",
                construct_core::install::releases::UNAUTHENTICATED_LIMIT
            );
        }
        CoreError::AssetNotFound { available, .. } if !available.is_empty() => {
            eprintln!("\navailable assets:");
            for a in available {
                eprintln!("  {a}");
            }
        }
        CoreError::IncompleteInstall { staging, .. } => {
            eprintln!(
                "\nThe new pack is staged at {} — move it into place by hand, \
                 or delete it and re-run install.",
                staging.display()
            );
        }
        CoreError::UnwritableLevelDat { written: true, .. } => {
            // Unlike the refusal-before-write case above, `write` already
            // renamed a new level.dat into place before verification failed:
            // the world genuinely changed, just not into the requested
            // state. Saying "not modified" here would be false, and would
            // give the user no reason to reach for the backup that was just
            // taken for them.
            eprintln!(
                "\nlevel.dat was rewritten but did not read back as expected.\n\
                 Restore it from the backup printed above before relying on this world."
            );
        }
        _ => {}
    }
}

/// 0 success · 1 failure · 2 usage · 3 not found · 4 world in use · 5 partial
/// success.
///
/// Ambiguity is 2, not 3: the target exists, the reference was underspecified.
/// A malformed reference is also 2: the input never named anything real, so
/// it's the user's syntax that's wrong, not a lookup that failed.
///
/// 5 never comes from this function: `commands::install::run` exits directly
/// with it when the packs are installed but the `level.dat` write failed, so
/// the success payload already printed is not overwritten by an error path.
fn exit_code(err: &CoreError) -> i32 {
    match err {
        CoreError::NoInstallations { .. }
        | CoreError::WorldNotFound { .. }
        | CoreError::StructureNotFound { .. }
        | CoreError::InstallationNotFound { .. }
        | CoreError::ConstructNotInstalled { .. }
        | CoreError::AssetNotFound { .. } => 3,
        CoreError::AmbiguousWorld { .. }
        | CoreError::AmbiguousStructure { .. }
        | CoreError::AmbiguousInstallation { .. }
        | CoreError::MalformedReference { .. }
        | CoreError::BadStructureName { .. }
        | CoreError::NotImplemented { .. } => 2,
        CoreError::WorldInUse { .. } => 4,
        _ => 1,
    }
}
