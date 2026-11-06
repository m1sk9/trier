use clap::Command;
use clap_complete::{Shell, generate};

use super::Report;

/// Prints the completion script of `command` for `shell`.
pub fn run(shell: Shell, command: &mut Command, report: &mut Report) {
    let name = command.get_name().to_owned();
    let mut script = Vec::new();
    generate(shell, command, name, &mut script);
    // The generators emit UTF-8 only; lossy keeps this infallible.
    for line in String::from_utf8_lossy(&script).lines() {
        report.out(line);
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn sample() -> Command {
        Command::new("trier").subcommand(Command::new("explain"))
    }

    #[test]
    fn every_shell_gets_a_script_that_names_the_command_and_subcommands() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::Elvish,
            Shell::PowerShell,
        ] {
            let mut report = Report::default();
            run(shell, &mut sample(), &mut report);
            let out = report.stdout();
            assert!(out.contains("trier"), "{shell}: {out}");
            assert!(out.contains("explain"), "{shell}: {out}");
            assert!(report.stderr().is_empty(), "{shell}");
        }
    }
}
