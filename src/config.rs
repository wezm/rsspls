use eyre::WrapErr;
use serde::Deserialize;
use simple_eyre::eyre;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub rsspls: RssplsConfig,
    pub feed: Vec<ChannelConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RssplsConfig {
    pub output: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelConfig {
    pub title: String,
    pub filename: String,
    pub config: FeedConfig,
}

// TODO: Rename?
#[derive(Debug, Deserialize)]
pub struct FeedConfig {
    pub url: String,
    pub item: String,
    pub heading: String,
    pub link: Option<String>,
    pub summary: Option<String>,
    pub date: Option<String>,
}

impl Config {
    /// Read the config file path and the supplied path or default if None
    pub fn read(config_path: Option<PathBuf>) -> eyre::Result<Config> {
        let dirs = crate::dirs::new()?;
        let config_path = config_path.ok_or(()).or_else(|()| {
            dirs.place_config_file("feeds.toml")
                .wrap_err("unable to create path to config file")
        })?;
        let raw_config = fs::read(&config_path).wrap_err_with(|| {
            format!(
                "unable to read configuration file: {}",
                config_path.display()
            )
        })?;
        toml::from_slice(&raw_config).wrap_err_with(|| {
            format!(
                "unable to parse configuration file: {}",
                config_path.display()
            )
        })
    }
}
