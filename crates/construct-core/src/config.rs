//! Configuration loading.
//!
//! Precedence is CLI flag → environment variable → config file →
//! auto-discovery. An absent file means all defaults, so there is no init step.
//! Unknown keys warn rather than fail.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub default_installation: Option<String>,
    pub roots: Vec<ExtraRoot>,
    pub backups: Backups,
}

/// An extra `com.mojang` root to probe. Named, because an unnamed root cannot
/// appear in the `<installation>/<account>/<world>` grammar.
#[derive(Debug, Clone)]
pub struct ExtraRoot {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
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
            backups,
        },
        warnings,
    ))
}

/// Loads config, then applies environment overrides on top.
pub fn load(explicit: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded> {
    let path = explicit
        .map(Path::to_path_buf)
        .or_else(|| env("CONSTRUCT_CONFIG").map(PathBuf::from))
        .or_else(default_path);

    let (mut config, mut warnings, source) = match &path {
        Some(p) if p.is_file() => {
            let text = std::fs::read_to_string(p)?;
            let (c, w) = parse(&text, p)?;
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
    }
}
