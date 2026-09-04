//! Which Minecraft, when no world names one.
//!
//! §10's rule, and the same never-guess rule §6 applies to world references:
//! an explicit request, failing that the configured default, failing that the
//! sole installation, failing that an error listing the candidates.

use crate::discovery::{Installation, World};
use crate::error::{CoreError, Result};

pub fn choose<'a>(
    installations: &'a [Installation],
    requested: Option<&str>,
    default: Option<&str>,
) -> Result<&'a Installation> {
    if installations.is_empty() {
        return Err(CoreError::NoInstallations { probed: Vec::new() });
    }
    let names = || {
        installations
            .iter()
            .map(|i| i.name.clone())
            .collect::<Vec<_>>()
    };

    // A name that was asked for and does not exist is an error even when there
    // is only one installation: silently using it would ignore the request.
    if let Some(name) = requested.or(default) {
        return installations
            .iter()
            .find(|i| i.name == name)
            .ok_or_else(|| CoreError::InstallationNotFound {
                name: name.to_string(),
                available: names(),
            });
    }
    match installations {
        [only] => Ok(only),
        _ => Err(CoreError::AmbiguousInstallation {
            candidates: names(),
        }),
    }
}

/// The installation a world belongs to.
///
/// `install --world W` deploys into *that* installation's dev-pack root, so
/// `preview` and `release` are never mixed (§6).
pub fn for_world<'a>(installations: &'a [Installation], world: &World) -> Result<&'a Installation> {
    installations
        .iter()
        .find(|i| i.name == world.installation)
        .ok_or_else(|| CoreError::InstallationNotFound {
            name: world.installation.clone(),
            available: installations.iter().map(|i| i.name.clone()).collect(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inst(name: &str) -> Installation {
        Installation {
            name: name.to_string(),
            dev_pack_root: PathBuf::from(format!("/{name}")),
            world_roots: Vec::new(),
        }
    }

    #[test]
    fn a_sole_installation_needs_no_configuration() {
        let all = vec![inst("mcpelauncher")];
        assert_eq!(choose(&all, None, None).unwrap().name, "mcpelauncher");
    }

    #[test]
    fn two_installations_and_no_default_is_ambiguous_not_a_guess() {
        let all = vec![inst("release"), inst("preview")];
        let CoreError::AmbiguousInstallation { candidates } = choose(&all, None, None).unwrap_err()
        else {
            panic!("expected AmbiguousInstallation");
        };
        assert_eq!(
            candidates,
            vec!["release".to_string(), "preview".to_string()]
        );
    }

    #[test]
    fn the_configured_default_settles_it() {
        let all = vec![inst("release"), inst("preview")];
        assert_eq!(choose(&all, None, Some("preview")).unwrap().name, "preview");
    }

    #[test]
    fn an_explicit_request_beats_the_configured_default() {
        let all = vec![inst("release"), inst("preview")];
        assert_eq!(
            choose(&all, Some("release"), Some("preview")).unwrap().name,
            "release"
        );
    }

    #[test]
    fn a_request_naming_nothing_is_an_error_listing_what_exists() {
        let all = vec![inst("release")];
        let CoreError::InstallationNotFound { name, available } =
            choose(&all, Some("nope"), None).unwrap_err()
        else {
            panic!("expected InstallationNotFound");
        };
        assert_eq!(name, "nope");
        assert_eq!(available, vec!["release".to_string()]);
    }

    #[test]
    fn a_stale_default_pointing_at_nothing_is_an_error_not_a_silent_fallback() {
        // Falling back to the sole installation would quietly ignore what the
        // user configured, which is exactly the "never guess" case.
        let all = vec![inst("release")];
        assert!(matches!(
            choose(&all, None, Some("preview")),
            Err(CoreError::InstallationNotFound { .. })
        ));
    }

    #[test]
    fn no_installations_at_all_says_so() {
        assert!(matches!(
            choose(&[], None, None),
            Err(CoreError::NoInstallations { .. })
        ));
    }
}
