//! What the scriptorium holds, as a listing: every quire, the sessions gathered
//! in it, and how each of them is labelled for a reader choosing one.
//!
//! [`discovery`] answers where the sessions are; this answers what they look
//! like on a shelf. The split is worth keeping because the two are asked at
//! different rates: locating a session is a directory walk, while labelling one
//! means reading it, which is why the titles are memoized in [`Peeks`] rather
//! than recovered on every listing.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::Result;

use crate::{discovery, transcript::Folio};

/// How recently a session must have been written to count as one still being
/// written. Generous enough to cover a turn the model is thinking through, short
/// enough that a session put down for a coffee stops claiming to be live.
const STILL_WARM: Duration = Duration::from_secs(90);

/// Every quire the projects root holds, most recently active first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    pub quires: Vec<Gathering>,
}

/// One project's gathering of sessions, most recently written first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gathering {
    /// The encoded directory name, which is what a URL names a quire by. It is
    /// Claude Code's own flattening of the project path, so it is stable, and
    /// it holds no separators, so nothing built from it can leave the root.
    pub id: String,
    pub dir: PathBuf,
    pub sessions: Vec<Listed>,
}

/// One session, as a listing shows it: what it is called in a URL, where it is,
/// and what a reader judges it by before opening it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The file stem, which is the session id Claude Code named the file after.
    pub id: String,
    pub path: PathBuf,
    pub modified: SystemTime,
    pub bytes: u64,
}

impl Catalogue {
    /// Stats every session under `root`. No session is read, so this is a
    /// directory walk however large the corpus is; titles come from [`Peeks`].
    pub fn scan(root: &Path) -> Result<Self> {
        let quires = discovery::all_quires(root)?
            .into_iter()
            .filter_map(Gathering::of)
            .filter(|quire| !quire.sessions.is_empty())
            .collect();
        Ok(Self { quires })
    }

    pub fn quire(&self, id: &str) -> Option<&Gathering> {
        self.quires.iter().find(|quire| quire.id == id)
    }

    /// The session this id names, wherever it is filed.
    ///
    /// **This is how a session id becomes a path, and the only way it should.**
    /// A URL names a session by an id, and looking one up here is what keeps a
    /// request from composing a path of its own: an id that is not in the
    /// listing has no path at all, so reaching outside the root is not
    /// something to defend against but something that cannot be expressed.
    pub fn session(&self, id: &str) -> Option<&Listed> {
        self.quires
            .iter()
            .flat_map(|quire| &quire.sessions)
            .find(|session| session.id == id)
    }
}

impl Gathering {
    /// A quire as a listing, or `None` for one whose directory name cannot be
    /// read as text: it could not be named in a URL, and Claude Code's own
    /// encoding never produces one.
    fn of(quire: discovery::Quire) -> Option<Self> {
        let id = quire.dir.file_name()?.to_str()?.to_owned();
        let sessions = quire
            .sessions
            .iter()
            .filter_map(|session| Listed::of(session))
            .collect();
        Some(Self {
            id,
            dir: quire.dir,
            sessions,
        })
    }

    /// When this quire was last worked in, which is its most recent session's
    /// stamp: `sessions` is ordered by it, so the head carries the answer.
    pub fn modified(&self) -> Option<SystemTime> {
        self.sessions.first().map(|session| session.modified)
    }

    pub fn latest(&self) -> Option<&Listed> {
        self.sessions.first()
    }
}

impl Listed {
    fn of(path: &Path) -> Option<Self> {
        let metadata = path.metadata().ok()?;
        Some(Self {
            id: path.file_stem()?.to_str()?.to_owned(),
            path: path.to_owned(),
            modified: metadata.modified().ok()?,
            bytes: metadata.len(),
        })
    }

    /// Whether this session is still being written, which is the one a reader
    /// opening a codex is usually looking for.
    pub fn is_live(&self, now: SystemTime) -> bool {
        now.duration_since(self.modified)
            .is_ok_and(|since| since < STILL_WARM)
    }
}

/// The titles a listing shows, remembered so a listing costs a read only for
/// what has changed.
///
/// [`Folio::peek`] reads a whole session file to recover its title, which is
/// affordable once and not once per listing: a corpus is hundreds of megabytes
/// and a codex re-lists on a timer. A session is unchanged when its stamp and
/// its length are, and a session file is only ever appended to, so a stale entry
/// is not a risk a length can hide.
#[derive(Debug, Default)]
pub struct Peeks {
    held: HashMap<PathBuf, (Stamp, Peeked)>,
}

/// What a peek recovered, reduced to the two things a listing shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peeked {
    pub title: Option<String>,
    pub project: Option<PathBuf>,
}

type Stamp = (SystemTime, u64);

impl Peeks {
    /// This session's title and working directory, read only if it has changed
    /// since it was last read.
    pub fn of(&mut self, session: &Listed) -> &Peeked {
        let stamp = (session.modified, session.bytes);
        let entry = self.held.entry(session.path.clone());
        let (held, peeked) = entry.or_insert_with(|| (stamp, peek(&session.path)));
        if *held != stamp {
            *held = stamp;
            *peeked = peek(&session.path);
        }
        peeked
    }

    /// What to call a quire: the working directory its most recent session ran
    /// in, since the encoded directory name is lossy (every separator and dot
    /// flattened to a dash). Falls back to that name when no session says.
    pub fn project(&mut self, quire: &Gathering) -> String {
        quire
            .latest()
            .and_then(|session| self.of(session).project.clone())
            .map(|project| project.display().to_string())
            .unwrap_or_else(|| quire.dir.display().to_string())
    }

    /// How a session is labelled in a listing, which is Claude's own title for
    /// it, or its opening prompt, or an admission that it has neither.
    pub fn title(&mut self, session: &Listed) -> String {
        self.of(session)
            .title
            .as_deref()
            .map(condense)
            .unwrap_or_else(|| "(untitled)".to_owned())
    }

    /// Forgets sessions the listing no longer holds, so a long-running codex
    /// doesn't accumulate an entry per session ever deleted under it.
    pub fn keep_to(&mut self, catalogue: &Catalogue) {
        self.held.retain(|path, _| {
            catalogue
                .quires
                .iter()
                .flat_map(|quire| &quire.sessions)
                .any(|session| &session.path == path)
        });
    }
}

/// How many of a quire's sessions the codex page shows before sending the reader
/// to the quire's own page. Enough that the session being written right now is
/// on the front page of any project worked in today, few enough that the front
/// page costs a bounded number of reads however long a project has run.
const PREVIEWED: usize = 5;

/// A listing with every label already looked up: the titles read, the stamps
/// turned into words. The renderer sets this and reads nothing, so a page is a
/// function of a value the way a folio is (see the purity `Scribe` keeps).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shelf {
    pub quires: Vec<Shelved>,
}

/// One quire as a page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shelved {
    pub id: String,
    /// The working directory the project's sessions ran in.
    pub project: String,
    /// How long ago it was last worked in.
    pub when: String,
    pub sessions: Vec<Leaf>,
    /// Sessions this listing left out, which the quire's own page shows.
    pub more: usize,
}

/// One session as a page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub id: String,
    pub title: String,
    pub when: String,
    pub bytes: u64,
    /// Whether it is still being written, which is what a reader opening a
    /// codex is usually looking for.
    pub is_live: bool,
}

impl Shelf {
    /// The whole codex: every quire, each previewing its most recent sessions.
    pub fn of(catalogue: &Catalogue, peeks: &mut Peeks, now: SystemTime) -> Self {
        let quires = catalogue
            .quires
            .iter()
            .map(|quire| Shelved::of(quire, peeks, now, PREVIEWED))
            .collect();
        Self { quires }
    }
}

impl Shelf {
    /// Every word on this page that came out of a transcript rather than out of
    /// this crate, for weighing against the cut faces. A title is a session's own
    /// text and a project path is the machine's, so a listing can reach beyond
    /// the cut exactly as a folio can.
    pub fn labels(&self) -> String {
        self.quires
            .iter()
            .map(Shelved::labels)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Shelved {
    /// One quire, showing at most `most` of its sessions.
    pub fn of(quire: &Gathering, peeks: &mut Peeks, now: SystemTime, most: usize) -> Self {
        let sessions = quire
            .sessions
            .iter()
            .take(most)
            .map(|session| Leaf {
                id: session.id.clone(),
                title: peeks.title(session),
                when: ago(now, session.modified),
                bytes: session.bytes,
                is_live: session.is_live(now),
            })
            .collect();
        Self {
            id: quire.id.clone(),
            project: peeks.project(quire),
            when: quire
                .modified()
                .map(|modified| ago(now, modified))
                .unwrap_or_default(),
            sessions,
            more: quire.sessions.len().saturating_sub(most),
        }
    }

    /// One quire, showing all of it, which is what its own page is for.
    pub fn whole(quire: &Gathering, peeks: &mut Peeks, now: SystemTime) -> Self {
        Shelved::of(quire, peeks, now, usize::MAX)
    }

    /// Whether any of the sessions shown here is still being written.
    pub fn is_live(&self) -> bool {
        self.sessions.iter().any(|session| session.is_live)
    }

    /// See [`Shelf::labels`].
    pub fn labels(&self) -> String {
        let mut labels = self.project.clone();
        for session in &self.sessions {
            labels.push('\n');
            labels.push_str(&session.title);
        }
        labels
    }
}

fn peek(path: &Path) -> Peeked {
    let peeked = Folio::peek(path);
    Peeked {
        title: peeked.title,
        project: peeked.cwd,
    }
}

/// A single-line label from a title that may contain newlines or run long.
pub fn condense(title: &str) -> String {
    const MAX: usize = 72;
    let line: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line,
    }
}

/// A coarse "how long ago" for ordering intuition, not precision. `now` is
/// passed in rather than read, so the answer is a function of its arguments and
/// a listing can be tested without waiting for the clock.
pub fn ago(now: SystemTime, then: SystemTime) -> String {
    let Ok(elapsed) = now.duration_since(then) else {
        return "just now".to_owned();
    };
    let seconds = elapsed.as_secs();
    match seconds {
        ..60 => "just now".to_owned(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86_400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::UNIX_EPOCH};

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn listed(path: &Path, seconds: u64) -> Listed {
        Listed {
            id: "abc".to_owned(),
            path: path.to_owned(),
            modified: at(seconds),
            bytes: 12,
        }
    }

    #[test]
    fn a_title_becomes_one_line_and_is_cut_to_length() {
        assert_eq!(condense("two\nlines  here"), "two lines here");
        let long = "w ".repeat(80);
        assert!(condense(&long).ends_with('…'));
        assert_eq!(condense(&long).chars().count(), 73);
    }

    #[test]
    fn how_long_ago_climbs_units() {
        assert_eq!(ago(at(1_000), at(970)), "just now");
        assert_eq!(ago(at(1_000), at(400)), "10m ago");
        assert_eq!(ago(at(90_000), at(10_000)), "22h ago");
        assert_eq!(ago(at(900_000), at(10_000)), "10d ago");
    }

    /// A stamp from the future is a clock that stepped back, not a session
    /// written ahead of time, and "just now" is the honest reading.
    #[test]
    fn a_session_stamped_ahead_of_the_clock_reads_as_just_now() {
        assert_eq!(ago(at(1_000), at(9_000)), "just now");
    }

    #[test]
    fn a_session_written_within_the_window_is_still_live() {
        let session = listed(Path::new("live.jsonl"), 1_000);

        assert!(session.is_live(at(1_030)));
        assert!(!session.is_live(at(9_000)));
    }

    #[test]
    fn a_peek_is_reread_only_once_the_session_has_changed() {
        let dir = std::env::temp_dir().join(format!("catalogue-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        fs::write(
            &path,
            "{\"type\":\"ai-title\",\"aiTitle\":\"First light\"}\n",
        )
        .unwrap();

        let mut peeks = Peeks::default();
        let session = listed(&path, 1_000);
        assert_eq!(peeks.title(&session), "First light");

        // The same stamp: whatever the file now says, the listing keeps what it
        // read, which is the whole point of holding it.
        fs::write(&path, "{\"type\":\"ai-title\",\"aiTitle\":\"Second\"}\n").unwrap();
        assert_eq!(peeks.title(&session), "First light");

        // A new stamp, so it is read again.
        let grown = listed(&path, 2_000);
        assert_eq!(peeks.title(&grown), "Second");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_untitled_session_says_so() {
        let mut peeks = Peeks::default();

        assert_eq!(
            peeks.title(&listed(Path::new("nowhere.jsonl"), 1)),
            "(untitled)"
        );
    }
}
