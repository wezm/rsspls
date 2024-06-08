use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use eyre::WrapErr;
use simple_eyre::eyre;

pub type Dirs = Arc<Mutex<xdg::BaseDirectories>>;

pub fn new() -> eyre::Result<xdg::BaseDirectories> {
    xdg::BaseDirectories::with_prefix("rsspls")
        .wrap_err("unable to determine home directory of current user")
}

pub fn home_dir() -> Option<PathBuf> {
    // This module only supports Unix, and the behavior of `std::env::home_dir()` is only
    // problematic on Windows.
    #[allow(deprecated)]
    std::env::home_dir()
}
