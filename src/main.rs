use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use std::{env, fs};

use clap::Parser;
use eyre::{eyre, WrapErr};
use log::{error, info};
use reqwest::Client;
use serde::Deserialize;
use simple_eyre::eyre;

#[derive(Debug, Deserialize)]
struct Config {
    feed: Vec<Feed>,
}

#[derive(Debug, Deserialize)]
struct Feed {
    url: String,
    item: String,
    heading: String,
    summary: Option<String>,
    date: Option<String>,
}

/// Generate an RSS feed from websites
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    /// path to configuration file
    #[clap(short, long, value_parser)]
    config: Option<PathBuf>,
}

const RSSPLS_LOG: &str = "RSSPLS_LOG";

#[tokio::main]
async fn main() -> ExitCode {
    match try_main().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(report) => {
            error!("{:?}", report);
            ExitCode::FAILURE
        }
    }
}

async fn try_main() -> eyre::Result<bool> {
    simple_eyre::install()?;
    match env::var_os(RSSPLS_LOG) {
        None => env::set_var(RSSPLS_LOG, "info"),
        Some(_) => {}
    }
    pretty_env_logger::try_init_custom_env(RSSPLS_LOG)?;

    let cli = Cli::parse();

    let config_path = cli
        .config
        .ok_or_else(|| eyre!("--config is required (for now)"))?;
    let raw_config = fs::read(&config_path).wrap_err_with(|| {
        format!(
            "unable to read configuration file: {}",
            config_path.display()
        )
    })?;
    let config: Config = toml::from_slice(&raw_config).wrap_err_with(|| {
        format!(
            "unable to parse configuration file: {}",
            config_path.display()
        )
    })?;

    dbg!(&config);

    let connect_timeout = Duration::from_secs(10);
    let timeout = Duration::from_secs(30);
    let client = Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .wrap_err("unable to build HTTP client")?;

    let mut ok = true;
    for feed in &config.feed {
        let res = process(&client, &feed).await;
        ok &= res.is_ok();
        match res {
            Ok(()) => {}
            Err(report) => {
                error!("{:?}", report);
            }
        }
    }

    Ok(ok)
}

async fn process(client: &Client, feed: &Feed) -> eyre::Result<()> {
    info!("processing {}", feed.url);
    let resp = client
        .get(&feed.url)
        .send()
        .await
        .wrap_err_with(|| format!("unable to fetch {}", feed.url))?;

    // Check response
    let status = resp.status();
    if !status.is_success() {
        return Err(eyre!(
            "failed to fetch {}: {} {}",
            feed.url,
            status.as_str(),
            status.canonical_reason().unwrap_or("Unknown Status")
        ));
    }

    // Read body
    // TODO: Handle encodings other than UTF-8
    let html = resp.text().await.wrap_err("unable to read response body")?;

    dbg!(&html);

    Ok(())
}
