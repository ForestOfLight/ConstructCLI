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

/// Complete structure names for whatever the command line is pointing at.
///
/// The names on offer are the ones the command could actually go on to use.
/// `export`/`delete` narrow to one world under `--world` and to the shared copy
/// of Construct without it, so completion narrows the same way — a name the
/// command would then refuse is worse than no suggestion at all.
pub fn complete_structures() -> Vec<CompletionCandidate> {
    let (installations, worlds) = discover_environment();
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |name: String, help: &'static str| {
        if seen.insert(name.clone()) {
            candidates.push(CompletionCandidate::new(name).help(Some(StyledStr::from(help))));
        }
    };

    match extract_target_from_args() {
        Target::None => return Vec::new(),
        Target::Shared => {
            let Some(inst) = choose_installation(&installations) else {
                return Vec::new();
            };
            let Ok(home) = pack::for_installation(inst) else {
                return Vec::new();
            };
            for entry in catalog::from_pack(&home.pack.dir, pack::Scope::Shared) {
                push(entry.name, "shared Construct structure");
            }
        }
        Target::World { reference, scoped } => {
            let Ok(world) = discovery::reference::resolve(&reference, &worlds) else {
                return Vec::new();
            };

            // 1. World database structures
            if let Ok(store) = store::open_world_store(&world)
                && let Ok(entries) = catalog::from_world(&store)
            {
                for entry in entries {
                    push(entry.name, "world database structure");
                }
            }

            // 2. Pack structures
            if let Ok(inst) = discovery::installation::for_world(&installations, &world) {
                for home in pack::serving(&world, inst) {
                    // A world-scoped command cannot reach the shared copy, so
                    // its names are not on offer for one.
                    if scoped && home.kind.scope() == pack::Scope::Shared {
                        continue;
                    }
                    let scope_help = match home.kind.scope() {
                        pack::Scope::World => "world pack structure",
                        pack::Scope::Shared => "shared Construct structure",
                    };
                    for entry in catalog::from_pack(&home.dir, home.kind.scope()) {
                        push(entry.name, scope_help);
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| a.get_value().cmp(b.get_value()));
    candidates
}

/// The installation a command with no world named would resolve, by the same
/// precedence `main.rs` uses. Completion must not exit, so every failure here
/// is simply "no suggestions".
fn choose_installation(installations: &[Installation]) -> Option<&Installation> {
    let loaded = config::load(None, &|k| std::env::var(k).ok()).ok();
    discovery::installation::choose(
        installations,
        std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
        loaded
            .as_ref()
            .and_then(|l| l.config.default_installation.as_deref()),
    )
    .ok()
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

/// Which structures the word being completed could name.
enum Target {
    World {
        reference: String,
        /// `true` when the command reads that world and nothing else, so the
        /// shared copy of Construct is not on offer. `export` and `delete` under
        /// `--world`; never `copy`, whose source world sees both packs and has
        /// `--pack` to choose between them.
        scoped: bool,
    },
    /// `export`/`delete` with no `--world`: the shared copy of Construct.
    Shared,
    /// Nothing to complete from — an unrecognised command, or `copy` before its
    /// source world has been typed.
    None,
}

/// Work out what the active command line is asking for structures from.
fn extract_target_from_args() -> Target {
    let words = get_command_words();
    if words.is_empty() {
        return Target::None;
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

    let Some(idx) = subcmd_idx else {
        return Target::None;
    };
    let start = idx + 1;
    // Iterate positional arguments after the subcommand (skipping flags), and
    // pick up `--world`'s value on the way — for `export` and `delete` the
    // world is a flag, not a positional.
    let mut positionals = Vec::new();
    let mut world_flag: Option<String> = None;
    let mut expect_world = false;
    let mut skip_next = false;
    for word in &words[start..] {
        if skip_next {
            skip_next = false;
            continue;
        }
        if expect_world {
            expect_world = false;
            // The word being completed is empty; treat it as "not typed yet".
            if !word.is_empty() {
                world_flag = Some(word.clone());
            }
            continue;
        }
        if let Some(value) = word
            .strip_prefix("--world=")
            .or_else(|| word.strip_prefix("-w="))
        {
            world_flag = Some(value.to_string());
            continue;
        }
        if word.starts_with('-') {
            if word == "--world" || word == "-w" {
                expect_world = true;
            } else if word == "-o"
                || word == "--output"
                || word == "--on-overlap"
                || word == "--source"
                || word == "--pack"
                || word == "--com-mojang"
            {
                // Options that take an argument
                skip_next = true;
            }
            continue;
        }
        positionals.push(word.clone());
    }

    match subcmd {
        // export <structures...> [--world W]
        // delete <structures...> [--world W]
        "export" | "delete" => match world_flag {
            Some(reference) => Target::World {
                reference,
                scoped: true,
            },
            None => Target::Shared,
        },
        // copy <src_world> <dst_world> <structures...> (structures come from src_world)
        "copy" => match positionals.first() {
            Some(reference) => Target::World {
                reference: reference.clone(),
                scoped: false,
            },
            None => Target::None,
        },
        _ => Target::None,
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
