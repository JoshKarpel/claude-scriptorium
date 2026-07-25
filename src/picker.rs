//! Interactive selection of a session to render, when the user names none.
//!
//! Two stages: pick a project, then a session within it. The current project
//! floats to the top and every list starts on its first row, so accepting both
//! defaults (Enter, Enter) lands on the current project's most recent session,
//! the same thing `--latest` resolves non-interactively.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Result, bail};
use inquire::{InquireError, Select};

use crate::{discovery, transcript::Folio};

/// Walks the project and session pickers, returning the chosen session file.
pub fn pick_session(root: &Path, cwd: &Path) -> Result<PathBuf> {
    let quire = pick_project(root, cwd)?;
    pick_from_quire(&quire)
}

fn pick_project(root: &Path, cwd: &Path) -> Result<discovery::Quire> {
    let mut quires = discovery::all_quires(root)?;
    if quires.is_empty() {
        bail!("no recorded sessions under {}", root.display());
    }

    // Float the current project to the top so its most recent session is one
    // Enter away. Matching is on the encoded directory name because that is
    // exactly how Claude Code derived the directory from this path.
    let current = discovery::encode_project_path(cwd);
    if let Some(here) = quires
        .iter()
        .position(|quire| quire.dir.file_name().is_some_and(|name| name == &*current))
    {
        quires[..=here].rotate_right(1);
    }

    let choices = quires.into_iter().map(ProjectChoice::new).collect();
    select("Project", choices).map(|choice| choice.quire)
}

fn pick_from_quire(quire: &discovery::Quire) -> Result<PathBuf> {
    let choices = quire
        .sessions
        .iter()
        .map(|session| SessionChoice::new(session.clone()))
        .collect();
    select("Session", choices).map(|choice| choice.path)
}

/// A project the user can pick, labelled by the real working directory the
/// session ran in (recovered from the transcript, since the encoded directory
/// name is lossy).
struct ProjectChoice {
    quire: discovery::Quire,
    label: String,
}

impl ProjectChoice {
    fn new(quire: discovery::Quire) -> Self {
        let label = quire
            .latest()
            .ok()
            .map(Folio::peek)
            .and_then(|peek| peek.cwd)
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_else(|| quire.dir.display().to_string());
        Self { quire, label }
    }
}

impl fmt::Display for ProjectChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// A session the user can pick, labelled by how long ago it was touched and
/// Claude's own title for it (or the first prompt when it has no title yet).
struct SessionChoice {
    path: PathBuf,
    label: String,
}

impl SessionChoice {
    fn new(path: PathBuf) -> Self {
        let when = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map(relative_time)
            .unwrap_or_else(|_| "?".to_owned());
        let title = Folio::peek(&path)
            .title
            .map(|title| condense(&title))
            .unwrap_or_else(|| "(untitled)".to_owned());
        Self {
            label: format!("{when:>9}  {title}"),
            path,
        }
    }
}

impl fmt::Display for SessionChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Prompts with a scrollable, fuzzy-filterable list, turning a cancellation
/// (Esc / Ctrl-C) into a clean exit rather than an error dump.
fn select<T: fmt::Display>(message: &str, choices: Vec<T>) -> Result<T> {
    match Select::new(message, choices).with_page_size(15).prompt() {
        Ok(choice) => Ok(choice),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            bail!("no session selected")
        }
        Err(error) => Err(error.into()),
    }
}

/// A single-line label from a title that may contain newlines or run long.
fn condense(title: &str) -> String {
    const MAX: usize = 72;
    let line: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line,
    }
}

/// A coarse "how long ago" for ordering intuition, not precision.
fn relative_time(modified: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return "just now".to_owned();
    };
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        "just now".to_owned()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}
