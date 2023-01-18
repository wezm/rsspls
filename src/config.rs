use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use eyre::WrapErr;
use serde::Deserialize;
use simple_eyre::eyre;

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
    pub date: Option<Date>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Date {
    Selector(String),
    Config(DateConfig),
}

#[derive(Debug, Deserialize)]
pub struct DateConfig {
    pub selector: String,
    pub format: Option<String>,
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

impl Date {
    pub fn selector(&self) -> &str {
        match self {
            Date::Selector(selector) => selector,
            Date::Config(DateConfig { selector, .. }) => selector,
        }
    }

    pub fn parse(&self, date: &str) -> eyre::Result<DateTime<FixedOffset>> {
        anydate::parse(date).map_err(eyre::Report::from)
    }
}
