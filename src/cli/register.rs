use std::path::Path;

use crate::cli::Report;

/// `trier register`; see [`crate::register::register`].
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn run(config_path: &Path, report: &mut Report) -> bool {
    crate::register::register(config_path, report)
}
