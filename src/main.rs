mod cli;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use std::{env, fs};

use atomicwrites::AtomicFile;
use chrono::{DateTime, FixedOffset};
use eyre::{eyre, Report, WrapErr};
use futures::future;
use kuchiki::traits::TendrilSink;
use kuchiki::{ElementData, NodeDataRef};
use log::{debug, error, info, warn};
use reqwest::Client;
use rss::{Channel, ChannelBuilder, GuidBuilder, ItemBuilder};
use serde::Deserialize;
use simple_eyre::eyre;

#[derive(Debug, Deserialize)]
struct Config {
    rsspls: RssplsConfig,
    feed: Vec<ChannelConfig>,
}

#[derive(Debug, Deserialize)]
struct RssplsConfig {
    output: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelConfig {
    title: String,
    filename: String,
    config: FeedConfig,
}

// TODO: Rename?
#[derive(Debug, Deserialize)]
struct FeedConfig {
    url: String,
    item: String,
    heading: String,
    summary: Option<String>,
    date: Option<String>,
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

    let cli = cli::parse_args().wrap_err("unable to parse CLI arguments")?;
    let cli = match cli {
        Some(cli) => cli,
        // Help or version info was printed and we should return
        None => return Ok(true),
    };

    // Determine the config file path and read it
    let xdg_dirs = xdg::BaseDirectories::with_prefix("rsspls")
        .wrap_err("unable to determine home directory of current user")?;
    let config_path = cli.config_path.ok_or(()).or_else(|()| {
        xdg_dirs
            .place_config_file("feeds.toml")
            .wrap_err("unable to create path to config file")
    })?;
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

    // Ensure output directory exists
    let output_dir = cli.output_path.or_else(|| config.rsspls.output.map(|ref path| PathBuf::from(path)))
        .ok_or_else(|| eyre!("output directory must be supplied via --output or be present in configuration file"))?;

    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).wrap_err_with(|| {
            format!(
                "unable to create output directory: {}",
                output_dir.display()
            )
        })?;
    }

    // Set up the HTTP client
    let connect_timeout = Duration::from_secs(10);
    let timeout = Duration::from_secs(30);
    let client = Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .wrap_err("unable to build HTTP client")?;

    // Spawn the tasks
    let futures = config.feed.into_iter().map(|feed| {
        let client = client.clone(); // Client uses Arc internally
        let output_dir = output_dir.clone();
        tokio::spawn(async move {
            let res = process(&client, &feed).await;
            let res = res
                .and_then(|ref channel| {
                    // TODO: channel.validate()
                    let filename = Path::new(&feed.filename);
                    let output_path =
                        output_dir.join(filename.file_name().ok_or_else(|| {
                            eyre!("{} is not a valid file name", filename.display())
                        })?);
                    write_channel(channel, &output_path).wrap_err_with(|| {
                        format!("unable to write output file: {}", output_path.display())
                    })
                })
                .wrap_err_with(|| format!("error processing feed for {}", feed.config.url));

            if let Err(ref report) = res {
                error!("{:?}", report);
            }
            res.is_ok()
        })
    });

    // Run all the futures at the same time
    // The ? here will fail on an error if the JoinHandle fails
    let ok = future::try_join_all(futures)
        .await?
        .into_iter()
        .fold(true, |ok, succeeded| ok & succeeded);

    Ok(ok)
}

fn write_channel(channel: &Channel, output_path: &Path) -> Result<(), Report> {
    // Write the new file into a temporary location, then move it into place
    let file = AtomicFile::new(output_path, atomicwrites::AllowOverwrite);
    file.write(|f| {
        info!("write {}", output_path.display());
        channel
            .write_to(f)
            .map(drop)
            .wrap_err("unable to write feed")
    })
    .map_err(|err| match err {
        atomicwrites::Error::Internal(atomic_err) => atomic_err.into(),
        atomicwrites::Error::User(myerr) => myerr,
    })
}

async fn process(client: &Client, channel_config: &ChannelConfig) -> eyre::Result<Channel> {
    let config = &channel_config.config;
    info!("processing {}", config.url);
    let resp = client
        .get(&config.url)
        .send()
        .await
        .wrap_err_with(|| format!("unable to fetch {}", config.url))?;

    // Check response
    let status = resp.status();
    if !status.is_success() {
        return Err(eyre!(
            "failed to fetch {}: {} {}",
            config.url,
            status.as_str(),
            status.canonical_reason().unwrap_or("Unknown Status")
        ));
    }

    // Read body
    let html = resp.text().await.wrap_err("unable to read response body")?;

    let doc = kuchiki::parse_html().one(html);
    let mut items = Vec::new();
    for item in doc
        .select(&config.item)
        .map_err(|()| eyre!("invalid selector for item: {}", config.item))?
    {
        let title = item
            .as_node()
            .select_first(&config.heading)
            .map_err(|()| eyre!("invalid selector for title: {}", config.heading))?;
        // TODO: Need to make links absolute (probably ones in content too)
        let attrs = title.attributes.borrow();
        let link = attrs
            .get("href")
            .ok_or_else(|| eyre!("element selected as heading has no 'href' attribute"))?;
        let description = extract_description(config, &item)?;
        let date = extract_pub_date(config, &item)?;
        let guid = GuidBuilder::default().value(link).permalink(false).build();

        let rss_item = ItemBuilder::default()
            .title(title.text_contents())
            .link(Some(link.to_string()))
            .guid(Some(guid))
            .pub_date(date.map(|date| date.to_rfc2822()))
            .description(description)
            .build();
        items.push(rss_item);
    }

    let channel = ChannelBuilder::default()
        .title(&channel_config.title)
        .link(&config.url)
        .generator(Some(version_string()))
        .items(items)
        .build();

    Ok(channel)
}

fn extract_pub_date(
    config: &FeedConfig,
    item: &NodeDataRef<ElementData>,
) -> eyre::Result<Option<DateTime<FixedOffset>>> {
    config
        .date
        .as_ref()
        .map(|selector| {
            item.as_node()
                .select_first(selector)
                .map_err(|()| eyre!("invalid selector for date: {}", selector))
                .map(|node| parse_date(&node))
        })
        .transpose()
        .map(Option::flatten)
}

fn parse_date(node: &NodeDataRef<ElementData>) -> Option<DateTime<FixedOffset>> {
    let attrs = node.attributes.borrow();
    (&node.name.local == "time")
        .then(|| attrs.get("datetime"))
        .flatten()
        .and_then(|datetime| {
            debug!("trying datetime attribute");
            anydate::parse(datetime.trim()).ok()
        })
        .map(|x| {
            debug!("using datetime attribute");
            x
        })
        .or_else(|| {
            let date = node.text_contents();
            let date = date.trim();
            anydate::parse(date)
                .map_err(|_err| {
                    warn!("unable to parse date '{}'", date);
                })
                .ok()
        })
}

fn extract_description(
    config: &FeedConfig,
    item: &NodeDataRef<ElementData>,
) -> eyre::Result<Option<String>> {
    config
        .summary
        .as_ref()
        .map(|selector| {
            item.as_node()
                .select_first(selector)
                .map_err(|()| eyre!("invalid selector for summary: {}", selector))
                .and_then(|node| {
                    let mut text = Vec::new();
                    node.as_node()
                        .serialize(&mut text)
                        .wrap_err("unable to serialise description")
                        .map(|()| String::from_utf8(text).unwrap()) // NOTE(unwrap): Should be safe as XML has be legit Unicode)
                })
        })
        .transpose()
}

pub fn version_string() -> String {
    format!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}
