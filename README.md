<h1 align="center">
  <img src="feed-icon.svg" width="48" alt=""><br>
  RSS Please
</h1>

<div align="center">
  <strong>A small tool (<code>rsspls</code>) to generate RSS feeds from web
  pages that lack them. It runs on BSD, Linux, macOS, Windows, and
  more.</strong>
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

For more information including installation instructions, documentation, and
news visit the [RSS Please website][website].

<div align="center">
  <a href="https://rsspls.7bit.org/"><img src="visit-website.png" width="198" alt="Visit Website"></a>
</div>

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
[website]: https://rsspls.7bit.org/
