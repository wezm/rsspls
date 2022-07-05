use std::sync::{Arc, Mutex};

use eyre::WrapErr;
use simple_eyre::eyre;

pub type Dirs = Arc<Mutex<xdg::BaseDirectories>>;

pub fn new() -> eyre::Result<xdg::BaseDirectories> {
    xdg::BaseDirectories::with_prefix("rsspls")
        .wrap_err("unable to determine home directory of current user")
}
