use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use trier::{cli, paths};

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

    match args.command {
        Command::Check => {
            let ok = cli::check::run(
                &config_path,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )?;
            Ok(exit_code(ok))
        }
        Command::Explain { url } => {
            let ok = cli::explain::run(
                &url,
                &config_path,
                paths::state_file(),
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )?;
            Ok(exit_code(ok))
        }
        Command::Config {
            command: ConfigCommand::Path,
        } => {
            println!("{}", config_path.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Config {
            command: ConfigCommand::Init { force },
        } => {
            cli::config::init(&config_path, force).context("config init failed")?;
            println!("Wrote {}", config_path.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn exit_code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
