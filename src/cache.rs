use std::fs;
use std::path::Path;

use basic_toml as toml;
use log::debug;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct RequestCacheWrite<'a> {
    pub headers: Vec<(&'a str, &'a str)>,
    pub version: &'a str,
    pub config_hash: &'a str,
}

#[derive(Debug, Deserialize)]
struct RequestCacheRead {
    headers: Vec<(String, String)>,
    /// The version of rsspls that created this request cache
    ///
    /// May be missing if the cache was created by an older version.
    #[serde(default)]
    version: Option<String>,
    /// Hash of the config
    ///
    /// Used as cache buster when config changes.
    ///
    /// May be missing if the cache was created by an older version.
    #[serde(default)]
    config_hash: Option<String>,
}

pub fn deserialise_cached_headers(
    path: &Path,
    config_hash: &str,
) -> Option<HeaderMap<HeaderValue>> {
    let raw = fs::read(path).ok()?;
    let cache: RequestCacheRead = toml::from_slice(&raw).ok()?;

    if cache.version.as_deref() != Some(crate::version()) {
        debug!(
            "cache version ({:?}) != to this version ({:?}), ignoring cache at: {}",
            cache.version,
            crate::version(),
            path.display()
        );
        return None;
    } else if cache.config_hash.as_deref() != Some(config_hash) {
        debug!(
            "cache config hash mismatch ({:?}) != ({:?}), ignoring cache at: {}",
            cache.config_hash,
            config_hash,
            path.display()
        );
        return None;
    }

    debug!("using cache at: {}", path.display());
    Some(
        cache
            .headers
            .into_iter()
            .filter_map(|(name, value)| {
                HeaderName::try_from(name)
                    .ok()
                    .zip(HeaderValue::try_from(value).ok())
            })
            .collect(),
    )
}
