#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(not(target_os = "macos"))]
compile_error!("trier supports macOS only: it routes URLs through Launch Services and AppKit.");

pub mod browser;
pub mod cli;
pub mod config;
pub mod macos;
pub mod paths;
pub mod pattern;
pub mod resolve;
pub mod state;
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test_support;
