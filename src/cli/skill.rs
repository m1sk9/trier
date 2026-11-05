use std::io;
use std::path::{Path, PathBuf};

use super::WriteError;

pub const SKILL: &str = include_str!("../../skills/trier/SKILL.md");

/// Where an agent reads user-level skills from.
///
/// Only locations confirmed in the agents' own documentation are listed;
/// anything else is reached with `--path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    /// Claude Code: `$CLAUDE_CONFIG_DIR/skills` or `~/.claude/skills`.
    Claude,
    /// The shared `~/.agents/skills` directory (Codex and others).
    Agents,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("cannot locate the skills directory: HOME is not set (use --path)")]
    NoHome,
    #[error("{} already exists (use --force to overwrite)", .0.display())]
    AlreadyExists(PathBuf),
    // The io error is exposed via `source()`; repeating it here would make
    // anyhow print it twice.
    #[error("failed to write {}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// The directory that holds `SKILL.md` for `agent`.
///
/// The environment is passed in so tests need not mutate it.
pub fn default_dir(
    agent: Agent,
    home: Option<&Path>,
    claude_config_dir: Option<&Path>,
) -> Result<PathBuf, SkillError> {
    let skills = match agent {
        Agent::Claude => match claude_config_dir.filter(|dir| dir.is_absolute()) {
            Some(dir) => dir.join("skills"),
            None => home.ok_or(SkillError::NoHome)?.join(".claude/skills"),
        },
        Agent::Agents => home.ok_or(SkillError::NoHome)?.join(".agents/skills"),
    };
    Ok(skills.join("trier"))
}

/// Writes `SKILL.md` into `dir` and returns its path.
pub fn install(dir: &Path, force: bool) -> Result<PathBuf, SkillError> {
    let path = dir.join("SKILL.md");
    super::write_text(&path, SKILL, force)
        .map_err(|e| match e {
            WriteError::AlreadyExists => SkillError::AlreadyExists(path.clone()),
            WriteError::Io(source) => SkillError::Io {
                path: path.clone(),
                source,
            },
        })
        .map(|()| path)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::config::Config;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = crate::test_support::temp_dir(&format!("skill-{name}"));
        std::fs::remove_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn claude_skill_lives_under_dot_claude() {
        assert_eq!(
            default_dir(Agent::Claude, Some(Path::new("/home/me")), None).unwrap(),
            Path::new("/home/me/.claude/skills/trier")
        );
    }

    #[test]
    fn claude_config_dir_overrides_home() {
        assert_eq!(
            default_dir(
                Agent::Claude,
                Some(Path::new("/home/me")),
                Some(Path::new("/cfg/claude"))
            )
            .unwrap(),
            Path::new("/cfg/claude/skills/trier")
        );
    }

    #[test]
    fn relative_claude_config_dir_is_ignored() {
        assert_eq!(
            default_dir(
                Agent::Claude,
                Some(Path::new("/home/me")),
                Some(Path::new("relative"))
            )
            .unwrap(),
            Path::new("/home/me/.claude/skills/trier")
        );
    }

    #[test]
    fn shared_agents_skill_lives_under_dot_agents() {
        assert_eq!(
            default_dir(
                Agent::Agents,
                Some(Path::new("/home/me")),
                Some(Path::new("/cfg/claude"))
            )
            .unwrap(),
            Path::new("/home/me/.agents/skills/trier")
        );
    }

    #[test]
    fn missing_home_is_an_error_unless_claude_config_dir_is_set() {
        assert!(matches!(
            default_dir(Agent::Agents, None, None),
            Err(SkillError::NoHome)
        ));
        assert!(matches!(
            default_dir(Agent::Claude, None, None),
            Err(SkillError::NoHome)
        ));
        assert!(default_dir(Agent::Claude, None, Some(Path::new("/cfg"))).is_ok());
    }

    #[test]
    fn install_creates_parent_dirs_and_writes_the_skill() {
        let dir = temp_dir("create").join("nested/trier");
        let path = install(&dir, false).unwrap();
        assert_eq!(path, dir.join("SKILL.md"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), SKILL);
    }

    #[test]
    fn install_refuses_to_overwrite_without_force() {
        let dir = temp_dir("refuse");
        install(&dir, false).unwrap();
        std::fs::write(dir.join("SKILL.md"), "user edits").unwrap();
        assert!(matches!(
            install(&dir, false),
            Err(SkillError::AlreadyExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            "user edits"
        );
    }

    #[test]
    fn install_force_overwrites() {
        let dir = temp_dir("force");
        install(&dir, false).unwrap();
        std::fs::write(dir.join("SKILL.md"), "user edits").unwrap();
        install(&dir, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            SKILL
        );
    }

    #[test]
    fn install_io_errors_name_the_skill_file() {
        let dir = temp_dir("io");
        std::fs::create_dir_all(dir.join("SKILL.md")).unwrap();
        let err = install(&dir, true).unwrap_err();
        assert!(matches!(&err, SkillError::Io { path, .. } if *path == dir.join("SKILL.md")));
    }

    fn frontmatter() -> &'static str {
        SKILL
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .expect("SKILL.md starts with frontmatter")
            .0
    }

    #[test]
    fn frontmatter_follows_the_agent_skills_spec() {
        let field = |key: &str| {
            frontmatter()
                .lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        // The name must match the directory `default_dir` installs into.
        assert_eq!(field("name"), "trier");
        let description = field("description");
        assert!((1..=1024).contains(&description.chars().count()));
    }

    #[test]
    fn config_example_in_the_skill_is_a_valid_config() {
        let block = SKILL
            .split("```toml\n")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .expect("SKILL.md has a toml block");
        block.parse::<Config>().unwrap();
    }

    #[test]
    fn every_builtin_browser_is_listed_in_the_skill() {
        for (name, _) in crate::browser::BUILTIN {
            assert!(SKILL.contains(&format!("`{name}`")), "{name} is missing");
        }
    }
}
