<h1 align="center">
  <img src="feed-icon.svg" width="48" alt=""><br>
  RSS Please
</h1>

<div align="center">
  <strong>A small tool (<code>rsspls</code>) to generate RSS feeds from web pages that lack them.
    It runs on BSD, Linux, macOS, Windows, and more.</strong>
</div>

<br>

<div align="center">
  <a href="https://cirrus-ci.com/github/wezm/rsspls">
    <img src="https://api.cirrus-ci.com/github/wezm/rsspls.svg" alt="Build Status"></a>
  <a href="https://crates.io/crates/rsspls">
    <img src="https://img.shields.io/crates/v/rsspls.svg" alt="Version">
  </a>
  <img src="https://img.shields.io/crates/l/rsspls.svg" alt="License">
</div>

<br>

`rsspls` generates RSS feeds from web pages. Example use cases:

* Create a feed for a blog that does not have one so that you will know when
  there are new posts.
* Create a feed from the search results on real estate agent's website so that
  you know when there are new listings—without having to check manually all the
  time.
* Create a feed of the upcoming tour dates of your favourite band or DJ.
* Create a feed of the product page for a company, so you know when new
  products are added.

The idea is that you will then subscribe to the generated feeds in your feed
reader. This will typically require the feeds to be hosted via a web server.

Contents
--------

- [Contents](#contents)
- [Install](#install)
  - [Package Manager](#package-manager)
  - [Pre-compiled Binary](#pre-compiled-binary)
- [Usage](#usage)
  - [Configuration](#configuration)
    - [Annotated Sample Configuration](#annotated-sample-configuration)
    - [More Detail on Date Handling](#more-detail-on-date-handling)
  - [Hosting](#hosting)
  - [Caveats](#caveats)
  - [Caching](#caching)
- [Build From Source](#build-from-source)
  - [From Git Checkout or Release Tarball](#from-git-checkout-or-release-tarball)
  - [From crates.io](#from-cratesio)
- [Credits](#credits)
- [Licence](#licence)

Install
-------

`rsspls` can be installed via one of the following methods:

* [Package Manager](#package-manager)
* [Pre-compiled Binary](#pre-compiled-binary)
* [Build From Source](#build-from-source)

### Package Manager

`rsspls` is packaged in these package managers:

* [AUR](https://aur.archlinux.org/packages/rsspls)
* Homebrew: `brew install wezm/taps/rsspls`

### Pre-compiled Binary

Pre-compiled binaries are available for a number of platforms.
They require no additional dependencies on your computer.

* [FreeBSD 13 amd64](https://releases.wezm.net/rsspls/0.7.1/rsspls-0.7.1-amd64-unknown-freebsd.tar.gz)
* [Linux x86\_64](https://releases.wezm.net/rsspls/0.7.1/rsspls-0.7.1-x86_64-unknown-linux-musl.tar.gz)
* [MacOS Universal](https://releases.wezm.net/rsspls/0.7.1/rsspls-0.7.1-universal-apple-darwin.tar.gz)
* [Windows x86\_64](https://releases.wezm.net/rsspls/0.7.1/rsspls-0.7.1-x86_64-pc-windows-msvc.zip)

Example to download and extract a binary:

    curl https://releases.wezm.net/rsspls/0.7.1/rsspls-0.7.1-x86_64-unknown-linux-musl.tar.gz | tar zxf -

This will result in the `rsspls` binary in the current directory.

Usage
-----

### Configuration

Unless specified via the `--config` command line option `rsspls` reads its
configuration from one of the following paths:

* UNIX-like systems:
  * `$XDG_CONFIG_HOME/rsspls/feeds.toml`
  * `~/.config/rsspls/feeds.toml` if `XDG_CONFIG_HOME` is unset.
* Windows:
  * `C:\Users\You\AppData\Roaming\rsspls\feeds.toml`
  
The configuration file is in [TOML][toml] format.

The parts of the page to extract for the feed are specified using [CSS
selectors][selectors].

#### Annotated Sample Configuration

The sample file below demonstrates all the parts of the configuration.

```toml
# The configuration must start with the [rsspls] section
[rsspls]
# Optional output directory to write the feeds to. If not specified it must be supplied via
# the --output command line option.
output = "/tmp"

# Next is the array of feeds, each one starts with [[feed]]
[[feed]]
# The title of the channel in the feed
title = "My Great RSS Feed"
# The output filename without the output directory to write this feed to.
# Note: this is a filename only, not a path. It should not contain slashes.
filename = "wezm.rss"
# Optional user agent header to be set for the http(s) request.
# This line can also be removed entirely if not needed.
user_agent = "Mozilla/5.0"

# The configuration for the feed
[feed.config]
# The URL of the web page to generate the feed from.
url = "https://www.wezm.net/"
# A CSS selector to select elements on the page that represent items in the feed.
item = "article"
# A CSS selector relative to `item` to an element that will supply the title for the item.
heading = "h3"
# A CSS selector relative to `item` to an element that will supply the link for the item.
# Note: This element must have a `href` attribute.
# Note: If not supplied rsspls will attempt to use the heading selector for link for backwards
#       compatibility with earlier versions. A message will be emitted in this case.
link = "h3 a"
# Optional CSS selector relative to `item` that will supply the content of the RSS item.
summary = ".post-body"
# Optional CSS selector relative to `item` that supples the publication date of the RSS item.
date = "time"
# Alternatively for more control `date` can be specified as a table:
# [feed.config.date]
# selector = "time"
# # Optional type of value being parsed.
# # Defaults to DateTime, can also be Date if you're parsing a value without a time.
# type = "Date" 
# # format of the date to parse. See the following for the syntax
# # https://time-rs.github.io/book/api/format-description.html
# format = "[day padding:none]/[month padding:none]/[year]" # will parse 1/2/1934 style dates

# A second example feed
[[feed]]
title = "Example Site"
filename = "example.rss"

[feed.config]
url = "https://example.com/"
item = "div"
heading = "a"
```

The first example above (for my blog WezM.net) matches HTML that looks like this:

```html
<section class="posts-section">
  <h2>Recent Posts</h2>

  <article id="garage-door-monitor">
    <h3><a href="https://www.wezm.net/v2/posts/2022/garage-door-monitor/">Monitoring My Garage Door With a Raspberry Pi, Rust, and a 13Mb Linux System</a></h3>
    <div class="post-metadata">
      <div class="date-published">
        <time datetime="2022-04-20T06:38:27+10:00">20 April 2022</time>
      </div>
    </div>

    <div class="post-body">
      <p>I’ve accidentally left our garage door open a few times. To combat this I built
        a monitor that sends an alert via Mattermost when the door has been left open
        for more than 5 minutes. This turned out to be a super fun project. I used
        parts on hand as much as possible, implemented the monitoring application in
        Rust, and then built a stripped down Linux image to run it.
      </p>
    </div>

    <a href="https://www.wezm.net/v2/posts/2022/garage-door-monitor/">Continue Reading →</a>
  </article>

  <article id="monospace-kobo-ereader">
    <!-- another article -->
  </article>

  <!-- more articles -->

  <a href="https://www.wezm.net/v2/posts/">View more posts →</a>
</section>
```

#### More Detail on Date Handling

The `date` key in the configuration can be a string or a table. If it's a
string then it's used as selector to find the element containing the date and
`rsspls` will attempt to automatically parse the value. If automatic parsing
fails you can manually specify the format using the table form of `date`:

```toml
[feed.config.date]
selector = "time" # required
type = "Date"
format = "[day padding:none]/[month padding:none]/[year]"
```

* `type` is `Date` when you want to parse just a date. Use `DateTime` if you're
  parsing a date and time with the format. Defaults to `DateTime`.
* `format` is a format description using the syntax described on this page:
  <https://time-rs.github.io/book/api/format-description.html>.

If the element matched by the `date` selector is a `<time>` element then
`rsspls` will first try to parse the value in the `datetime` attribute if
present. If the attribute is missing or the element is not a `time` element
then `rsspls` will use the supplied format or attempt automatic parsing of the
text content of the element.

### Hosting

It is expected that `rsspls` will be run on a web server that is serving the
directory the feeds are written to. `rsspls` just generates the feeds, it's not
a server. In order to have the feeds update you will need to arrange for
`rsspls` to be run periodically. You might do this with [cron], [systemd
timers][timers], or the Windows equivalent.

### Caveats

`rsspls` just fetches and parses the HTML of the web page you specify. It does
not run JavaScript. If the website is entirely generated by JavaScript (such as
Twitter) then `rsspls` will not work.

### Caching

When websites respond with cache headers `rsspls` will make a conditional
request on subsequent runs and will not regenerate the feed if the server
responds with 304 Not Modified. Cache data is stored in
`$XDG_CACHE_HOME/rsspls`, which defaults to `~/.cache/rsspls` on UNIX-like
systems or `C:\Users\You\AppData\Local\rsspls` on Windows.

Build From Source
-----------------

**Minimum Supported Rust Version:** 1.70.0

`rsspls` is implemented in Rust. See the Rust website for [instructions on
installing the toolchain][rustup].

### From Git Checkout or Release Tarball

Build the binary with `cargo build --release --locked`. The binary will be in
`target/release/rsspls`.

### From crates.io

`cargo install rsspls`

Credits
-------

* [RSS feed icon](http://www.feedicons.com/) by The Mozilla Foundation

Licence
-------

This project is dual licenced under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/wezm/rsspls/blob/master/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/wezm/rsspls/blob/master/LICENSE-MIT))

at your option.

[rustup]: https://www.rust-lang.org/tools/install
[toml]: https://toml.io/
[selectors]: https://developer.mozilla.org/en-US/docs/Learn/CSS/Building_blocks/Selectors
[cron]: https://en.wikipedia.org/wiki/Cron
[timers]: https://wiki.archlinux.org/title/Systemd/Timers
