use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eyre::eyre;
use simple_eyre::eyre;

pub type Dirs = Arc<Mutex<BaseDirs>>;

pub struct BaseDirs;

pub fn new() -> eyre::Result<BaseDirs> {
    Ok(BaseDirs)
}

pub fn home_dir() -> Option<PathBuf> {
    ::dirs::home_dir()
}

impl BaseDirs {
    pub fn place_config_file<P: AsRef<Path>>(&self, path: P) -> eyre::Result<PathBuf> {
        ::dirs::config_dir()
            .ok_or_else(|| eyre!("unable to dermine user config dir"))
            .map(|mut config| {
                config.push("rsspls");
                config.push(path);
                config
            })
    }

    pub fn place_cache_file<P: AsRef<Path>>(&self, path: P) -> eyre::Result<PathBuf> {
        ::dirs::cache_dir()
            .ok_or_else(|| eyre!("unable to dermine user cache dir"))
            .map(|mut config| {
                config.push("rsspls");
                config.push(path);
                config
            })
    }
}
