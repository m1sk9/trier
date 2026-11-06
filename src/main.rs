use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use trier::{cli, handler, macos, paths, register};

#[derive(Debug, Parser)]
#[command(version, about, arg_required_else_help = true)]
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
    /// Make trier the default browser by generating and registering ~/Applications/Trier.app.
    Register,
    /// Restore the previous default browser and remove ~/Applications/Trier.app.
    Unregister,
    /// Show the config, the bundle and whether trier is the default browser.
    Status,
    /// Print a shell completion script to stdout.
    Completions {
        /// The shell to generate completions for.
        shell: Shell,
    },
    /// Install the trier skill (usage notes for LLM coding agents).
    Skill {
        /// Whose skills directory to install into.
        #[arg(long, value_enum, default_value_t = AgentArg::Claude, conflicts_with = "path")]
        agent: AgentArg,
        /// Install into this directory instead (SKILL.md is written directly inside it).
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
        /// Overwrite an existing SKILL.md.
        #[arg(long)]
        force: bool,
    },
    /// Manage the config file.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AgentArg {
    /// Claude Code: ~/.claude/skills/trier (or $CLAUDE_CONFIG_DIR/skills/trier).
    Claude,
    /// The shared ~/.agents/skills/trier directory, read by Codex and others.
    Agents,
}

impl From<AgentArg> for cli::skill::Agent {
    fn from(arg: AgentArg) -> Self {
        match arg {
            AgentArg::Claude => Self::Claude,
            AgentArg::Agents => Self::Agents,
        }
    }
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
    // Only the bundle's `LSEnvironment` sets this, so only Launch Services
    // launches enter handler mode. Not a TTY check: a script running `trier`
    // without arguments would wait for URLs that never come.
    if std::env::var_os(macos::HANDLER_ENV).is_some_and(|v| v == "1") {
        handler::run();
    }
    let args = Args::parse();
    let config_path = paths::config_path(args.config.as_deref())?;

    let mut report = match args.command {
        Command::Register | Command::Unregister => cli::Report::live(),
        _ => cli::Report::default(),
    };
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
        Command::Register => cli::register::run(&config_path, &mut report),
        Command::Unregister => cli::unregister::run(&mut report),
        Command::Status => cli::status::run(
            cli::status::Inputs {
                config_path: config_path.clone(),
                bundle: register::installed_bundle_path().map_err(|e| e.to_string()),
                state_path: paths::state_file(),
                log_path: paths::log_file(),
                https_handler: macos::default_https_handler().map(|(_, id)| id),
            },
            &mut report,
        ),
        Command::Completions { shell } => {
            cli::completions::run(shell, &mut Args::command(), &mut report);
            true
        }
        Command::Skill { agent, path, force } => {
            let dir = match path {
                Some(dir) => dir,
                None => cli::skill::default_dir(
                    agent.into(),
                    paths::home_dir().as_deref(),
                    std::env::var_os("CLAUDE_CONFIG_DIR")
                        .filter(|v| !v.is_empty())
                        .map(PathBuf::from)
                        .as_deref(),
                )?,
            };
            let written = cli::skill::install(&dir, force).context("skill install failed")?;
            report.out(format_args!("Wrote {}", written.display()));
            true
        }
        Command::Config {
            command: ConfigCommand::Path,
        } => {
            report.out(config_path.display());
            true
        }
        Command::Config {
            command: ConfigCommand::Init { force },
        } => {
            cli::config::init(&config_path, force).context("config init failed")?;
            report.out(format_args!("Wrote {}", config_path.display()));
            true
        }
    };
    // Not `?` on each stdout write: a broken pipe on stdout would drop
    // every error and warning still to come on stderr.
    let (mut stdout, mut stderr) = (std::io::stdout().lock(), std::io::stderr().lock());
    let mut stdout_result = Ok(());
    for (stream, line) in report.lines() {
        match stream {
            cli::Stream::Out if stdout_result.is_ok() => {
                stdout_result = writeln!(stdout, "{line}");
            }
            cli::Stream::Out => {}
            cli::Stream::Err => writeln!(stderr, "{line}")?,
        }
    }
    stdout_result?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
