mod cli;
mod commands;
mod output;

use clap::Parser;
use cli::{Cli, Command};
use construct_core::error::CoreError;
use construct_core::{config, discovery};
use output::Out;

fn main() {
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
        Command::Worlds if installations.is_empty() => Err(no_installations()),
        Command::Worlds => commands::worlds::run(&worlds, out),
        Command::List { world } => {
            let w = resolve_world(world)?;
            commands::list::run(&w, cli.source.map(Into::into), out)
        }
        Command::Export {
            world,
            structures,
            output,
        } => {
            if structures.len() > 1 && output.is_some() {
                // -o names a single file and cannot name several. Usage error,
                // not a failure: nothing was attempted.
                eprintln!(
                    "error: -o takes a single output file, but {} structures were given\n\n\
                     Drop -o to write one file per structure, or pass --merge (stage 3).",
                    structures.len()
                );
                std::process::exit(2);
            }
            let w = resolve_world(world)?;
            commands::export::run(
                &w,
                structures,
                output.as_deref(),
                cli.source.map(Into::into),
                cli.force,
                out,
            )
        }
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
        CoreError::AmbiguousStructure { name, sources } => {
            eprintln!("\n{name} exists in: {}", sources.join(", "));
            eprintln!("\nDisambiguate with --source:");
            eprintln!("  construct list <world> --source world   # or: --source pack");
            // Construct's own list resolves this by letting the pack copy win
            // (§17). The CLI refuses instead — but the user is usually asking
            // which one the game shows, so answer it.
            if sources.iter().any(|s| s == "pack") {
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
        _ => {}
    }
}

/// 0 success · 1 failure · 2 usage · 3 not found · 4 world in use.
///
/// Ambiguity is 2, not 3: the target exists, the reference was underspecified.
/// A malformed reference is also 2: the input never named anything real, so
/// it's the user's syntax that's wrong, not a lookup that failed.
fn exit_code(err: &CoreError) -> i32 {
    match err {
        CoreError::NoInstallations { .. }
        | CoreError::WorldNotFound { .. }
        | CoreError::StructureNotFound { .. }
        | CoreError::InstallationNotFound { .. }
        | CoreError::ConstructNotInstalled { .. } => 3,
        CoreError::AmbiguousWorld { .. }
        | CoreError::AmbiguousStructure { .. }
        | CoreError::AmbiguousInstallation { .. }
        | CoreError::MalformedReference { .. } => 2,
        CoreError::WorldInUse { .. } => 4,
        _ => 1,
    }
}
