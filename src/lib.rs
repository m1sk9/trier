#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod browser;
pub mod cli;
pub mod config;
pub mod macos;
pub mod paths;
pub mod pattern;
pub mod resolve;
pub mod state;
#[cfg(test)]
mod test_support;
