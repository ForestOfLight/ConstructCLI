//! The GitHub releases lookup behind a trait, so install is fully mockable.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::Path;

pub const REPO: &str = "ForestOfLight/Construct";
/// The only release source a shipped build can reach. Fixed rather than
/// configurable: see [`base`] for the one exception and why it cannot survive
/// into a released binary.
pub const API_BASE: &str = "https://api.github.com";
/// GitHub's unauthenticated limit, named in the error §11 asks for.
pub const UNAUTHENTICATED_LIMIT: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

pub trait Releases {
    /// `None` means the latest release.
    fn release(&self, version: Option<&str>) -> Result<Release>;
    fn download(&self, asset: &Asset, to: &Path) -> Result<u64>;
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
}

pub fn parse_release(json: &str) -> Result<Release> {
    let raw: RawRelease = serde_json::from_str(json).map_err(|e| CoreError::Network {
        reason: format!("unexpected response from GitHub: {e}"),
    })?;
    Ok(Release {
        tag: raw.tag_name,
        assets: raw
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
                size: a.size,
            })
            .collect(),
    })
}

/// `1.2.0` and `v1.2.0` both name the same tag.
pub fn tag_for(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

/// The `.mcaddon`, matched by pattern rather than position.
pub fn asset_for(release: &Release) -> Result<&Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.starts_with("Construct-v") && a.name.ends_with(".mcaddon"))
        .ok_or_else(|| CoreError::AssetNotFound {
            version: release.tag.clone(),
            available: release.assets.iter().map(|a| a.name.clone()).collect(),
        })
}

pub struct GitHub {
    base: String,
    token: Option<String>,
}

/// [`API_BASE`], unless a test has pointed the client somewhere else.
///
/// `CONSTRUCT_GITHUB_API` lets the CLI's integration tests serve canned
/// responses without reaching the network — several of them assert on
/// conditions the real endpoint cannot produce, such as an asset whose
/// declared size disagrees with the bytes served.
///
/// The override is compiled out unless the crate is built with `test-hooks`,
/// which only construct-cli's `[dev-dependencies]` turns on. A shipped binary
/// comes from `cargo build --release`, which pulls no dev-dependencies, so
/// nothing at runtime can redirect it away from [`API_BASE`] — which is the
/// point. Gating on the feature rather than on `debug_assertions` keeps that
/// true while letting the same tests run under `cargo test --release`.
fn base() -> String {
    #[cfg(feature = "test-hooks")]
    if let Ok(base) = std::env::var("CONSTRUCT_GITHUB_API") {
        return base;
    }
    API_BASE.to_string()
}

impl GitHub {
    pub fn new(token: Option<String>) -> Self {
        Self {
            base: base(),
            token,
        }
    }
}

impl Releases for GitHub {
    fn release(&self, version: Option<&str>) -> Result<Release> {
        let url = match version {
            Some(v) => format!("{}/repos/{REPO}/releases/tags/{}", self.base, tag_for(v)),
            None => format!("{}/repos/{REPO}/releases/latest", self.base),
        };
        let mut req = ureq::get(&url).header("User-Agent", "ConstructCLI");
        if let Some(token) = &self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = req
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(map_transport)?;
        let status = response.status().as_u16();
        if matches!(status, 403 | 429)
            && response
                .headers()
                .get("x-ratelimit-remaining")
                .map(|v| v == "0")
                .unwrap_or(false)
        {
            return Err(CoreError::RateLimited);
        }
        if status == 404 {
            return Err(CoreError::AssetNotFound {
                version: version.unwrap_or("latest").to_string(),
                available: Vec::new(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(CoreError::Network {
                reason: format!("GitHub returned HTTP {status}"),
            });
        }
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(map_transport)?;
        parse_release(&body)
    }

    fn download(&self, asset: &Asset, to: &Path) -> Result<u64> {
        let mut response = ureq::get(&asset.url)
            .header("User-Agent", "ConstructCLI")
            .call()
            .map_err(map_transport)?;
        let mut reader = response.body_mut().as_reader();
        let mut file = std::fs::File::create(to)?;
        let written = std::io::copy(&mut reader, &mut file)?;
        if written != asset.size {
            return Err(CoreError::Network {
                reason: format!(
                    "download truncated: expected {} bytes, got {written}",
                    asset.size
                ),
            });
        }
        Ok(written)
    }
}

fn map_transport(e: impl std::fmt::Display) -> CoreError {
    CoreError::Network {
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST: &str = r#"{
        "tag_name": "v1.2.0",
        "name": "v1.2.0 for MC 26.40",
        "assets": [
            { "name": "Construct-v1.2.0.mcaddon",
              "size": 1234238,
              "content_type": "application/octet-stream",
              "browser_download_url": "https://github.com/ForestOfLight/Construct/releases/download/v1.2.0/Construct-v1.2.0.mcaddon" }
        ]
    }"#;

    #[test]
    fn parses_a_release_and_its_asset() {
        let r = parse_release(LATEST).unwrap();
        assert_eq!(r.tag, "v1.2.0");
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].size, 1234238);
        assert!(r.assets[0].url.ends_with("Construct-v1.2.0.mcaddon"));
    }

    #[test]
    fn the_mcaddon_asset_is_matched_by_pattern_not_position() {
        let r = Release {
            tag: "v9.9.9".into(),
            assets: vec![
                Asset {
                    name: "sha256sums.txt".into(),
                    url: "u1".into(),
                    size: 1,
                },
                Asset {
                    name: "Construct-v9.9.9.mcaddon".into(),
                    url: "u2".into(),
                    size: 2,
                },
            ],
        };
        assert_eq!(asset_for(&r).unwrap().url, "u2");
    }

    #[test]
    fn a_release_with_no_mcaddon_lists_what_it_did_have() {
        let r = Release {
            tag: "v9.9.9".into(),
            assets: vec![Asset {
                name: "notes.txt".into(),
                url: "u".into(),
                size: 1,
            }],
        };
        let CoreError::AssetNotFound { available, .. } = asset_for(&r).unwrap_err() else {
            panic!("expected AssetNotFound");
        };
        assert_eq!(available, vec!["notes.txt".to_string()]);
    }

    #[test]
    fn a_version_is_accepted_with_or_without_its_v() {
        assert_eq!(tag_for("1.2.0"), "v1.2.0");
        assert_eq!(tag_for("v1.2.0"), "v1.2.0");
    }

    #[test]
    fn malformed_json_is_a_network_error_not_a_panic() {
        assert!(matches!(
            parse_release("{ nope"),
            Err(CoreError::Network { .. })
        ));
    }

    /// Everything above this line runs against canned bytes, and so proves the
    /// client handles the shape we *believe* GitHub returns. This one asks the
    /// real endpoint whether that belief still holds — the stubs in
    /// construct-cli's tests are modelled on it, and would keep passing in
    /// perfect ignorance if GitHub changed the payload under them.
    ///
    /// `#[ignore]`d because it needs the network and spends one of the 60
    /// unauthenticated requests an IP gets per hour. It asserts only on what
    /// the client actually depends on, so a new release does not break it:
    ///
    /// ```
    /// cargo test -p construct-core --lib live_github -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reaches the real api.github.com"]
    fn live_github_still_returns_the_shape_the_stubs_assume() {
        let release = GitHub::new(
            std::env::var("CONSTRUCT_GITHUB_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .ok(),
        )
        .release(None)
        .expect("the latest Construct release must resolve");

        assert!(
            release.tag.starts_with('v'),
            "install compares against a v-prefixed tag; got {:?}",
            release.tag
        );

        let asset = asset_for(&release).expect("the release must carry an .mcaddon");
        assert!(
            asset.size > 0,
            "install checks the download against this size; a 0 would wave anything through"
        );
        assert!(
            asset.url.starts_with("https://"),
            "the download url must be https; got {:?}",
            asset.url
        );

        println!(
            "live: {} -> {} ({} bytes)",
            release.tag, asset.name, asset.size
        );
    }
}
