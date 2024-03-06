use std::mem;

use basic_toml as toml;
use kuchiki::traits::TendrilSink;
use kuchiki::{ElementData, NodeDataRef, NodeRef};
use log::{debug, info, warn};
use mime_guess::mime;
use reqwest::header::HeaderMap;
use reqwest::{Client, RequestBuilder, StatusCode};
use rss::{Channel, ChannelBuilder, EnclosureBuilder, GuidBuilder, Item, ItemBuilder};
use simple_eyre::eyre::{self, eyre, WrapErr};
use time::format_description::well_known::Rfc2822;
use time::OffsetDateTime;
use url::Url;

use crate::cache::RequestCacheWrite;
use crate::config::{ChannelConfig, DateConfig, FeedConfig};

pub enum ProcessResult {
    NotModified,
    Ok {
        channel: Channel,
        headers: Option<String>,
    },
}

pub async fn process_feed(
    client: &Client,
    channel_config: &ChannelConfig,
    cached_headers: &Option<HeaderMap>,
) -> eyre::Result<ProcessResult> {
    let config = &channel_config.config;
    info!("processing {}", config.url);
    let url: Url = config
        .url
        .parse()
        .wrap_err_with(|| format!("unable to parse {} as a URL", config.url))?;
    let req = add_headers(
        client.get(url.clone()),
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
        return Ok(ProcessResult::NotModified);
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

    let link_selector = config.link.as_ref().unwrap_or(&config.heading);

    // Collect the headers for later
    let headers: Vec<_> = resp
        .headers()
        .iter()
        .filter_map(|(name, value)| value.to_str().ok().map(|val| (name.as_str(), val)))
        .collect();
    let map = RequestCacheWrite {
        headers,
        version: crate::version(),
    };
    let serialised_headers = toml::to_string(&map)
        .map_err(|err| warn!("unable to serialise headers: {}", err))
        .ok();

    // Read body
    let html = resp.text().await.wrap_err("unable to read response body")?;

    let doc = kuchiki::parse_html().one(html);
    let base_url = Url::options().base_url(Some(&url));
    rewrite_urls(&doc, &base_url)?;

    let mut items = Vec::new();
    for item in doc
        .select(&config.item)
        .map_err(|()| eyre!("invalid selector for item: {}", config.item))?
    {
        match parse_item(config, item, link_selector, &base_url) {
            Ok(rss_item) => items.push(rss_item),
            Err(e) => eprintln!("Error parsing RSS item {}: {e}", config.item),
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

fn parse_item(
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
    let node = config.summary.as_ref().and_then(|selector| {
        item.as_node()
            .select_first(selector)
            .map_err(|()| {
                warn!(
                    "summary selector for item with title '{}' did not match anything",
                    title.trim()
                )
            })
            .ok()
    });
    if node.is_none() {
        return Ok(None);
    }

    node.map(|node| {
        let mut text = Vec::new();
        node.as_node()
            .serialize(&mut text)
            .wrap_err("unable to serialise description")
            .map(|()| String::from_utf8(text).unwrap()) // NOTE(unwrap): Should be safe as XML has to be legit Unicode)
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
