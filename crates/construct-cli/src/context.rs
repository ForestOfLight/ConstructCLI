use crate::cli::Cli;
use crate::failure;
use crate::output::Out;
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::{Result, config, discovery};
use std::path::{Path, PathBuf};

pub struct Discovered {
    pub installations: Vec<Installation>,
    pub worlds: Vec<World>,
    pub probed: Vec<PathBuf>,
}

pub fn discover(settings: &config::Config, extra_paths: &[PathBuf]) -> Discovered {
    let mut extra_roots: Vec<(String, PathBuf)> = settings
        .roots
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();
    let mut extra_worlds: Vec<PathBuf> = settings.other_worlds.clone();
    let mut flag_roots = 0;
    for path in extra_paths {
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
    let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
    let localappdata = std::env::var("LOCALAPPDATA").ok().map(PathBuf::from);

    let mut candidates = discovery::platform::candidates(
        Path::new(&home),
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

    let probed: Vec<PathBuf> = candidates.iter().map(|c| c.dev_pack_root.clone()).collect();
    debug_assert!(
        settings
            .roots
            .iter()
            .all(|r| candidates.iter().any(|c| c.name == r.name)),
        "a configured root lost its name before discovery"
    );

    let installations = discovery::platform::resolve(candidates);
    let worlds = discovery::enumerate(&installations, &extra_worlds);
    Discovered {
        installations,
        worlds,
        probed,
    }
}

pub struct Context {
    pub installations: Vec<Installation>,
    pub worlds: Vec<World>,
    pub settings: config::Config,
    pub config_flag: Option<PathBuf>,
    probed: Vec<PathBuf>,
}

impl Context {
    pub fn build(cli: &Cli, out: &mut Out) -> Result<Self> {
        let loaded = config::load(cli.config.as_deref(), &|k| std::env::var(k).ok())?;
        for w in &loaded.warnings {
            out.warn(w.clone());
        }
        let found = discover(&loaded.config, cli.command.paths());
        Ok(Self {
            installations: found.installations,
            worlds: found.worlds,
            settings: loaded.config,
            config_flag: cli.config.clone(),
            probed: found.probed,
        })
    }

    pub fn nothing_to_search(&self) -> bool {
        self.installations.is_empty() && self.worlds.is_empty()
    }

    pub fn no_installations(&self) -> CoreError {
        CoreError::NoInstallations {
            probed: self.probed.clone(),
        }
    }

    pub fn world(&self, reference: &str) -> Result<World> {
        discovery::reference::resolve(reference, &self.worlds).map_err(|e| {
            if self.nothing_to_search() && matches!(e, CoreError::WorldNotFound { .. }) {
                self.no_installations()
            } else {
                e
            }
        })
    }

    pub fn installation(&self) -> Result<&Installation> {
        discovery::installation::choose(
            &self.installations,
            self.settings.default_installation.as_deref(),
        )
    }

    pub fn installation_for(&self, world: Option<&World>) -> Result<&Installation> {
        match world {
            Some(w) => discovery::installation::for_world(&self.installations, w),
            None => self.installation(),
        }
    }

    pub fn optional_world(&self, reference: Option<&str>) -> failure::Result<Option<World>> {
        Ok(reference.map(|r| self.world(r)).transpose()?)
    }
}
