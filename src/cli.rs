use std::convert::Infallible;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

use log::debug;
use pico_args::Arguments;
use simple_eyre::eyre;

use crate::version_string;

pub struct Cli {
    pub config_path: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
    pub param_kv: Option<(String, String)>,
}

pub fn parse_args() -> eyre::Result<Option<Cli>> {
    let mut pargs = Arguments::from_env();
    if pargs.contains(["-V", "--version"]) {
        return print_version();
    } else if pargs.contains(["-h", "--help"]) {
        return print_usage();
    }

    let param_kv =
        pargs
            .opt_value_from_str(["-p", "--parameter"])?
            .and_then(|param_arg: String| {
                let parts: Vec<&str> = param_arg.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    debug!("Could not parse parameter argument, continuing without.");
                    None
                }
            });

    Ok(Some(Cli {
        config_path: pargs.opt_value_from_os_str(["-c", "--config"], pathbuf)?,
        output_path: pargs.opt_value_from_os_str(["-o", "--output"], pathbuf)?,
        param_kv,
    }))
}

fn pathbuf(s: &OsStr) -> Result<PathBuf, Infallible> {
    Ok(PathBuf::from(s))
}

fn print_version() -> eyre::Result<Option<Cli>> {
    println!("{}", version_string());
    Ok(None)
}

pub fn print_usage() -> eyre::Result<Option<Cli>> {
    println!(
        "{}

{bin} generates RSS feeds from web pages.

USAGE:
    {bin} [OPTIONS] -o OUTPUT_DIR

OPTIONS:
    -h, --help
            Prints this help information

    -c, --config
            Specify the path to the configuration file.
            $XDG_CONFIG_HOME/rsspls/feeds.toml is used if not supplied.

    -o, --output
            Directory to write generated feeds to.

    -V, --version
            Prints version information

FILES:
     ~/$XDG_CONFIG_HOME/rsspls/feeds.toml    rsspls configuration file.

     ~/$XDG_CONFIG_HOME/rsspls               Configuration directory.

     ~/XDG_CACHE_HOME/rsspls                 Cache directory.

     Note: XDG_CONFIG_HOME defaults to ~/.config, XDG_CACHE_HOME
     defaults to ~/.cache.

AUTHOR
    {}

SEE ALSO
    https://github.com/wezm/rsspls  Source code and issue tracker.",
        version_string(),
        env!("CARGO_PKG_AUTHORS"),
        bin = env!("CARGO_PKG_NAME")
    );
    Ok(None)
}
