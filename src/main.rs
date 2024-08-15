mod cache;
mod cli;
mod config;
mod feed;

#[cfg(windows)]
mod dirs;

#[cfg(not(windows))]
mod xdg;

#[cfg(not(windows))]
use crate::xdg as dirs;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, fs};

use atomicwrites::AtomicFile;
use eyre::{eyre, Report, WrapErr};
use futures::future;
use log::{debug, error, info};
use reqwest::Client as HttpClient;
use rss::Channel;
use simple_eyre::eyre;

use crate::cache::deserialise_cached_headers;
use crate::config::ConfigHash;
use crate::config::{ChannelConfig, Config};
use crate::dirs::Dirs;
use crate::feed::{process_feed, ProcessResult};

const RSSPLS_LOG: &str = "RSSPLS_LOG";

#[derive(Clone)]
pub struct Client {
    /// Whether file URLs are enabled
    file_urls: bool,
    /// HTTP client
    http: HttpClient,
}

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

    let config = Config::read(cli.config_path)?;

    // Determine output directory
    let output_dir = match cli.output_path {
        Some(path) => Some(path),
        None => config
            .rsspls
            .output
            .map(|ref path| {
                dirs::home_dir()
                    .ok_or_else(|| eyre!("unable to determine home directory"))
                    .map(|home| expand_tilde(path, home))
            })
            .transpose()?,
    }
    .ok_or_else(|| {
        eyre!("output directory must be supplied via --output or be present in configuration file")
    })?;

    // Ensure output directory exists
    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).wrap_err_with(|| {
            format!(
                "unable to create output directory: {}",
                output_dir.display()
            )
        })?;
        info!("created output directory: {}", output_dir.display());
    }

    // Set up the HTTP client
    let connect_timeout = Duration::from_secs(10);
    let timeout = Duration::from_secs(30);
    let mut client_builder = HttpClient::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout);

    // Add proxy if provided
    match config.rsspls.proxy {
        Some(proxy) => {
            debug!("using proxy from configuration file: {}", proxy);
            client_builder = client_builder.proxy(reqwest::Proxy::all(proxy)?)
        }
        None => {
            if let Ok(proxy) = env::var("http_proxy") {
                debug!("using http proxy from 'http_proxy' env var: {}", proxy);
                client_builder = client_builder.proxy(reqwest::Proxy::http(proxy)?)
            }
            if let Ok(proxy) = env::var("HTTPS_PROXY") {
                debug!("using https proxy from 'HTTPS_PROXY' env var: {}", proxy);
                client_builder = client_builder.proxy(reqwest::Proxy::https(proxy)?)
            }
        }
    };

    let client = Client {
        file_urls: config.rsspls.file_urls,

        http: client_builder
            .build()
            .wrap_err("unable to build HTTP client")?,
    };

    // Wrap up xdg::BaseDirectories for sharing between tasks. Mutex is used so that only one
    // thread at a time will attempt to create cache directories.
    let dirs = dirs::new()?;
    let dirs = Arc::new(Mutex::new(dirs));

    // Spawn the tasks
    let config_hash = Arc::new(config.hash.clone());
    let futures = config.feed.into_iter().map(|feed| {
        let client = client.clone(); // Client uses Arc internally
        let output_dir = output_dir.clone();
        let dirs = Arc::clone(&dirs);
        let config_hash = Arc::clone(&config_hash);
        tokio::spawn(async move {
            let res = process(
                &feed,
                &client,
                ConfigHash(config_hash.as_str()),
                output_dir,
                dirs,
            )
            .await;
            if let Err(ref report) = res {
                // Eat errors when processing feeds so that we don't stop processing the others.
                // Errors are reported, then we return a boolean indicating success or not, which
                // is used to set the exit status of the program later.
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

async fn process(
    feed: &ChannelConfig,
    client: &Client,
    config_hash: ConfigHash<'_>,
    output_dir: PathBuf,
    dirs: Dirs,
) -> Result<(), Report> {
    // Generate paths up front so we report any errors before making requests
    let filename = Path::new(&feed.filename);
    let filename = filename
        .file_name()
        .map(Path::new)
        .ok_or_else(|| eyre!("{} is not a valid file name", filename.display()))?;
    let output_path = output_dir.join(filename);
    let cache_filename = filename.with_extension("toml");
    let cache_path = {
        let dirs = dirs.lock().map_err(|_| eyre!("unable to acquire mutex"))?;
        dirs.place_cache_file(&cache_filename)
            .wrap_err("unable to create path to cache file")
    }?;
    let cached_headers = deserialise_cached_headers(&cache_path, config_hash);

    process_feed(client, feed, config_hash, &cached_headers)
        .await
        .and_then(|ref process_result| {
            match process_result {
                ProcessResult::NotModified => Ok(()),
                ProcessResult::Ok { channel, headers } => {
                    // TODO: channel.validate()
                    write_channel(channel, &output_path).wrap_err_with(|| {
                        format!("unable to write output file: {}", output_path.display())
                    })?;

                    // Update the cache
                    if let Some(headers) = headers {
                        debug!("write cache {}", cache_path.display());
                        fs::write(cache_path, headers).wrap_err("unable to write to cache")?;
                    }

                    Ok(())
                }
            }
        })
        .wrap_err_with(|| format!("error processing feed for {}", feed.config.url))
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

pub fn version_string() -> String {
    format!("{} version {}", env!("CARGO_PKG_NAME"), version())
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn expand_tilde<P: Into<PathBuf>>(path: P, mut home: PathBuf) -> PathBuf {
    let path = path.into();

    // NOTE: starts_with only considers whole path components
    if path.starts_with("~") {
        if path == Path::new("~") {
            home
        } else {
            home.push(path.strip_prefix("~").unwrap());
            home
        }
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn test_home() {
        let expanded = expand_tilde("asdf", PathBuf::from("/home/foo"));
        assert_eq!(expanded, Path::new("asdf"));

        let expanded = expand_tilde("~asdf", PathBuf::from("/home/foo"));
        assert_eq!(expanded, Path::new("~asdf"));

        let expanded = expand_tilde("~/some/where", PathBuf::from("/home/foo"));
        assert_eq!(expanded, Path::new("/home/foo/some/where"));

        let expanded = expand_tilde("~/some/where", PathBuf::from("/"));
        assert_eq!(expanded, Path::new("/some/where"));
    }

    #[test]
    #[cfg(windows)]
    fn test_home_windows() {
        let expanded = expand_tilde("asdf", PathBuf::from(r"C:\Users\Foo"));
        assert_eq!(expanded, Path::new("asdf"));

        let expanded = expand_tilde("~asdf", PathBuf::from(r"C:\Users\Foo"));
        assert_eq!(expanded, Path::new("~asdf"));

        let expanded = expand_tilde(r"~\some\where", PathBuf::from(r"C:\Users\Foo"));
        assert_eq!(expanded, Path::new(r"C:\Users\Foo\some\where"));

        let expanded = expand_tilde(r"~\some\where", PathBuf::from(r"C:\"));
        assert_eq!(expanded, Path::new(r"C:\some\where"));
    }
}
