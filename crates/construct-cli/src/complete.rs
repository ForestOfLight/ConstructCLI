use crate::context;
use clap::builder::StyledStr;
use clap_complete::engine::CompletionCandidate;
use construct_core::catalog::{self, Source};
use construct_core::config;
use construct_core::discovery::{self, Installation, World};
use construct_core::pack;
use construct_core::store;
use std::path::PathBuf;

pub fn complete_worlds() -> Vec<CompletionCandidate> {
    let (installations, worlds) = discover_environment();
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for w in &worlds {
        if seen.insert(w.display_name.clone()) {
            let help_text = format!("{} ({})", w.installation, w.folder);
            candidates.push(
                CompletionCandidate::new(&w.display_name).help(Some(StyledStr::from(help_text))),
            );
        }

        if w.folder != w.display_name && seen.insert(w.folder.clone()) {
            let help_folder = format!("{} ({})", w.installation, w.display_name);
            candidates
                .push(CompletionCandidate::new(&w.folder).help(Some(StyledStr::from(help_folder))));
        }

        if (installations.len() > 1 || w.account.is_some()) && seen.insert(w.qualified()) {
            let qualified = w.qualified();
            let help_qual = format!("{} - {}", w.installation, w.display_name);
            candidates
                .push(CompletionCandidate::new(qualified).help(Some(StyledStr::from(help_qual))));
        }
    }

    candidates
}

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
            for entry in catalog::from_pack(&home.pack.dir, Source::SharedPack) {
                push(entry.name, "shared Construct structure");
            }
        }
        Target::World { reference, scoped } => {
            let Ok(world) = discovery::reference::resolve(&reference, &worlds) else {
                return Vec::new();
            };

            if let Ok(store) = store::open_world_store(&world)
                && let Ok(entries) = catalog::from_world(&store)
            {
                for entry in entries {
                    push(entry.name, "world database structure");
                }
            }

            if let Ok(inst) = discovery::installation::for_world(&installations, &world) {
                for home in pack::serving(&world, inst) {
                    let source = home.kind.source();
                    if scoped && source == Source::SharedPack {
                        continue;
                    }
                    let help = match source {
                        Source::SharedPack => "shared Construct structure",
                        _ => "world pack structure",
                    };
                    for entry in catalog::from_pack(&home.dir, source) {
                        push(entry.name, help);
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| a.get_value().cmp(b.get_value()));
    candidates
}

fn choose_installation(installations: &[Installation]) -> Option<&Installation> {
    discovery::installation::choose(
        installations,
        loaded_config().default_installation.as_deref(),
    )
    .ok()
}

fn discover_environment() -> (Vec<Installation>, Vec<World>) {
    let found = context::discover(&loaded_config(), &extract_paths_from_args());
    (found.installations, found.worlds)
}

fn loaded_config() -> config::Config {
    config::load(extract_config_from_args().as_deref(), &|k| {
        std::env::var(k).ok()
    })
    .map(|l| l.config)
    .unwrap_or_default()
}

fn extract_config_from_args() -> Option<PathBuf> {
    let args: Vec<String> = get_command_words();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            return Some(PathBuf::from(&args[i + 1]));
        }
        if let Some(stripped) = args[i].strip_prefix("--config=") {
            return Some(PathBuf::from(stripped));
        }
        i += 1;
    }
    None
}

fn extract_paths_from_args() -> Vec<PathBuf> {
    let args: Vec<String> = get_command_words();
    let mut paths = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--path" && i + 1 < args.len() {
            paths.push(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if let Some(stripped) = args[i].strip_prefix("--path=") {
            paths.push(PathBuf::from(stripped));
            i += 1;
        } else {
            i += 1;
        }
    }
    paths
}

enum Target {
    World { reference: String, scoped: bool },
    Shared,
    None,
}

fn extract_target_from_args() -> Target {
    let words = get_command_words();
    if words.is_empty() {
        return Target::None;
    }

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
            } else if word == "-n"
                || word == "--name"
                || word == "--on-overlap"
                || word == "--source"
                || word == "--path"
            {
                skip_next = true;
            }
            continue;
        }
        positionals.push(word.clone());
    }

    match subcmd {
        "export" | "delete" => match world_flag {
            Some(reference) => Target::World {
                reference,
                scoped: true,
            },
            None => Target::Shared,
        },
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

fn get_command_words() -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--") {
        args[pos + 1..].to_vec()
    } else {
        args
    }
}
