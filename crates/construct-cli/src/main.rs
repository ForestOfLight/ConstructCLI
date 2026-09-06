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
            // stdout before stderr, the same ordering `install` uses: the
            // machine-readable document lands first, the prose explaining it
            // second.
            out.emit_error(&err);
            report(&err);
            std::process::exit(exit_code(&err));
        }
    }
}

fn run(cli: &Cli, out: &mut Out) -> construct_core::Result<()> {
    let loaded = config::load(cli.config.as_deref(), &|k| std::env::var(k).ok())?;
    for w in &loaded.warnings {
        out.warn(w.clone());
    }

    // Precedence: CLI flag beats everything below it.
    //
    // Config roots keep their configured names — that is the entire reason §7 makes
    // `name` mandatory, and `config::parse` has already rejected duplicates and any
    // name that would shadow a built-in installation. Roots from `--path` have
    // no name to carry, so they are numbered.
    let mut extra_roots: Vec<(String, std::path::PathBuf)> = loaded
        .config
        .roots
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();
    // `--path` takes either kind of directory, so sort each one before use: a
    // world folder joins discovery directly under the `path` installation,
    // anything else is probed as a com.mojang root.
    // Worlds `construct add` recorded. They are extra worlds in exactly the
    // sense `--path` means, so they join the same list rather than a second one.
    let mut extra_worlds: Vec<std::path::PathBuf> = loaded.config.other_worlds.clone();
    let mut flag_roots = 0;
    for path in cli.command.paths() {
        match discovery::classify(path) {
            discovery::PathKind::World => extra_worlds.push(path.clone()),
            discovery::PathKind::Root => {
                flag_roots += 1;
                extra_roots.push((format!("flag{flag_roots}"), path.clone()));
            }
        }
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
    let worlds = discovery::enumerate(&installations, &extra_worlds);

    // Deliberately NOT an early return. A world reference may be a filesystem
    // path, which resolves with zero installations — §6 promises that, and every
    // CI runner depends on it. `no_installations` is only reported when it is
    // genuinely the explanation: a plain `WorldNotFound` with nothing to search
    // is best explained as "there's nothing to search." A `MalformedReference`
    // (bad syntax) or `UnreadableWorld` (found it, can't read it) is true
    // regardless of how many installations exist, and must pass through
    // untouched rather than being overwritten.
    //
    // "Nothing to search" is not the same as "no installations": `--path` can
    // name a world folder that belongs to no installation at all, and a world
    // in hand is something to search.
    let nothing_to_search = installations.is_empty() && worlds.is_empty();
    let no_installations = || CoreError::NoInstallations {
        probed: probed.clone(),
    };
    let resolve_world = |r: &str| {
        discovery::reference::resolve(r, &worlds).map_err(|e| {
            if nothing_to_search && matches!(e, CoreError::WorldNotFound { .. }) {
                no_installations()
            } else {
                e
            }
        })
    };

    match &cli.command {
        Command::Add { path } => commands::add::run(path, cli.config.as_deref(), out),
        Command::Worlds { .. } if nothing_to_search => Err(no_installations()),
        Command::Worlds { .. } => commands::worlds::run(&worlds, out),
        Command::Structures { world, source, .. } => {
            check_source_against_world("structures", "read from", world.as_ref(), *source, false);
            match world {
                Some(world) => {
                    let w = resolve_world(world)?;
                    commands::structures::run(&w, &installations, source.map(Into::into), out)
                }
                None => {
                    let installation = discovery::installation::choose(
                        &installations,
                        loaded.config.default_installation.as_deref(),
                    )?;
                    commands::structures::shared(installation, out)
                }
            }
        }
        Command::Export {
            world,
            structures,
            name: output,
            merge,
            on_overlap,
            source,
            force,
            ..
        } => {
            if *merge && output.is_none() {
                // --merge produces one file, and there is no structure name to
                // derive it from — the result is not any one of the inputs.
                eprintln!(
                    "error: --merge writes a single file and needs -n to name it\n\n\
                     construct export <s1> <s2>... --merge -n merged.mcstructure"
                );
                std::process::exit(2);
            }
            check_source_against_world(
                "export <structure>",
                "read from",
                world.as_ref(),
                *source,
                true,
            );
            // `-n` names the file this writes, and the only file Minecraft
            // loads is a `.mcstructure`. A missing extension is completed
            // rather than refused — `-n castle` is unambiguous — but a
            // different one is a mistake worth stopping: the bytes would be
            // fine and the file would be one the game never offers to load.
            let output = match output.as_deref().map(mcstructure_path) {
                Some(Err(found)) => {
                    eprintln!(
                        "error: -n writes a .mcstructure file, but {found} was given\n\n\
                         construct export <structure> -n {}.mcstructure",
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
                // -n names a single file and cannot name several. Usage error,
                // not a failure: nothing was attempted.
                eprintln!(
                    "error: -n takes a single output file, but {} structures were given\n\n\
                     Drop -n to write one file per structure, or add --merge to combine them \
                     into one.",
                    structures.len()
                );
                std::process::exit(2);
            }
            // Two scopes, two functions — the same shape `delete` has, and for
            // the same reason: absence means the shared copy of Construct and
            // `--world` means that world, and neither can reach the other's
            // files.
            match world {
                Some(reference) => {
                    let w = resolve_world(reference)?;
                    commands::export::for_world(
                        &w,
                        &installations,
                        structures,
                        output,
                        source.map(Into::into),
                        *force,
                        *merge,
                        (*on_overlap).into(),
                        out,
                    )
                }
                None => {
                    let installation = discovery::installation::choose(
                        &installations,
                        loaded.config.default_installation.as_deref(),
                    )?;
                    commands::export::shared(
                        installation,
                        structures,
                        output,
                        source.map(Into::into),
                        *force,
                        *merge,
                        (*on_overlap).into(),
                        out,
                    )
                }
            }
        }
        Command::Import {
            paths,
            world,
            name,
            force,
            ..
        } => {
            if name.is_some() {
                // --name renames one import and cannot name several, exactly
                // as -n names one output file. Usage error, not a failure:
                // nothing was attempted. A folder is a batch for the same
                // reason, whatever it happens to hold — its whole point is the
                // names it already carries.
                if let Some(dir) = paths.iter().find(|p| p.is_dir()) {
                    eprintln!(
                        "error: --name renames a single import, but {} is a folder\n\n\
                         A folder imports every structure under it, keeping its tree. \
                         Drop --name, or name a single file to rename it.",
                        dir.display()
                    );
                    std::process::exit(2);
                }
                if paths.len() > 1 {
                    eprintln!(
                        "error: --name renames a single import, but {} files were given\n\n\
                         Drop --name to derive each name from its file stem, or import them \
                         one at a time.",
                        paths.len()
                    );
                    std::process::exit(2);
                }
            }
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
                    loaded.config.default_installation.as_deref(),
                )?,
            };
            commands::import::run(
                paths,
                w.as_ref(),
                installation,
                name.as_deref(),
                *force,
                out,
            )
        }
        Command::Copy {
            src_world,
            dst_world,
            structures,
            source,
            force,
            ..
        } => {
            let src = resolve_world(src_world)?;
            let dst = resolve_world(dst_world)?;
            // No `--world` here to contradict, so all three `--source` values
            // are live: `copy` is the one command whose source world can see
            // its database, its own pack, and the shared copy at once.
            commands::copy::run(
                &src,
                &dst,
                structures,
                &installations,
                source.map(Into::into),
                *force,
                out,
            )
        }
        Command::Delete {
            structures,
            world,
            source,
            ..
        } => {
            check_source_against_world(
                "delete <structure>",
                "delete from",
                world.as_ref(),
                *source,
                true,
            );
            match world {
                Some(reference) => {
                    let w = resolve_world(reference)?;
                    commands::delete::for_world(
                        &w,
                        structures,
                        &installations,
                        source.map(Into::into),
                        out,
                    )
                }
                None => {
                    let installation = discovery::installation::choose(
                        &installations,
                        loaded.config.default_installation.as_deref(),
                    )?;
                    commands::delete::shared(installation, structures, source.map(Into::into), out)
                }
            }
        }
        Command::EnableBetaApis { world, .. } => {
            let w = resolve_world(world)?;
            commands::enable_beta_apis::run(&w, &loaded.config.backups, out)
        }
        Command::Install {
            version,
            world,
            force,
            ..
        } => {
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
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
                *force,
                out,
            )
        }
        Command::Status { .. } => {
            let installation = discovery::installation::choose(
                &installations,
                loaded.config.default_installation.as_deref(),
            )?;
            let client = github_client();
            commands::status::run(&client, installation, &worlds, out)
        }
        Command::Completions { shell } => commands::completions::run(*shell),
    }
}

/// Refuses the ways `--source` and `--world` contradict each other.
///
/// clap cannot express this. `--source` is one enum, but its values do not all
/// mean the same kind of place: `world-db` and `world-pack` name places inside
/// a world, so they need one named. That half applies everywhere.
///
/// `world_excludes_shared` is the other half, and only `export` and `delete`
/// set it: for them `--world` is the grammar's scope selector and the shared
/// copy is the thing it exists to keep them away from, so `--world W --source
/// shared-pack` asks for two incompatible things at once. `structures` does
/// not set it — a world listing *shows* the shared copy's rows when that is
/// what the world runs, so narrowing to them is a coherent request, and it is
/// the only way to list a world's structures without opening its database.
///
/// `copy` calls this for neither half: it takes its worlds as positionals
/// rather than `--world`, and its source world really can see all three places
/// at once.
///
/// Both refusals exit 2 — nothing has been attempted, and silently ignoring
/// either would answer a question the user did not ask.
fn check_source_against_world(
    usage: &str,
    verb: &str,
    world: Option<&String>,
    source: Option<cli::SourceArg>,
    world_excludes_shared: bool,
) {
    let Some(source) = source else { return };
    let value = source.as_str();
    if world.is_none() && source.needs_a_world() {
        eprintln!(
            "error: --source {value} needs a world to {verb}\n\n\
             construct {usage} --world <world> --source {value}"
        );
        std::process::exit(2);
    }
    if world.is_some() && !source.needs_a_world() && world_excludes_shared {
        eprintln!(
            "error: --source {value} cannot be combined with --world\n\n\
             The shared copy of Construct serves every world using it, so a command \
             aimed at one world never reaches it. Drop --world to {verb} the shared \
             copy:\n\n\
             construct {usage} --source {value}"
        );
        std::process::exit(2);
    }
}

/// `-n`'s path with the `.mcstructure` extension it must have, or the
/// extension that was given instead.
///
/// Case-insensitive on the way in, because the filesystems this runs on are:
/// refusing `-n CASTLE.MCSTRUCTURE` would refuse a name that already works.
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

/// The GitHub releases client `install` and `status` both need.
///
/// The API base is fixed at [`releases::API_BASE`]. GitHub is the only release
/// source there will be, and a runtime override would let anything able to set
/// an environment variable point `install` at a server of its choosing — an
/// unsigned download, run as the user, from wherever that variable said.
///
/// Debug builds still honour `CONSTRUCT_GITHUB_API`, which is how the test
/// suite serves canned responses without reaching the network. Release builds
/// do not: the branch is `cfg`'d out, so a shipped binary has no such seam.
fn github_client() -> releases::GitHub {
    let token = std::env::var("CONSTRUCT_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok();
    #[cfg(debug_assertions)]
    if let Ok(base) = std::env::var("CONSTRUCT_GITHUB_API") {
        return releases::GitHub::with_base(base, token);
    }
    releases::GitHub::new(token)
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
            eprintln!("\nPoint at one explicitly:\n  construct worlds --path <path>");
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
            // One axis now, so the hint is the matched values read back: every
            // collision — database against pack, or one pack against the other
            // — is separated by the same flag, and the values to try are
            // exactly the places that matched.
            eprintln!("\nDisambiguate with --source:");
            let mut alternatives = sources.iter();
            if let Some(first) = alternatives.next() {
                eprintln!("  construct <command> ... --source {first}");
                for other in alternatives {
                    eprintln!("  construct <command> ... --source {other}");
                }
            }
            // Construct's own list resolves this by letting the pack copy win
            // (§17). The CLI refuses instead — but the user is usually asking
            // which one the game shows, so answer it.
            if sources.iter().any(|s| s.ends_with("-pack")) {
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
        CoreError::Db { .. } => {
            // Every command that names a structure searches the world's
            // database as well as its packs, so an unopenable database stops
            // even a command that only ever meant a pack file. `delete` is the
            // one that must refuse rather than carry on: skipping the database
            // half would report a structure deleted while a copy of it
            // survived there.
            eprintln!(
                "\nIf you meant a structure in Construct's folder, naming that pack \
                 skips the database:\n  construct <command> ... --source world-pack   \
                 # or: --source shared-pack"
            );
        }
        CoreError::UnwritableLevelDat { written: false, .. } => {
            eprintln!("\nThe world was not modified.");
        }
        CoreError::WorldInUse { at_risk, .. } => {
            // Not a guess any more. Phase one suspected it because `db/` was
            // written recently; phase two then watched and saw a *further*
            // write land, which a finished command cannot produce.
            eprintln!(
                "\nMinecraft has this world open — a new write landed in its database while \
                 this command watched it for up to {} seconds.",
                construct_core::inuse::CONFIRM_WATCH.as_secs()
            );
            // The two write paths are stopped for different reasons, and the
            // reason is the part worth reading.
            match at_risk {
                construct_core::inuse::AtRisk::LevelDat => eprintln!(
                    "The game keeps level.dat in memory and rewrites it whenever it saves, \
                     so a change made now would be silently discarded."
                ),
                construct_core::inuse::AtRisk::Database => eprintln!(
                    "Writing to a world's database while the game has it open is how a save \
                     gets corrupted, so this stops before opening it at all. There is no \
                     --force."
                ),
            }
            eprintln!(
                "\nClose the world in Minecraft (returning to the main menu is enough), then \
                 run this again. The world was not modified."
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

/// 0 success · 1 failure · 2 usage.
///
/// The code answers two questions and no more: did it work, and was the input
/// at fault. *What* went wrong is `error.kind` under `--json`, which is finer
/// than a number can be — the retired code 3 alone covered six distinct
/// failures. Without `--json` the reason is the prose `report` prints, and a
/// shell script genuinely cannot tell a live world from a dead disk. That is
/// the trade: one honest code instead of a taxonomy nobody could extend.
///
/// The 2-arm is the interesting one. Ambiguity is a usage error because the
/// target exists and the reference was underspecified, and a malformed
/// reference likewise never named anything real — in both the user's input is
/// what needs fixing, which is exactly what separates 2 from 1.
fn exit_code(err: &CoreError) -> i32 {
    match err {
        CoreError::AmbiguousWorld { .. }
        | CoreError::AmbiguousStructure { .. }
        | CoreError::AmbiguousInstallation { .. }
        | CoreError::MalformedReference { .. }
        | CoreError::BadStructureName { .. } => 2,
        _ => 1,
    }
}
