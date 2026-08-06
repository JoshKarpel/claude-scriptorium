//! Cloistering the codex: a systemd user service that keeps every recorded
//! session served without anyone attending it.
//!
//! It is a *user* unit rather than a system one, and that follows from what a
//! codex serves. The sessions are one user's, under their own home; a system
//! unit would need root to install and would then have to be told which user's
//! transcripts to read back. A user unit needs no privilege, and lingering
//! (`loginctl enable-linger`) is what makes it outlive the session that
//! installed it.
//!
//! The unit file is the single declaration of what is cloistered here. Nothing
//! is recorded alongside it: an install writes every argument out explicitly,
//! and [`Standing`] reads the address back out of the unit rather than
//! restating what an install once meant, so a hand-edited unit is reported as
//! it now stands.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail};

/// The unit installed when none is named. One machine serving one codex is the
/// case this exists for; `--name` is for the second one.
pub const DEFAULT_UNIT: &str = "claude-scriptorium-codex";

/// What to cloister: the binary to run, the arguments it serves under, and the
/// unit that will hold it. A value, so [`Charter::unit_file`] is a pure
/// function of it and the shell below is the only part that touches systemd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charter {
    /// Unit name, without the `.service` suffix.
    pub name: String,
    /// Absolute path to the binary, so the unit runs the same one that wrote it
    /// rather than whatever a `PATH` resolves to years later.
    pub program: PathBuf,
    /// The state of that binary when the unit was written, as one word (see
    /// [`stamped`]).
    ///
    /// `ExecStart` names a path and not a version, so installing a newer build
    /// over the same path leaves the unit byte-identical: without this a re-run
    /// after an upgrade would find nothing changed, skip the restart, and report
    /// that the unit was already written while the service went on running the
    /// code that was replaced. Recording it puts the binary's identity in the
    /// declaration, so a rebuild *is* a change and converges the same way every
    /// other change does.
    pub program_stamp: String,
    /// The `codex` invocation, every argument spelled out. A user unit inherits
    /// none of the installing shell's environment, so anything the codex would
    /// otherwise read from it (`CLAUDE_CONFIG_DIR`, most of all) has to be
    /// resolved here and written down.
    pub arguments: Vec<String>,
    /// Where the service runs, which is only ever the user's home: nothing the
    /// codex does is relative to a directory, so this is for a core dump to
    /// land somewhere sane rather than for the program.
    pub home: PathBuf,
}

impl Charter {
    pub fn unit(&self) -> String {
        format!("{}.service", self.name)
    }

    /// The systemd unit file, verbatim.
    ///
    /// `Type=exec` rather than `simple`: it holds the start incomplete until the
    /// binary has actually been executed, so a mistyped `ExecStart` fails the
    /// install loudly instead of reporting a service that started and instantly
    /// died. `WantedBy=default.target` is a user manager's own boot target.
    ///
    /// Nothing is ordered against `network-online.target`. It is a system unit
    /// and a user manager has no such target to wait on, and a codex binds a
    /// socket rather than reaching out, so there is nothing to wait for.
    ///
    /// The binary's own stamp is written as a comment rather than a directive:
    /// systemd has nothing to do with it, it exists so that rebuilding the
    /// binary changes this text (see [`Charter::program_stamp`]), and a comment
    /// is the one thing every version of systemd is guaranteed to ignore.
    pub fn unit_file(&self) -> String {
        let command = std::iter::once(quoted(&self.program.to_string_lossy()))
            .chain(self.arguments.iter().map(|argument| quoted(argument)))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "\
[Unit]
Description=claude-scriptorium codex, serving every recorded session
Documentation={home}
# Installed from a binary of {stamp}.

[Service]
Type=exec
ExecStart={command}
WorkingDirectory={directory}
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
",
            home = env!("CARGO_PKG_REPOSITORY"),
            stamp = self.program_stamp,
            directory = self.home.display(),
        )
    }
}

/// The state of a binary, as one word: when it was last written and how large it
/// is. Enough to tell one build from another over the same path, which is all
/// [`Charter::program_stamp`] asks of it. A binary whose metadata cannot be read
/// has no state to name, and says so rather than claiming an identity.
pub fn stamped(program: &Path) -> String {
    let Ok(metadata) = program.metadata() else {
        return "unknown".to_owned();
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs())
        .unwrap_or(0);
    format!("{modified}-{}", metadata.len())
}

/// Quotes an argument for a systemd `ExecStart`, which splits on whitespace
/// unless a word is quoted. A home directory with a space in it is otherwise a
/// unit that runs a different command than the one it was given.
fn quoted(argument: &str) -> String {
    if !argument.is_empty()
        && argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=".contains(c))
    {
        return argument.to_owned();
    }
    format!("\"{}\"", argument.replace('\\', r"\\").replace('"', "\\\""))
}

/// What a codex binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub host: String,
    pub port: u16,
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// The address the unit's `ExecStart` binds, recovered from the unit file
/// itself. An install writes `--host` and `--port` out in full, so this needs no
/// notion of what the defaults are and cannot drift from them; a unit edited to
/// drop either simply has no address to report.
pub fn bound(unit_file: &str) -> Option<Address> {
    let arguments = exec_start(unit_file)?;
    let value = |flag: &str| {
        arguments
            .iter()
            .position(|argument| argument == flag)
            .and_then(|at| arguments.get(at.checked_add(1)?))
            .cloned()
    };
    Some(Address {
        host: value("--host")?,
        port: value("--port")?.parse().ok()?,
    })
}

/// The words of a unit file's `ExecStart`, unquoted. Deliberately lenient about
/// systemd's full escaping grammar: it reads back what [`Charter::unit_file`]
/// writes, and anything it cannot make sense of is reported as no address
/// rather than as a wrong one.
fn exec_start(unit_file: &str) -> Option<Vec<String>> {
    let line = unit_file
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ExecStart="))?;

    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoting = false;
    let mut escaped = false;
    for character in line.chars() {
        match character {
            _ if escaped => {
                word.push(character);
                escaped = false;
            }
            '\\' if quoting => escaped = true,
            '"' => quoting = !quoting,
            c if c.is_whitespace() && !quoting => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// Where a user unit of this name lives.
pub fn unit_path(name: &str) -> Result<PathBuf> {
    let home = std::env::home_dir().context("locating home directory")?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(format!("{name}.service")))
}

/// What an install did, so the shell reports it rather than this module
/// printing.
pub struct Installed {
    pub path: PathBuf,
    /// Whether what is cloistered changed: the arguments, the paths, or the
    /// binary itself, all of which the unit's text states. A re-run that changes
    /// none of them does not restart the service, so a reader with the page open
    /// keeps their stream.
    pub changed: bool,
    pub linger: Linger,
}

/// Whether the user manager will keep the service running with nobody logged
/// in. Without lingering a user unit is a session process, and the codex stops
/// the moment the installing SSH connection closes.
pub enum Linger {
    Enabled,
    /// `loginctl` refused, with its own diagnostic. The service is running for
    /// this session either way, so this is reported rather than fatal.
    Refused(String),
}

/// Writes the unit, reloads, enables, and starts it, converging from whatever
/// state the machine was already in.
///
/// The write is what decides everything after it: the daemon reload and the
/// restart are the cost of a *changed* unit and are skipped when the text is
/// already what it should be, so re-running is genuinely free rather than
/// merely harmless. That rests on the text stating everything a re-run could
/// have changed, the binary included, which is what
/// [`Charter::program_stamp`] is for.
pub fn install(charter: &Charter) -> Result<Installed> {
    let path = unit_path(&charter.name)?;
    let directory = path
        .parent()
        .expect("a unit path always has its user unit directory as its parent");
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;

    let wanted = charter.unit_file();
    let changed = fs::read_to_string(&path).ok().as_deref() != Some(wanted.as_str());
    if changed {
        fs::write(&path, &wanted).with_context(|| format!("writing {}", path.display()))?;
        systemctl(&["daemon-reload"])?;
    }

    let linger = linger();
    let unit = charter.unit();
    systemctl(&["enable", &unit])?;
    // `restart` starts a stopped unit as well as replacing a running one, so a
    // changed unit needs no test of whether it was up.
    systemctl(&[if changed { "restart" } else { "start" }, &unit])?;

    Ok(Installed {
        path,
        changed,
        linger,
    })
}

/// How a unit stands with the user manager, as its own manager reports it.
pub struct Standing {
    pub path: PathBuf,
    /// The unit file as it is on disk, absent when nothing is cloistered under
    /// this name.
    pub unit_file: Option<String>,
    /// What `systemctl is-enabled` answers, verbatim: `enabled`, `disabled`,
    /// `not-found`, and the rest of systemd's own vocabulary.
    pub enabled: String,
    /// What `systemctl is-active` answers, verbatim: `active`, `inactive`,
    /// `failed`, `activating`.
    pub active: String,
}

impl Standing {
    /// The address the unit binds, read back out of the unit itself.
    pub fn bound(&self) -> Option<Address> {
        self.unit_file.as_deref().and_then(bound)
    }
}

/// Reports a unit's standing. Both queries answer on stdout for a unit that
/// does not exist (`not-found`, `inactive`) as readily as for one that does, so
/// this reads their output rather than their exit status, which is nonzero for
/// every answer but the affirmative one.
pub fn status(name: &str) -> Result<Standing> {
    let path = unit_path(name)?;
    let unit = format!("{name}.service");
    Ok(Standing {
        unit_file: fs::read_to_string(&path).ok(),
        path,
        enabled: reported(&["is-enabled", &unit])?,
        active: reported(&["is-active", &unit])?,
    })
}

/// What a removal found to undo.
pub struct Removed {
    pub path: PathBuf,
    /// Whether a unit file was there to delete. A remove that finds nothing has
    /// still converged on the state it was asked for, so it is not an error.
    pub existed: bool,
}

/// Stops, disables, and deletes the unit.
///
/// `disable --now` is one call rather than a stop and a disable, and it does
/// not mind a unit that is already stopped. What it does mind is a unit file
/// that is already gone, so the delete comes last.
pub fn remove(name: &str) -> Result<Removed> {
    let path = unit_path(name)?;
    let existed = path.exists();
    if existed {
        systemctl(&["disable", "--now", &format!("{name}.service")])?;
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        systemctl(&["daemon-reload"])?;
    }
    Ok(Removed { path, existed })
}

/// Asks `loginctl` to keep this user's manager running with nobody logged in,
/// which is what makes a user unit a service rather than a session process.
///
/// A refusal is reported rather than raised. It is one machine's policy (a
/// polkit rule wanting an interactive authentication) and the install has still
/// produced a running service; what it costs the user is that the service stops
/// when they log out, which the shell says outright.
fn linger() -> Linger {
    let account = match account() {
        Ok(account) => account,
        Err(error) => return Linger::Refused(error.to_string()),
    };
    let ran = Command::new("loginctl")
        .arg("enable-linger")
        .arg(&account)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match ran {
        Ok(output) if output.status.success() => Linger::Enabled,
        Ok(output) => Linger::Refused(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Linger::Refused(error.to_string()),
    }
}

/// Runs `systemctl --user`, failing loudly on a nonzero exit.
fn systemctl(arguments: &[&str]) -> Result<()> {
    let status = user_manager()
        .args(arguments)
        .status()
        .context("running systemctl (is this machine running systemd?)")?;
    if !status.success() {
        bail!("systemctl --user {} failed", arguments.join(" "));
    }
    Ok(())
}

/// Runs `systemctl --user` for an answer, returning its stdout trimmed and
/// ignoring the exit status, which these queries use to carry the answer.
fn reported(arguments: &[&str]) -> Result<String> {
    let output = user_manager()
        .args(arguments)
        .stderr(Stdio::null())
        .output()
        .context("running systemctl (is this machine running systemd?)")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The account whose user manager holds the unit. Both `login` and `sshd` set
/// these for any shell that reaches this code, so neither being set is a
/// machine this cannot address; it says so rather than guessing at a name.
fn account() -> Result<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .context("neither $USER nor $LOGNAME is set, so there is no user manager to address")
}

/// A `systemctl --user` invocation that can reach the user manager.
///
/// `systemctl --user` finds its manager through `XDG_RUNTIME_DIR`, which a login
/// shell has and a command run as `ssh host cmd`, or from a first-boot setup
/// script, does not: without it every call fails with `Failed to connect to bus:
/// No medium found`, which reads as systemd being absent and is really the
/// environment being thin. `loginctl` knows the path, so it is asked for it
/// rather than the conventional `/run/user/<uid>` being composed here.
fn user_manager() -> Command {
    let mut command = Command::new("systemctl");
    command.arg("--user");
    if std::env::var_os("XDG_RUNTIME_DIR").is_none()
        && let Some(runtime) = runtime_path()
    {
        command.env("XDG_RUNTIME_DIR", runtime);
    }
    command
}

/// Where this user's manager keeps its runtime state, as `loginctl` reports it.
fn runtime_path() -> Option<String> {
    let account = account().ok()?;
    let output = Command::new("loginctl")
        .args(["show-user", &account, "--value", "-p", "RuntimePath"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn charter() -> Charter {
        Charter {
            name: "scriptorium-test".to_owned(),
            program: PathBuf::from("/opt/bin/claude-scriptorium"),
            program_stamp: "1754400000-9123456".to_owned(),
            arguments: ["codex", "--host", "127.0.0.1", "--port", "8123", "--root"]
                .iter()
                .map(|argument| (*argument).to_owned())
                .chain(std::iter::once("/home/scribe/.claude/projects".to_owned()))
                .collect(),
            home: PathBuf::from("/home/scribe"),
        }
    }

    #[test]
    fn unit_file_runs_the_binary_that_wrote_it() {
        let unit = charter().unit_file();
        assert!(unit.contains(
            "ExecStart=/opt/bin/claude-scriptorium codex --host 127.0.0.1 --port 8123 --root /home/scribe/.claude/projects"
        ));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("WorkingDirectory=/home/scribe"));
    }

    #[test]
    fn unit_file_is_the_same_text_every_time() {
        assert_eq!(charter().unit_file(), charter().unit_file());
    }

    /// `ExecStart` names a path rather than a version, so a rebuilt binary
    /// leaves it identical. The unit has to change anyway, or a re-run after an
    /// upgrade would skip the restart and leave the old process serving.
    #[test]
    fn a_rebuilt_binary_writes_a_different_unit() {
        let installed = charter().unit_file();
        let mut rebuilt = charter();
        rebuilt.program_stamp = "1754500000-9200000".to_owned();

        assert_ne!(rebuilt.unit_file(), installed);
    }

    /// The stamp is this crate's own bookkeeping, so it must not reach systemd
    /// as something systemd will try to make sense of.
    #[test]
    fn the_binary_stamp_is_written_as_a_comment() {
        let unit = charter().unit_file();

        assert!(unit.contains("# Installed from a binary of 1754400000-9123456."));
        assert_eq!(
            exec_start(&unit),
            exec_start(&unit.replace("1754400000", "0"))
        );
    }

    #[test]
    fn a_path_with_a_space_is_quoted_so_systemd_keeps_it_whole() {
        let mut charter = charter();
        charter.program = PathBuf::from("/opt/my tools/claude-scriptorium");
        let unit = charter.unit_file();
        assert!(unit.contains(r#"ExecStart="/opt/my tools/claude-scriptorium" codex"#));
        assert_eq!(
            exec_start(&unit).unwrap()[0],
            "/opt/my tools/claude-scriptorium"
        );
    }

    #[test]
    fn a_quote_in_an_argument_survives_the_round_trip() {
        let mut charter = charter();
        charter.arguments = vec!["codex".to_owned(), r#"--root=/a "quoted" dir"#.to_owned()];
        let recovered = exec_start(&charter.unit_file()).unwrap();
        assert_eq!(recovered.last().unwrap(), r#"--root=/a "quoted" dir"#);
    }

    #[test]
    fn the_address_is_read_back_out_of_the_unit() {
        let bound = bound(&charter().unit_file()).unwrap();
        assert_eq!(bound.host, "127.0.0.1");
        assert_eq!(bound.port, 8123);
    }

    #[test]
    fn a_unit_whose_port_is_not_a_number_reports_no_address() {
        let unit = "ExecStart=/opt/bin/cs codex --host 0.0.0.0 --port http\n";
        assert_eq!(bound(unit), None);
    }

    #[test]
    fn a_unit_stating_no_port_reports_no_address_rather_than_a_default() {
        let unit = "[Service]\nExecStart=/opt/bin/claude-scriptorium codex --host 0.0.0.0\n";
        assert_eq!(bound(unit), None);
    }

    #[test]
    fn a_hand_edited_address_is_what_gets_reported() {
        let edited = charter().unit_file().replace("8123", "9000");
        assert_eq!(bound(&edited).unwrap().to_string(), "127.0.0.1:9000");
    }

    #[test]
    fn a_file_with_no_exec_start_reports_no_address() {
        assert_eq!(bound("[Unit]\nDescription=nothing here\n"), None);
    }
}
