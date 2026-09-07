use crate::failure::Failure;
use crate::output::Out;
use construct_core::error::CoreError;

pub fn render(failure: &Failure, out: &Out) {
    match failure {
        Failure::AlreadyReported => {}
        Failure::Usage { message, hint } => eprintln!("error: {message}\n\n{hint}"),
        Failure::Core(err) => {
            out.emit_error(err);
            eprintln!("error: {err}");
            explain(err);
        }
    }
}

fn explain(err: &CoreError) {
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
            eprintln!("\nDisambiguate with --source:");
            let mut alternatives = sources.iter();
            if let Some(first) = alternatives.next() {
                eprintln!("  construct <command> ... --source {first}");
                for other in alternatives {
                    eprintln!("  construct <command> ... --source {other}");
                }
            }
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
            eprintln!(
                "\nMinecraft has this world open — a new write landed in its database while \
                 this command watched it for up to {} seconds.",
                construct_core::inuse::CONFIRM_WATCH.as_secs()
            );
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
            eprintln!(
                "\nlevel.dat was rewritten but did not read back as expected.\n\
                 Restore it from the backup printed above before relying on this world."
            );
        }
        _ => {}
    }
}
