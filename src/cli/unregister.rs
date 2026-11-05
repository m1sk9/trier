use crate::cli::Report;

/// `trier unregister`; see [`crate::register::unregister`].
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn run(report: &mut Report) -> bool {
    crate::register::unregister(report)
}
