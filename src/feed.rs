use std::{fs, mem};

use basic_toml as toml;
use kuchiki::traits::TendrilSink;
use kuchiki::{ElementData, NodeDataRef, NodeRef};
use log::{debug, error, info, warn};
use mime_guess::mime;
use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, StatusCode};
use rss::{Channel, ChannelBuilder, EnclosureBuilder, GuidBuilder, Item, ItemBuilder};
use simple_eyre::eyre::{self, bail, eyre, WrapErr};
use time::format_description::well_known::Rfc2822;
use time::OffsetDateTime;
use tokio::task;
use url::Url;

use crate::cache::RequestCacheWrite;
use crate::config::{ChannelConfig, ConfigHash, DateConfig, FeedConfig};
use crate::Client;

#[derive(Debug)]
pub enum ProcessResult {
    NotModified,
    Ok {
        channel: Channel,
        headers: Option<String>,
    },
}

pub enum FetchResult {
    NotModified,
    Ok {
        html: String,
        headers: Option<String>,
    },
}

pub async fn process_feed(
    client: &Client,
    channel_config: &ChannelConfig,
    config_hash: ConfigHash<'_>,
    cached_headers: &Option<HeaderMap>,
) -> eyre::Result<ProcessResult> {
    let config = &channel_config.config;
    info!("processing {}", config.url);
    let url: Url = config
        .url
        .parse()
        .wrap_err_with(|| format!("unable to parse {} as a URL", config.url))?;

    let (html, serialised_headers) =
        match fetch_webpage(client, &url, cached_headers, channel_config, config_hash).await? {
            FetchResult::Ok { html, headers } => (html, headers),
            FetchResult::NotModified => return Ok(ProcessResult::NotModified),
        };

    let link_selector = config.link.as_ref().unwrap_or(&config.heading);

    let doc = kuchiki::parse_html().one(html);
    let base_url = Url::options().base_url(Some(&url));
    rewrite_urls(&doc, &base_url)?;

    let mut items = Vec::new();
    for item in doc
        .select(&config.item)
        .map_err(|()| eyre!("invalid selector for item: {}", config.item))?
    {
        match process_item(config, item, link_selector, &base_url) {
            Ok(rss_item) => items.push(rss_item),
            Err(err) => {
                let report = err.wrap_err(format!(
                    "unable to process RSS item matching '{}'",
                    config.item
                ));
                error!("{report:?}");
            }
        }
    }

    let channel = ChannelBuilder::default()
        .title(&channel_config.title)
        .link(url.to_string())
        .generator(Some(crate::version_string()))
        .items(items)
        .build();

    Ok(ProcessResult::Ok {
        channel,
        headers: serialised_headers,
    })
}

async fn fetch_webpage(
    client: &Client,
    url: &Url,
    cached_headers: &Option<HeaderMap>,
    channel_config: &ChannelConfig,
    config_hash: ConfigHash<'_>,
) -> eyre::Result<FetchResult> {
    if url.scheme() == "file" {
        if client.file_urls {
            fetch_webpage_local(url).await
        } else {
            bail!("unable to fetch: {url} as file URLs are not enabled in config")
        }
    } else {
        fetch_webpage_http(client, url, cached_headers, channel_config, config_hash).await
    }
}

async fn fetch_webpage_http(
    client: &Client,
    url: &Url,
    cached_headers: &Option<HeaderMap>,
    channel_config: &ChannelConfig,
    config_hash: ConfigHash<'_>,
) -> eyre::Result<FetchResult> {
    let config = &channel_config.config;

    let req = add_headers(
        client.http.get(url.clone()),
        cached_headers,
        &channel_config.user_agent,
    );

    let resp = req
        .send()
        .await
        .wrap_err_with(|| format!("unable to fetch {}", url))?;

    // Check response
    let status = resp.status();
    if status == StatusCode::NOT_MODIFIED {
        // Cache hit, nothing to do
        info!("{} is unmodified", url);
        return Ok(FetchResult::NotModified);
    }

    if !status.is_success() {
        return Err(eyre!(
            "failed to fetch {}: {} {}",
            config.url,
            status.as_str(),
            status.canonical_reason().unwrap_or("Unknown Status")
        ));
    }

    if config.link.is_none() {
        info!(
            "no explicit link selector provided, falling back to heading selector: {:?}",
            config.heading
        );
    }

    // Collect the headers for later
    let headers: Vec<_> = resp
        .headers()
        .iter()
        .filter_map(|(name, value)| value.to_str().ok().map(|val| (name.as_str(), val)))
        .collect();
    let map = RequestCacheWrite {
        headers,
        version: crate::version(),
        config_hash,
    };
    let serialised_headers = toml::to_string(&map)
        .map_err(|err| warn!("unable to serialise headers: {}", err))
        .ok();

    // Read body
    let html = resp.text().await.wrap_err("unable to read response body")?;

    Ok(FetchResult::Ok {
        html,
        headers: serialised_headers,
    })
}

async fn fetch_webpage_local(url: &Url) -> eyre::Result<FetchResult> {
    let path = url
        .to_file_path()
        .map_err(|()| eyre!("unable to extract path from: {}", url))?;
    debug!("read {}", path.display());
    let html = task::spawn_blocking(move || {
        fs::read_to_string(&path).wrap_err_with(|| format!("error reading {}", path.display()))
    })
    .await
    .wrap_err_with(|| format!("error joining task for {url}"))??;

    Ok(FetchResult::Ok {
        html,
        headers: None,
    })
}

fn process_item(
    config: &FeedConfig,
    item: NodeDataRef<ElementData>,
    link_selector: &str,
    base_url: &url::ParseOptions,
) -> eyre::Result<Item> {
    let title = item
        .as_node()
        .select_first(&config.heading)
        .map_err(|()| eyre!("invalid selector for heading: {}", config.heading))?;
    let link = item
        .as_node()
        .select_first(link_selector)
        .map_err(|()| eyre!("invalid selector for link: {}", link_selector))?;
    // TODO: Need to make links absolute (probably ones in content too)
    let attrs = link.attributes.borrow();
    let link_url = attrs
        .get("href")
        .ok_or_else(|| eyre!("element selected as link has no 'href' attribute"))?;
    let title_text = title.text_contents();
    let description = extract_description(config, &item, &title_text)?;
    let date = extract_pub_date(config, &item)?;
    let guid = GuidBuilder::default()
        .value(link_url)
        .permalink(false)
        .build();

    let mut rss_item_builder = ItemBuilder::default();
    rss_item_builder
        .title(title_text)
        .link(base_url.parse(link_url).ok().map(|u| u.to_string()))
        .guid(Some(guid))
        .pub_date(date.map(|date| date.format(&Rfc2822).unwrap()))
        .description(description);

    // Media enclosure
    if let Some(media_selector) = &config.media {
        debug!("checking for media matching {media_selector}");
        let media = item
            .as_node()
            .select_first(media_selector)
            .map_err(|()| eyre!("invalid selector for media: {}", media_selector))?;

        let media_attrs = media.attributes.borrow();
        let media_url = media_attrs
            .get("src")
            .or_else(|| media_attrs.get("href"))
            .ok_or_else(|| eyre!("element selected as media has no 'src' or 'href' attribute"))?;

        let parsed_url = base_url
            .parse(media_url)
            .map_err(|e| eyre!("media enclosure url invalid: {e}"))?;

        // Guessing the MIME type from the url as we don't have the full media
        let media_mime_type = parsed_url
            .path_segments()
            .and_then(|segments| segments.last())
            .map(|media_filename| mime_guess::from_path(media_filename).first_or_octet_stream())
            .unwrap_or_else(|| mime::APPLICATION_OCTET_STREAM);

        let mut enclosure_bld = EnclosureBuilder::default();
        enclosure_bld.url(parsed_url.to_string());
        enclosure_bld.mime_type(media_mime_type.to_string());
        // "When an enclosure's size cannot be determined, a publisher should use a length of 0."
        // https://www.rssboard.org/rss-profile#element-channel-item-enclosure
        enclosure_bld.length("0".to_string());

        rss_item_builder.enclosure(Some(enclosure_bld.build()));
    }

    Ok(rss_item_builder.build())
}

fn rewrite_urls(doc: &NodeRef, base_url: &url::ParseOptions) -> eyre::Result<()> {
    for el in doc
        .select("*[href]")
        .map_err(|()| eyre!("unable to select links for rewriting"))?
    {
        let mut attrs = el.attributes.borrow_mut();
        attrs.get_mut("href").and_then(|href| {
            let mut url = base_url.parse(href).ok().map(|url| url.to_string())?;
            mem::swap(href, &mut url);
            Some(())
        });
    }

    Ok(())
}

fn add_headers(
    mut req: RequestBuilder,
    cached_headers: &Option<HeaderMap>,
    user_agent: &Option<String>,
) -> RequestBuilder {
    use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, USER_AGENT};

    if let Some(ua) = user_agent {
        debug!("add User-Agent: {:?}", ua);
        req = req.header(USER_AGENT, ua);
    }

    let headers = match cached_headers {
        Some(headers) => headers,
        None => return req,
    };

    if let Some(last_modified) = headers.get(LAST_MODIFIED) {
        debug!("add If-Modified-Since: {:?}", last_modified.to_str().ok());
        req = req.header(IF_MODIFIED_SINCE, last_modified);
    }
    if let Some(etag) = headers.get(ETAG) {
        debug!("add If-None-Match: {:?}", etag.to_str().ok());
        req = req.header(IF_NONE_MATCH, etag);
    }
    req
}

fn extract_pub_date(
    config: &FeedConfig,
    item: &NodeDataRef<ElementData>,
) -> eyre::Result<Option<OffsetDateTime>> {
    config
        .date
        .as_ref()
        .map(|date| {
            item.as_node()
                .select_first(date.selector())
                .map_err(|()| eyre!("invalid selector for date: {}", date.selector()))
                .map(|node| parse_date(date, &node))
        })
        .transpose()
        .map(Option::flatten)
}

fn parse_date(date: &DateConfig, node: &NodeDataRef<ElementData>) -> Option<OffsetDateTime> {
    let attrs = node.attributes.borrow();
    (&node.name.local == "time")
        .then(|| attrs.get("datetime"))
        .flatten()
        .and_then(|datetime| {
            debug!("trying datetime attribute");
            date.parse(trim_date(datetime)).ok()
        })
        .map(|x| {
            debug!("using datetime attribute");
            x
        })
        .or_else(|| {
            let text = node.text_contents();
            let text = trim_date(&text);
            date.parse(text)
                .map_err(|_err| {
                    warn!("unable to parse date '{}'", text);
                })
                .ok()
        })
}

// Trim non-alphanumeric chars from either side of the string
fn trim_date(s: &str) -> &str {
    s.trim_matches(|c: char| !c.is_alphanumeric())
}

fn extract_description(
    config: &FeedConfig,
    item: &NodeDataRef<ElementData>,
    title: &str,
) -> eyre::Result<Option<String>> {
    let mut description = Vec::new();

    for selector in &config.summary {
        let nodes = item
            .as_node()
            .select(selector)
            .map_err(|()| {
                warn!(
                    "summary selector '{selector}' for item with title '{}' did not match anything",
                    title.trim()
                )
            })
            .ok();
        let Some(nodes) = nodes else {
            continue;
        };

        for node in nodes {
            node.as_node()
                .serialize(&mut description)
                .wrap_err("unable to serialise description")?
        }
    }

    if !description.is_empty() {
        // NOTE(unwrap): Should be safe as XML has to be legit Unicode)
        Ok(Some(String::from_utf8(description).unwrap()))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::{env, process};

    use reqwest::Client as HttpClient;

    use super::*;

    const HTML: &str = include_str!("../tests/local.html");

    struct RmOnDrop(PathBuf);

    impl RmOnDrop {
        fn new(path: PathBuf) -> Self {
            RmOnDrop(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for RmOnDrop {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn test_config() -> FeedConfig {
        FeedConfig {
            url: String::new(),
            item: String::new(),
            heading: String::new(),
            link: None,
            summary: Vec::new(),
            date: None,
            media: None,
        }
    }

    #[test]
    fn test_trim_date() {
        assert_eq!(trim_date("2021-05-20 —"), "2021-05-20");
        assert_eq!(
            trim_date("2022-04-20T06:38:27+10:00"),
            "2022-04-20T06:38:27+10:00"
        );
    }

    #[test]
    fn test_rewrite_urls() {
        let html = r#"<html><body><a href="/cool">cool thing</a> <div href="dont-do-this">ok</div><a href="http://example.com">example</a></body></html>"#;
        let expected = r#"<html><head></head><body><a href="http://example.com/cool">cool thing</a> <div href="http://example.com/dont-do-this">ok</div><a href="http://example.com/">example</a></body></html>"#;
        let doc = kuchiki::parse_html().one(html);
        let base_url = "http://example.com".parse().unwrap();
        let base = Url::options().base_url(Some(&base_url));
        rewrite_urls(&doc, &base).unwrap();
        let rewritten = doc.to_string();
        assert_eq!(rewritten, expected);
    }

    #[test]
    fn test_extract_description_multi() {
        // Test CSS selector for description that matches multiple elements
        let html = r#"<html><body><div class="item"><p>one</p><span>two</span></body></html>"#;
        let doc = kuchiki::parse_html().one(html);
        let item = doc.select_first(".item").unwrap();
        let config = FeedConfig {
            summary: vec!["span, p".to_string()],
            ..test_config()
        };

        let description = extract_description(&config, &item, "title")
            .unwrap()
            .unwrap();

        // Items come out in DOM order
        assert_eq!(description, "<p>one</p><span>two</span>");
    }

    #[test]
    fn test_extract_description_array() {
        // Test CSS selector for description that matches multiple elements
        let html = r#"<html><body><div class="item"><p>one</p><span>two</span></body></html>"#;
        let doc = kuchiki::parse_html().one(html);
        let item = doc.select_first(".item").unwrap();
        let config = FeedConfig {
            summary: vec!["span".to_string(), "p".to_string()],
            ..test_config()
        };

        let description = extract_description(&config, &item, "title")
            .unwrap()
            .unwrap();

        // Items come out in the order of the selector array
        assert_eq!(description, "<span>two</span><p>one</p>");
    }

    #[test]
    fn test_process_local_html() {
        let html_file_name = format!("rsspls.local.{}.html", process::id());
        let local_html = RmOnDrop::new(env::temp_dir().join(&html_file_name));
        fs::write(local_html.path(), HTML.as_bytes()).expect("unable to write test HTML");

        let url = Url::from_file_path(local_html.path())
            .expect("unable to construct file URL for test HTML");

        let client = Client {
            file_urls: true,
            http: HttpClient::new(),
        };

        let config = FeedConfig {
            url: url.to_string(),
            item: "nav a".to_string(),
            heading: "a".to_string(),
            ..test_config()
        };
        let channel_config = ChannelConfig {
            title: "Local Site".to_string(),
            filename: Path::new(&html_file_name)
                .with_extension("rss")
                .to_string_lossy()
                .into_owned(),
            user_agent: None,
            config,
        };
        let config_hash = ConfigHash(&html_file_name);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let res = runtime
            .block_on(process_feed(&client, &channel_config, config_hash, &None))
            .expect("unable to process local feed");

        let ProcessResult::Ok { channel, .. } = res else {
            panic!("expected ProcessResult::Ok but got: {:?}", res)
        };

        assert_eq!(channel.items().len(), 5);
        assert_eq!(channel.items()[0].title, Some("Install".to_string()));
    }

    #[test]
    fn test_process_local_files_disabled() {
        let html_file_name = "rsspls.local.html";
        let local_html = env::temp_dir().join(&html_file_name);
        let url =
            Url::from_file_path(&local_html).expect("unable to construct file URL for test HTML");

        let client = Client {
            file_urls: false,
            http: HttpClient::new(),
        };

        let config = FeedConfig {
            url: url.to_string(),
            item: "nav a".to_string(),
            heading: "a".to_string(),
            ..test_config()
        };
        let channel_config = ChannelConfig {
            title: "Local Site".to_string(),
            filename: Path::new(&html_file_name)
                .with_extension("rss")
                .to_string_lossy()
                .into_owned(),
            user_agent: None,
            config,
        };
        let config_hash = ConfigHash(&html_file_name);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let res = runtime.block_on(process_feed(&client, &channel_config, config_hash, &None));

        let Err(err) = res else {
            panic!("expected error, got: {:?}", res)
        };

        assert!(err
            .to_string()
            .contains("file URLs are not enabled in config"));
    }
}
