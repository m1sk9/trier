use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use trier::{cli, macos, paths};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Config file path. Defaults to $TRIER_CONFIG, then $XDG_CONFIG_HOME/trier/config.toml,
    /// then ~/.config/trier/config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the config file.
    Check,
    /// Show which browser a URL would open in, and why.
    Explain {
        /// The URL to resolve.
        url: String,
    },
    /// Open URLs, each in the browser its rules choose.
    Open {
        /// The URLs to open.
        #[arg(required = true)]
        urls: Vec<String>,
    },
    /// List built-in and custom browsers and where they are installed.
    Browsers,
    /// Manage the config file.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print the resolved config file path.
    Path,
    /// Write an example config to the resolved path.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
}

fn main() -> anyhow::Result<ExitCode> {
    let args = Args::parse();
    let config_path = paths::config_path(args.config.as_deref())?;

    let mut report = cli::Report::default();
    let ok = match args.command {
        Command::Check => {
            cli::check::run(&config_path, &macos::app_path_for_bundle_id, &mut report)
        }
        Command::Explain { url } => cli::explain::run(
            &url,
            &config_path,
            paths::state_file(),
            &macos::app_path_for_bundle_id,
            &mut report,
        ),
        Command::Open { urls } => cli::open::run(
            &urls,
            &config_path,
            paths::state_file(),
            &mut macos::open_url_with,
            &mut report,
        ),
        Command::Browsers => {
            cli::browsers::run(&config_path, &macos::app_path_for_bundle_id, &mut report)
        }
        Command::Config {
            command: ConfigCommand::Path,
        } => {
            println!("{}", config_path.display());
            true
        }
        Command::Config {
            command: ConfigCommand::Init { force },
        } => {
            cli::config::init(&config_path, force).context("config init failed")?;
            println!("Wrote {}", config_path.display());
            true
        }
    };
    std::io::stdout().write_all(report.stdout.as_bytes())?;
    std::io::stderr().write_all(report.stderr.as_bytes())?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
