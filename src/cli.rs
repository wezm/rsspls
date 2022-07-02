use std::convert::Infallible;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

use pico_args::Arguments;
use simple_eyre::eyre;

pub struct Cli {
    pub config_path: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
}

pub fn parse_args() -> eyre::Result<Option<Cli>> {
    let mut pargs = Arguments::from_env();
    if pargs.contains(["-V", "--version"]) {
        return print_version();
    } else if pargs.contains(["-h", "--help"]) {
        return print_usage();
    }

    Ok(Some(Cli {
        config_path: pargs.opt_value_from_os_str(["-c", "--config"], pathbuf)?,
        output_path: pargs.opt_value_from_os_str(["-o", "--output"], pathbuf)?,
    }))
}

fn pathbuf(s: &OsStr) -> Result<PathBuf, Infallible> {
    Ok(PathBuf::from(s))
}

fn print_version() -> eyre::Result<Option<Cli>> {
    println!("{}", version_string());
    Ok(None)
}

fn version_string() -> String {
    format!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
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
     ~/.config/rsspls/feeds.toml    rsspls configuration file.

     ~/.config/rsspls               Configuration directory.
                                    See also XDG_CONFIG_HOME.

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
