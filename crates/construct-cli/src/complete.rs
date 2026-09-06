use clap::builder::StyledStr;
use clap_complete::engine::CompletionCandidate;
use construct_core::catalog;
use construct_core::config;
use construct_core::discovery::{self, Installation, World};
use construct_core::pack;
use construct_core::store;
use std::path::PathBuf;

/// Complete discovered Minecraft Bedrock worlds.
pub fn complete_worlds() -> Vec<CompletionCandidate> {
    let (installations, worlds) = discover_environment();
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for w in &worlds {
        // Display name
        if seen.insert(w.display_name.clone()) {
            let help_text = format!("{} ({})", w.installation, w.folder);
            candidates.push(
                CompletionCandidate::new(&w.display_name).help(Some(StyledStr::from(help_text))),
            );
        }

        // Folder name (if different from display name)
        if w.folder != w.display_name && seen.insert(w.folder.clone()) {
            let help_folder = format!("{} ({})", w.installation, w.display_name);
            candidates.push(
                CompletionCandidate::new(&w.folder).help(Some(StyledStr::from(help_folder))),
            );
        }

        // Qualified reference (installation/folder or installation/account/folder)
        if (installations.len() > 1 || w.account.is_some()) && seen.insert(w.qualified()) {
            let qualified = w.qualified();
            let help_qual = format!("{} - {}", w.installation, w.display_name);
            candidates
                .push(CompletionCandidate::new(qualified).help(Some(StyledStr::from(help_qual))));
        }
    }

    candidates
}

/// Complete structure names for the world targeted in the current command line.
pub fn complete_structures() -> Vec<CompletionCandidate> {
    let (installations, worlds) = discover_environment();
    let target_world_str = extract_target_world_from_args();

    let Some(world_ref) = target_world_str else {
        return Vec::new();
    };

    let Ok(world) = discovery::reference::resolve(&world_ref, &worlds) else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. World database structures
    if let Ok(store) = store::open_world_store(&world)
        && let Ok(entries) = catalog::from_world(&store)
    {
        for entry in entries {
            if seen.insert(entry.name.clone()) {
                candidates.push(
                    CompletionCandidate::new(entry.name)
                        .help(Some(StyledStr::from("world database structure"))),
                );
            }
        }
    }

    // 2. Pack structures
    if let Ok(inst) = discovery::installation::for_world(&installations, &world) {
        let serving = pack::serving(&world, inst);
        for home in serving {
            let scope_help = match home.kind.scope() {
                pack::Scope::WorldLocal => "world pack structure",
                pack::Scope::Shared => "shared construct structure",
            };
            let entries = catalog::from_pack(&home.dir, home.kind.scope());
            for entry in entries {
                if seen.insert(entry.name.clone()) {
                    candidates.push(
                        CompletionCandidate::new(entry.name)
                            .help(Some(StyledStr::from(scope_help))),
                    );
                }
            }
        }
    }

    candidates.sort_by(|a, b| a.get_value().cmp(b.get_value()));
    candidates
}

/// Helper to discover installations and worlds safely without throwing or exiting.
fn discover_environment() -> (Vec<Installation>, Vec<World>) {
    let extra_com_mojang = extract_com_mojang_from_args();
    let loaded = config::load(None, &|k| std::env::var(k).ok()).ok();

    let mut extra_roots: Vec<(String, PathBuf)> = loaded
        .as_ref()
        .map(|l| {
            l.config
                .roots
                .iter()
                .map(|r| (r.name.clone(), r.path.clone()))
                .collect()
        })
        .unwrap_or_default();

    for (i, path) in extra_com_mojang.iter().enumerate() {
        extra_roots.push((format!("flag{}", i + 1), path.clone()));
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
    let localappdata = std::env::var("LOCALAPPDATA").ok().map(PathBuf::from);

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

    let installations = discovery::platform::resolve(candidates);
    let worlds = discovery::enumerate(&installations);
    (installations, worlds)
}

/// Extract `--com-mojang <path>` arguments from the invoking command line args.
fn extract_com_mojang_from_args() -> Vec<PathBuf> {
    let args: Vec<String> = get_command_words();
    let mut roots = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--com-mojang" && i + 1 < args.len() {
            roots.push(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if let Some(stripped) = args[i].strip_prefix("--com-mojang=") {
            roots.push(PathBuf::from(stripped));
            i += 1;
        } else {
            i += 1;
        }
    }
    roots
}

/// Extract the target world reference for structure completion from the active command line.
fn extract_target_world_from_args() -> Option<String> {
    let words = get_command_words();
    if words.is_empty() {
        return None;
    }

    // Find subcommand
    let mut subcmd_idx = None;
    let mut subcmd = "";
    for (i, word) in words.iter().enumerate() {
        if word == "export" || word == "copy" || word == "delete" {
            subcmd_idx = Some(i);
            subcmd = word.as_str();
            break;
        }
    }

    let start = subcmd_idx? + 1;
    // Iterate positional arguments after the subcommand (skipping flags)
    let mut positionals = Vec::new();
    let mut skip_next = false;
    for word in &words[start..] {
        if skip_next {
            skip_next = false;
            continue;
        }
        if word.starts_with('-') {
            // Options that take an argument
            if word == "-o"
                || word == "--output"
                || word == "--on-overlap"
                || word == "--source"
                || word == "--pack"
                || word == "--com-mojang"
            {
                skip_next = true;
            }
            continue;
        }
        positionals.push(word.clone());
    }

    match subcmd {
        // export <world> <structures...>
        // delete <world> <structures...>
        "export" | "delete" => positionals.first().cloned(),
        // copy <src_world> <dst_world> <structures...> (structures come from src_world)
        "copy" => positionals.first().cloned(),
        _ => None,
    }
}

/// Get the words passed to the completion invocation.
///
/// Under `clap_complete`, the command words being completed are passed after `--` in `argv`.
/// If no `--` is found (e.g. during testing), falls back to `std::env::args()`.
fn get_command_words() -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--") {
        args[pos + 1..].to_vec()
    } else {
        args
    }
}
