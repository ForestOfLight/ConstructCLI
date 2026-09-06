//! Configuration loading.
//!
//! Precedence is CLI flag → environment variable → config file →
//! auto-discovery. An absent file means all defaults, so there is no init step.
//! Unknown keys warn rather than fail.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize)]
pub struct Config {
    pub default_installation: Option<String>,
    pub roots: Vec<ExtraRoot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_worlds: Vec<PathBuf>,
    pub backups: Backups,
    #[serde(flatten, skip_serializing_if = "toml::Table::is_empty")]
    unknown: toml::Table,
}

/// An extra `com.mojang` root to probe. Named, because an unnamed root cannot
/// appear in the `<installation>/<account>/<world>` grammar.
#[derive(Debug, Clone, Serialize)]
pub struct ExtraRoot {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct Backups {
    pub dir: Option<PathBuf>,
    pub keep: usize,
}

impl Default for Backups {
    fn default() -> Self {
        Self {
            dir: None,
            keep: DEFAULT_KEEP,
        }
    }
}

/// Retention default: the last ten snapshots per world.
pub const DEFAULT_KEEP: usize = 10;

/// Reserved installation/root names that cannot be used for extra roots.
/// These come from:
/// - `release`, `preview`, `legacy`, `mcpelauncher`: built-in installations (discovery layer, Task 6)
/// - `path`: reserved for filesystem-path references (Task 6), and worn by
///   worlds named via `--path` — see `discovery::PATH_INSTALLATION`
/// - `env`: reserved for the CONSTRUCT_COM_MOJANG environment variable root
const RESERVED_NAMES: &[&str] = &[
    "release",
    "preview",
    "legacy",
    "mcpelauncher",
    crate::discovery::PATH_INSTALLATION,
    "env",
];

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
    pub source: Option<PathBuf>,
}

// The wire form. Separate from `Config` so unknown keys can be collected as
// warnings instead of aborting the load.
#[derive(Deserialize)]
struct WireConfig {
    default_installation: Option<String>,
    #[serde(default)]
    roots: Vec<WireRoot>,
    #[serde(default)]
    other_worlds: Vec<PathBuf>,
    backups: Option<WireBackups>,
    #[serde(flatten)]
    unknown: toml::Table,
}

#[derive(Deserialize)]
struct WireRoot {
    name: Option<String>,
    path: PathBuf,
}

#[derive(Deserialize)]
struct WireBackups {
    dir: Option<PathBuf>,
    keep: Option<usize>,
}

/// `~/.config/constructcli/config.toml` and platform equivalents.
pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "constructcli")
        .map(|d| d.config_dir().join("config.toml"))
}

pub fn parse(text: &str, path: &Path) -> Result<(Config, Vec<String>)> {
    let bad = |reason: String| CoreError::BadConfig {
        path: path.to_path_buf(),
        reason,
    };

    let wire: WireConfig = toml::from_str(text).map_err(|e| bad(e.to_string()))?;

    let mut warnings = Vec::new();
    for key in wire.unknown.keys() {
        warnings.push(format!("unknown config key `{key}` in {}", path.display()));
    }

    let mut roots = Vec::new();
    for r in wire.roots {
        let name = r.name.ok_or_else(|| {
            bad("every [[roots]] entry needs a `name`; an unnamed root cannot be addressed in a qualified reference".to_string())
        })?;

        // Check if name is reserved
        if RESERVED_NAMES.contains(&name.as_str()) {
            return Err(bad(format!(
                "root name `{name}` is reserved and cannot be used in a config file"
            )));
        }

        // Check for duplicate names
        if roots.iter().any(|r: &ExtraRoot| r.name == name) {
            return Err(bad(format!(
                "duplicate root name `{name}`; each root must have a unique name"
            )));
        }

        roots.push(ExtraRoot { name, path: r.path });
    }

    let backups = wire.backups.map_or_else(Backups::default, |b| Backups {
        dir: b.dir,
        keep: b.keep.unwrap_or(DEFAULT_KEEP),
    });

    Ok((
        Config {
            default_installation: wire.default_installation,
            roots,
            other_worlds: wire.other_worlds,
            backups,
            unknown: wire.unknown,
        },
        warnings,
    ))
}

/// The effective config location before any config contents are read.
pub fn path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("CONSTRUCT_CONFIG")
        .map(PathBuf::from)
        .or_else(default_path)
}

/// Loads only the config file, without applying environment overrides.
pub fn load_file(path: &Path) -> Result<(Config, Vec<String>)> {
    if !path.is_file() {
        return Ok((Config::default(), Vec::new()));
    }
    parse(&std::fs::read_to_string(path)?, path)
}

/// Saves a config, creating its parent directory when necessary.
pub fn save(path: &Path, config: &Config) -> Result<()> {
    let text = toml::to_string_pretty(config).map_err(|e| CoreError::BadConfig {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddedPath {
    Root,
    OtherWorld,
}

/// Adds a directory to the appropriate persistent path list.
pub fn add_path(config: &mut Config, path: &Path) -> Result<(AddedPath, bool)> {
    if !path.is_dir() {
        return Err(CoreError::InvalidPath {
            path: path.to_path_buf(),
            reason: "expected a directory".to_string(),
        });
    }
    let path = std::fs::canonicalize(path)?;

    if path.file_name().is_some_and(|name| name == "com.mojang") {
        if config.roots.iter().any(|root| root.path == path) {
            return Ok((AddedPath::Root, false));
        }
        let mut number = 1;
        let name = loop {
            let name = format!("root{number}");
            if !config.roots.iter().any(|root| root.name == name) {
                break name;
            }
            number += 1;
        };
        config.roots.push(ExtraRoot { name, path });
        return Ok((AddedPath::Root, true));
    }

    if path.join("level.dat").is_file() {
        let added = !config.other_worlds.contains(&path);
        if added {
            config.other_worlds.push(path);
        }
        return Ok((AddedPath::OtherWorld, added));
    }

    Err(CoreError::InvalidPath {
        path,
        reason: "expected a com.mojang directory or a world directory containing level.dat"
            .to_string(),
    })
}

/// Loads config, then applies environment overrides on top.
pub fn load(explicit: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded> {
    let path = explicit
        .map(Path::to_path_buf)
        .or_else(|| self::path(env));

    let (mut config, mut warnings, source) = match &path {
        Some(p) if p.is_file() => {
            let (c, w) = load_file(p)?;
            (c, w, Some(p.clone()))
        }
        // An absent file is not an error.
        _ => (Config::default(), Vec::new(), None),
    };

    if let Some(v) = env("CONSTRUCT_INSTALLATION") {
        config.default_installation = Some(v);
    }
    if let Some(v) = env("CONSTRUCT_COM_MOJANG") {
        config.roots.push(ExtraRoot {
            name: "env".to_string(),
            path: PathBuf::from(v),
        });
    }

    warnings.shrink_to_fit();
    Ok(Loaded {
        config,
        warnings,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn an_absent_file_yields_defaults_with_no_error() {
        // There is no init step; absent config must simply mean all defaults.
        let loaded = load(Some(Path::new("/nonexistent/config.toml")), &no_env).unwrap();
        assert_eq!(loaded.config.default_installation, None);
        assert!(loaded.config.roots.is_empty());
        assert_eq!(loaded.config.backups.keep, 10);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn keep_defaults_to_ten_when_the_file_omits_it() {
        let (c, _) = parse("[backups]\ndir = \"/tmp/b\"\n", Path::new("c.toml")).unwrap();
        assert_eq!(c.backups.keep, 10);
        assert_eq!(c.backups.dir, Some(PathBuf::from("/tmp/b")));
    }

    #[test]
    fn parses_a_full_config() {
        let text = r#"
default_installation = "release"

[[roots]]
name = "backup"
path = "D:/MinecraftBackups/com.mojang"

[backups]
dir = "/Volumes/Spare/construct-backups"
keep = 3
"#;
        let (c, warnings) = parse(text, Path::new("c.toml")).unwrap();
        assert_eq!(c.default_installation.as_deref(), Some("release"));
        assert_eq!(c.roots.len(), 1);
        assert_eq!(c.roots[0].name, "backup");
        assert_eq!(c.backups.keep, 3);
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_keys_warn_rather_than_fail() {
        let (c, warnings) = parse(
            "nonsense = 1\ndefault_installation = \"release\"\n",
            Path::new("c.toml"),
        )
        .unwrap();
        assert_eq!(c.default_installation.as_deref(), Some("release"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("nonsense"),
            "warning should name the key: {}",
            warnings[0]
        );
    }

    #[test]
    fn saving_preserves_unknown_keys() {
        let (config, _) = parse("future_setting = true\n", Path::new("c.toml")).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        save(&path, &config).unwrap();
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("future_setting = true"));
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(matches!(
            parse("this is not toml [[[", Path::new("c.toml")),
            Err(CoreError::BadConfig { .. })
        ));
    }

    #[test]
    fn an_extra_root_must_be_named() {
        // An unnamed root cannot appear in the installation/account/world grammar.
        assert!(matches!(
            parse("[[roots]]\npath = \"/tmp/x\"\n", Path::new("c.toml")),
            Err(CoreError::BadConfig { .. })
        ));
    }

    #[test]
    fn the_env_var_overrides_the_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "default_installation = \"release\"\n").unwrap();

        let env = |k: &str| (k == "CONSTRUCT_INSTALLATION").then(|| "preview".to_string());
        let loaded = load(Some(&path), &env).unwrap();
        assert_eq!(
            loaded.config.default_installation.as_deref(),
            Some("preview")
        );
    }

    #[test]
    fn construct_com_mojang_env_var_becomes_an_extra_root() {
        let env = |k: &str| (k == "CONSTRUCT_COM_MOJANG").then(|| "/tmp/env-root".to_string());
        let loaded = load(Some(Path::new("/nonexistent")), &env).unwrap();
        assert_eq!(loaded.config.roots.len(), 1);
        assert_eq!(loaded.config.roots[0].path, PathBuf::from("/tmp/env-root"));
        assert_eq!(loaded.config.roots[0].name, "env");
    }

    #[test]
    fn two_roots_with_the_same_name_are_rejected() {
        let text = r#"
[[roots]]
name = "backup"
path = "/tmp/backup1"

[[roots]]
name = "backup"
path = "/tmp/backup2"
"#;
        let result = parse(text, Path::new("c.toml"));
        assert!(matches!(result, Err(CoreError::BadConfig { .. })));
        if let Err(CoreError::BadConfig { reason, .. }) = result {
            assert!(
                reason.contains("duplicate") && reason.contains("backup"),
                "error should mention duplicate and name: {}",
                reason
            );
        }
    }

    #[test]
    fn a_root_named_release_is_rejected_as_reserved() {
        let text = r#"
[[roots]]
name = "release"
path = "/tmp/x"
"#;
        let result = parse(text, Path::new("c.toml"));
        assert!(matches!(result, Err(CoreError::BadConfig { .. })));
        if let Err(CoreError::BadConfig { reason, .. }) = result {
            assert!(
                reason.contains("reserved") && reason.contains("release"),
                "error should mention reserved and release: {}",
                reason
            );
        }
    }

    #[test]
    fn a_root_named_path_is_rejected_as_reserved() {
        let text = r#"
[[roots]]
name = "path"
path = "/tmp/x"
"#;
        let result = parse(text, Path::new("c.toml"));
        assert!(matches!(result, Err(CoreError::BadConfig { .. })));
        if let Err(CoreError::BadConfig { reason, .. }) = result {
            assert!(
                reason.contains("reserved") && reason.contains("path"),
                "error should mention reserved and path: {}",
                reason
            );
        }
    }

    #[test]
    fn a_root_named_env_is_rejected_as_reserved() {
        let text = r#"
[[roots]]
name = "env"
path = "/tmp/x"
"#;
        let result = parse(text, Path::new("c.toml"));
        assert!(matches!(result, Err(CoreError::BadConfig { .. })));
        if let Err(CoreError::BadConfig { reason, .. }) = result {
            assert!(
                reason.contains("reserved") && reason.contains("env"),
                "error should mention reserved and env: {}",
                reason
            );
        }
    }

    #[test]
    fn a_valid_config_with_two_differently_named_roots_parses_cleanly() {
        let text = r#"
[[roots]]
name = "backup"
path = "/tmp/backup"

[[roots]]
name = "external"
path = "/tmp/external"
"#;
        let (c, warnings) = parse(text, Path::new("c.toml")).unwrap();
        assert_eq!(c.roots.len(), 2);
        assert_eq!(c.roots[0].name, "backup");
        assert_eq!(c.roots[1].name, "external");
        assert!(warnings.is_empty());
    }
}
