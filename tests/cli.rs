use std::path::{Path, PathBuf};

use assert_cmd::Command;

const EXAMPLE: &str = include_str!("../config/config.toml.example");

/// A fresh per-test directory, so parallel tests never share files.
fn test_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `trier` with an environment that does not depend on the host machine.
fn trier(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("trier").unwrap();
    cmd.env_remove("TRIER_HANDLER")
        .env_remove("TRIER_CONFIG")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .env("HOME", home);
    cmd
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn example_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/config.toml.example")
}

#[test]
fn check_accepts_example_config() {
    let home = test_dir("check-example");
    let path = example_path();
    let output = trier(&home)
        .args(["check", "--config"])
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(stdout(&output).contains(&format!("OK: {}", path.display())));
}

#[test]
fn check_rejects_broken_config_with_exit_1() {
    let home = test_dir("check-broken");
    let path = home.join("broken.toml");
    std::fs::write(&path, "version = 2\n").unwrap();
    let output = trier(&home)
        .args(["check", "--config"])
        .arg(&path)
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(
        stderr(&output).contains("unsupported config version 2"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_rejects_missing_config_with_exit_1() {
    let home = test_dir("check-missing");
    let output = trier(&home)
        .arg("check")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(stderr(&output).contains("failed to read"));
}

#[test]
fn config_path_defaults_to_home_dot_config() {
    let home = test_dir("path-home");
    trier(&home)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(format!(
            "{}\n",
            home.join(".config/trier/config.toml").display()
        ));
}

#[test]
fn config_path_honors_xdg_config_home() {
    let home = test_dir("path-xdg");
    let xdg = home.join("xdg");
    trier(&home)
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(format!("{}\n", xdg.join("trier/config.toml").display()));
}

#[test]
fn config_path_honors_trier_config_env() {
    let home = test_dir("path-env");
    trier(&home)
        .env("TRIER_CONFIG", "/env/config.toml")
        .args(["config", "path"])
        .assert()
        .success()
        .stdout("/env/config.toml\n");
}

#[test]
fn config_path_flag_wins_over_env() {
    let home = test_dir("path-flag");
    trier(&home)
        .env("TRIER_CONFIG", "/env/config.toml")
        .args(["config", "path", "--config", "/flag/config.toml"])
        .assert()
        .success()
        .stdout("/flag/config.toml\n");
}

#[test]
fn config_init_creates_example_at_resolved_path() {
    let home = test_dir("init-create");
    let path = home.join(".config/trier/config.toml");
    trier(&home).args(["config", "init"]).assert().success();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
    trier(&home).arg("check").assert().success();
}

#[test]
fn config_init_refuses_to_overwrite_without_force() {
    let home = test_dir("init-refuse");
    let path = home.join("config.toml");
    std::fs::write(&path, "user edits").unwrap();
    let output = trier(&home)
        .args(["config", "init", "--config"])
        .arg(&path)
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(
        stderr(&output).contains("already exists"),
        "{}",
        stderr(&output)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "user edits");
}

#[test]
fn config_init_force_overwrites() {
    let home = test_dir("init-force");
    let path = home.join("config.toml");
    std::fs::write(&path, "user edits").unwrap();
    trier(&home)
        .args(["config", "init", "--force", "--config"])
        .arg(&path)
        .assert()
        .success();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
}

#[test]
fn explain_prints_matched_rule_and_browser() {
    let home = test_dir("explain-rule");
    let output = trier(&home)
        .args(["explain", "https://github.com/ab-company/trier", "--config"])
        .arg(example_path())
        .assert()
        .success()
        .get_output()
        .clone();
    let out = stdout(&output);
    assert!(out.contains("rules[0] (github.com/ab-company/**)"), "{out}");
    assert!(out.contains("firefox@developer-edition"), "{out}");
}

#[test]
fn explain_falls_back_to_default_browser() {
    let home = test_dir("explain-default");
    let output = trier(&home)
        .args(["explain", "https://example.com", "--config"])
        .arg(example_path())
        .assert()
        .success()
        .get_output()
        .clone();
    let out = stdout(&output);
    assert!(out.contains("matched:   default_browser"), "{out}");
    assert!(out.contains("browser:   safari"), "{out}");
}

#[test]
fn explain_rejects_unsupported_scheme_with_exit_1() {
    let home = test_dir("explain-scheme");
    let output = trier(&home)
        .args(["explain", "mailto:me@example.com", "--config"])
        .arg(example_path())
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(stderr(&output).contains("unsupported URL scheme `mailto`"));
}

#[test]
fn browsers_lists_builtin_and_custom_entries() {
    let home = test_dir("browsers");
    let output = trier(&home)
        .args(["browsers", "--config"])
        .arg(example_path())
        .assert()
        .success()
        .get_output()
        .clone();
    let out = stdout(&output);
    let row = |name: &str| {
        out.lines()
            .find(|l| l.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("no row for {name} in\n{out}"))
            .to_owned()
    };
    assert!(row("firefox@dev").contains("custom"), "{out}");
    assert!(row("safari").contains("built-in"), "{out}");
    assert!(row("safari").trim_end().ends_with("Safari.app"), "{out}");
}

#[test]
fn no_subcommand_prints_help_and_exits_2() {
    let home = test_dir("no-subcommand");
    let output = trier(&home).assert().code(2).get_output().clone();
    assert!(stderr(&output).contains("Usage:"), "{}", stderr(&output));
}

#[test]
fn status_reports_an_unregistered_install() {
    let home = test_dir("status");
    let output = trier(&home)
        .args(["status", "--config"])
        .arg(example_path())
        .assert()
        .code(1)
        .get_output()
        .clone();
    let out = stdout(&output);
    assert!(out.contains("registered: no"), "{out}");
    assert!(
        out.contains(&format!(
            "bundle:     {}",
            home.join("Applications/Trier.app").display()
        )),
        "{out}"
    );
}

const SKILL: &str = include_str!("../skills/trier/SKILL.md");

#[test]
fn skill_installs_for_claude_under_home() {
    let home = test_dir("skill-claude");
    trier(&home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .arg("skill")
        .assert()
        .success();
    let path = home.join(".claude/skills/trier/SKILL.md");
    assert_eq!(std::fs::read_to_string(path).unwrap(), SKILL);
}

#[test]
fn skill_honors_claude_config_dir() {
    let home = test_dir("skill-claude-dir");
    let cfg = home.join("claude-cfg");
    trier(&home)
        .env("CLAUDE_CONFIG_DIR", &cfg)
        .arg("skill")
        .assert()
        .success();
    assert!(cfg.join("skills/trier/SKILL.md").is_file());
}

#[test]
fn skill_installs_for_shared_agents_dir() {
    let home = test_dir("skill-agents");
    trier(&home)
        .args(["skill", "--agent", "agents"])
        .assert()
        .success();
    assert!(home.join(".agents/skills/trier/SKILL.md").is_file());
}

#[test]
fn skill_path_writes_directly_into_the_directory() {
    let home = test_dir("skill-path");
    let dir = home.join("custom/trier");
    trier(&home)
        .args(["skill", "--path"])
        .arg(&dir)
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        SKILL
    );
}

#[test]
fn skill_refuses_to_overwrite_without_force() {
    let home = test_dir("skill-refuse");
    let dir = home.join("trier");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), "user edits").unwrap();
    let output = trier(&home)
        .args(["skill", "--path"])
        .arg(&dir)
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(stderr(&output).contains("--force"), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        "user edits"
    );
    trier(&home)
        .args(["skill", "--force", "--path"])
        .arg(&dir)
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        SKILL
    );
}

#[test]
fn skill_rejects_agent_together_with_path() {
    let home = test_dir("skill-conflict");
    trier(&home)
        .args(["skill", "--agent", "agents", "--path", "x"])
        .assert()
        .failure();
}

#[test]
fn completions_cover_every_subcommand_for_every_shell() {
    let home = test_dir("completions");
    for shell in ["bash", "zsh", "fish", "elvish"] {
        let output = trier(&home)
            .args(["completions", shell])
            .assert()
            .success()
            .get_output()
            .clone();
        let out = stdout(&output);
        for word in [
            "check",
            "explain",
            "open",
            "browsers",
            "register",
            "unregister",
            "status",
            "completions",
            "skill",
            "config",
            // fish spells long flags as `-l config`.
            if shell == "fish" {
                "-l config"
            } else {
                "--config"
            },
        ] {
            assert!(out.contains(word), "{shell} is missing `{word}`");
        }
    }
}

#[test]
fn completions_rejects_shells_trier_does_not_support() {
    let home = test_dir("completions-bad");
    trier(&home)
        .args(["completions", "powershell"])
        .assert()
        .failure();
}
