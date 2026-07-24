//! Locating session files under Claude Code's project store.

use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};

/// Every session Claude Code has recorded for one project directory.
#[derive(Debug)]
pub struct Quire {
    pub dir: PathBuf,
    /// Session files, most recently modified first.
    pub sessions: Vec<PathBuf>,
}

impl Quire {
    pub fn latest(&self) -> Result<&Path> {
        self.sessions
            .first()
            .map(PathBuf::as_path)
            .ok_or_else(|| anyhow!("no sessions recorded in {}", self.dir.display()))
    }
}

/// The root Claude Code writes project transcripts under.
pub fn projects_root() -> Result<PathBuf> {
    if let Some(configured) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(configured).join("projects"));
    }
    let home = std::env::home_dir().context("locating home directory")?;
    Ok(home.join(".claude").join("projects"))
}

/// Claude Code names each project directory after its path, with everything
/// outside `[A-Za-z0-9_]` flattened to a dash.
pub fn encode_project_path(project: &Path) -> String {
    project
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn quire_for(root: &Path, project: &Path) -> Result<Quire> {
    let dir = root.join(encode_project_path(project));
    if !dir.is_dir() {
        bail!(
            "no recorded sessions for {} (looked in {})",
            project.display(),
            dir.display()
        );
    }
    Ok(Quire {
        sessions: sessions_in(&dir)?,
        dir,
    })
}

/// Session files in one project directory, most recently modified first.
///
/// `agent-*.jsonl` holds a subagent's own transcript, which is reachable from
/// the parent session it was spawned by.
fn sessions_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("agent-") {
            continue;
        }
        found.push((entry.metadata()?.modified()?, path));
    }
    found.sort_by_key(|(modified, _)| Reverse(*modified));
    Ok(found.into_iter().map(|(_, path)| path).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_and_dots_flatten_to_dashes() {
        assert_eq!(
            encode_project_path(Path::new("/home/scribe/projects/dot.ted")),
            "-home-scribe-projects-dot-ted"
        );
    }

    #[test]
    fn underscores_survive_encoding() {
        assert_eq!(
            encode_project_path(Path::new("/srv/quire_two")),
            "-srv-quire_two"
        );
    }
}
