//! What the harness wrote into a session, and how each note is set.
//!
//! A gloss is a note entered by a hand other than the conversation's: a hook's
//! output, a rule file pulled into context, the instructions a skill carries,
//! a slash command the user typed, a plan-mode boundary. None of it is the
//! conversation and all of it is why the conversation went the way it did, so
//! the folio sets it rather than dropping it.
//!
//! The shapes here are the harness's rather than a contract, so every reading
//! is lenient and answers `Option`: a note whose shape this doesn't recognise
//! is left unset rather than aborting a render, the same way a tool with no
//! view of its own falls back in [`crate::tools`]. What a note has to say is
//! also what decides whether it is set at all: one that only reports that
//! something ran is dropped, as an acknowledging tool result is.

use std::borrow::Cow;

use maud::html;
use serde_json::Value;

use crate::{
    render::Scribe,
    tools::{Setting, plain, printed, prose},
};

/// What entered the note, which is what its panel is labelled by. The summary
/// line says which hook, which file, which command; the label says only who
/// wrote it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossKind {
    Hook,
    Rule,
    Skill,
    Command,
    Plan,
    Note,
}

impl GlossKind {
    pub fn label(self) -> &'static str {
        match self {
            GlossKind::Hook => "hook",
            GlossKind::Rule => "rule",
            GlossKind::Skill => "skill",
            GlossKind::Command => "command",
            GlossKind::Plan => "plan",
            GlossKind::Note => "note",
        }
    }
}

/// What a gloss's fold holds, and how the body should be set.
///
/// Three readings, not two, because a hook's output sits between them: a program
/// printed it, so its line breaks are its own and must be kept, but it is
/// markdown often enough (headings, lists) that setting it as preformatted text
/// throws away structure a reader wants. [`Body::Printed`] is that middle
/// reading, and [`crate::render::Scribe::markdown_printed`] says what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Text composed as markdown, which reflows: a skill's instructions, a rule
    /// pulled into context, a plan.
    Prose(String),
    /// Text a program printed, read as markdown but keeping its own line breaks.
    Printed(String),
    /// Output with no shape of its own, set exactly as it came.
    Plain(String),
}

/// Which firing of a hook a note belongs to. One hook writes several lines
/// (what it decided, what it injected, what it printed), and this is what lets
/// [`crate::transcript::Folio::panels`] gather them into one panel, the way a
/// tool result is gathered into the call it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firing {
    /// The harness's `toolUseID`, which names the *event* and not the hook: one
    /// event runs every hook matching it, so four `SessionStart` hooks share
    /// this.
    event: String,
    /// The hook's own command, which is what tells one hook from another within
    /// an event. The lines a hook writes *through* the harness (what it decided,
    /// what it injected) carry no command of their own, which is what lets them
    /// join whichever hook of the event they turn up beside.
    command: Option<String>,
}

impl Firing {
    /// True when two notes could have come from one hook: the same event, and
    /// no two *different* commands between them. Two named commands under one
    /// event are two hooks and must stay two panels, or the folio claims a
    /// single hook ran four commands.
    fn joins(&self, other: &Firing) -> bool {
        self.event == other.event
            && match (&self.command, &other.command) {
                (Some(mine), Some(theirs)) => mine == theirs,
                _ => true,
            }
    }
}

/// One note the harness wrote into the session, as data: the markup comes
/// later, from [`setting`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gloss {
    pub kind: GlossKind,
    /// What the note is about: the hook that ran, the file pulled in, the
    /// command typed.
    pub gist: Option<String>,
    /// Qualifiers the gist doesn't carry: a non-zero exit, a timeout, the
    /// arguments a command took.
    pub notes: Vec<String>,
    /// What the note has to say, in the order it was said. One firing of a hook
    /// can write more than one, so this is a list rather than the one body a
    /// single line carries.
    pub body: Vec<Body>,
    pub firing: Option<Firing>,
}

impl Gloss {
    fn new(kind: GlossKind) -> Self {
        Self {
            kind,
            gist: None,
            notes: Vec::new(),
            body: Vec::new(),
            firing: None,
        }
    }

    /// Names the firing this note belongs to.
    fn firing(mut self, event: Option<&str>, command: Option<&str>) -> Self {
        self.firing = event.map(|event| Firing {
            event: event.to_owned(),
            command: command.map(str::to_owned),
        });
        self
    }

    /// Takes in another note from the same hook firing. The first line a hook
    /// writes says what it decided and the next carries what it injected, so
    /// what is already set labels the panel and the incoming note fills the
    /// gaps. Nothing it had to say is dropped: the bodies stack in the fold, and
    /// a named command narrows the firing so a *third* hook of the same event
    /// can no longer join what is now identified.
    pub(crate) fn absorb(&mut self, other: Gloss) {
        if self.gist.is_none() {
            self.gist = other.gist;
        }
        self.body.extend(other.body);
        for note in other.notes {
            if !self.notes.contains(&note) {
                self.notes.push(note);
            }
        }
        if let (Some(firing), Some(joined)) = (&mut self.firing, other.firing)
            && firing.command.is_none()
        {
            firing.command = joined.command;
        }
    }

    /// Re-reads this note as the instructions of the slash command that ran it.
    ///
    /// [`meta`] knows a skill by the directory it opens on, and a built-in one
    /// has no directory on disk to name, so its instructions arrive as bare
    /// prose and fall through to the catch-all. The command standing directly in
    /// front of them is what says otherwise. Naming it after that command is
    /// what makes a built-in read as the same thing a skill with a directory
    /// does, and as the same thing again when the model reaches for one with no
    /// command at all: one shape for "a skill was loaded", however it was.
    ///
    /// A note with no body is the whole of what was said on its gist alone, so
    /// that text becomes the body rather than being dropped by the name taking
    /// its place.
    pub(crate) fn ran_by(&mut self, command: &str) {
        self.kind = GlossKind::Skill;
        if self.body.is_empty()
            && let Some(said) = self.gist.take()
        {
            self.body.push(Body::Prose(said));
        }
        self.gist = Some(command.trim_start_matches('/').to_owned());
    }

    /// True when this note and the next belong to the same firing of one hook,
    /// so the folio should set them as one panel rather than two.
    pub(crate) fn same_firing_as(&self, other: &Gloss) -> bool {
        self.kind == GlossKind::Hook
            && other.kind == GlossKind::Hook
            && match (&self.firing, &other.firing) {
                (Some(mine), Some(theirs)) => mine.joins(theirs),
                _ => false,
            }
    }

    fn gist(mut self, gist: impl Into<String>) -> Self {
        self.gist = Some(gist.into());
        self
    }

    fn maybe_gist(self, gist: Option<impl Into<String>>) -> Self {
        match gist {
            Some(gist) => self.gist(gist),
            None => self,
        }
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    fn maybe_note(self, note: Option<impl Into<String>>) -> Self {
        match note {
            Some(note) => self.note(note),
            None => self,
        }
    }

    fn prose(mut self, body: impl Into<String>) -> Self {
        self.body.push(Body::Prose(body.into()));
        self
    }

    fn printed(mut self, body: impl Into<String>) -> Self {
        self.body.push(Body::Printed(body.into()));
        self
    }

    fn plain(mut self, body: impl Into<String>) -> Self {
        self.body.push(Body::Plain(body.into()));
        self
    }

    fn maybe_plain(self, body: Option<impl Into<String>>) -> Self {
        match body {
            Some(body) => self.plain(body),
            None => self,
        }
    }

    fn maybe_prose(self, body: Option<impl Into<String>>) -> Self {
        match body {
            Some(body) => self.prose(body),
            None => self,
        }
    }

    /// True when the note says nothing a reader could act on. Such a gloss is
    /// dropped rather than set as a bare line reporting that something ran.
    fn is_bare(&self) -> bool {
        self.gist.is_none() && self.body.is_empty()
    }
}

/// How a gloss is set: the same summary-line-and-fold vocabulary a tool call
/// takes, so the two read as one kind of thing on the page. A firing that had
/// several things to say stacks them in the one fold, in the order it said them.
pub fn setting(scribe: &Scribe, gloss: &Gloss) -> Setting {
    let setting = Setting::new().maybe_gist(gloss.gist.as_deref());
    let setting = gloss
        .notes
        .iter()
        .fold(setting, |setting, note| setting.note(note.as_str()));
    if gloss.body.is_empty() {
        return setting;
    }
    setting.body(html! {
        @for body in &gloss.body {
            (match body {
                Body::Prose(text) => prose(scribe, text),
                Body::Printed(text) => printed(scribe, text),
                Body::Plain(text) => plain(scribe, text),
            })
        }
    })
}

/// The note an `attachment` line carries, or `None` when it is scaffolding: an
/// inventory of the tools, skills, or agents available, a restatement of the
/// checklist a `TodoWrite` already shows, or a change of date the timestamps
/// already say.
pub fn attachment(attachment: &Value) -> Option<Gloss> {
    let gloss = match text(attachment, "type")? {
        "hook_success" => hook_output(attachment)?,
        "hook_system_message" => Gloss::new(GlossKind::Hook)
            .firing(text(attachment, "toolUseID"), None)
            .maybe_gist(text(attachment, "content"))
            .maybe_note(text(attachment, "hookName")),
        // A hook printed this, so its line breaks are its own: markdown folds a
        // single newline into a space, and `M  CLAUDE.md\nM  src/gloss.rs` came
        // out as one run of filenames, which is the shape a hook reporting on a
        // working tree always takes. It is still markdown often enough that
        // setting it as preformatted text would throw away headings and lists
        // the corpus is full of, so it takes the middle reading.
        "hook_additional_context" => Gloss::new(GlossKind::Hook)
            .firing(text(attachment, "toolUseID"), None)
            .maybe_gist(text(attachment, "hookName"))
            .note("added context")
            .printed(paragraphs(attachment.get("content")?)?),
        "hook_cancelled" => Gloss::new(GlossKind::Hook)
            .firing(text(attachment, "toolUseID"), text(attachment, "command"))
            .maybe_gist(text(attachment, "hookName"))
            .maybe_note(text(attachment, "command"))
            .note(
                if flag(attachment, "timedOut") {
                    "timed out"
                } else {
                    "cancelled"
                },
            ),
        // Only the rules and `CLAUDE.md` files the harness pulls in *during* a
        // session are recorded, one attachment each: what a session starts with
        // (the project's own `CLAUDE.md`, the user's global one, every rule with
        // no path to trigger on) is assembled into the system prompt, which the
        // transcript does not carry at all. So a folio can only ever set the
        // instructions a session reached for as it worked, and never the ones it
        // began with.
        "nested_memory" => Gloss::new(GlossKind::Rule)
            .maybe_gist(subject(attachment))
            .maybe_note(text(attachment.get("content")?, "type").map(str::to_lowercase))
            .prose(text(attachment.get("content")?, "content")?),
        "plan_mode" => plan_entered(attachment)?,
        "plan_mode_exit" => plan_left(
            attachment,
            if flag(attachment, "planExists") {
                "left, plan written"
            } else {
                "left, no plan written"
            },
        ),
        // A file the user changed in their own editor while the session ran:
        // the code the assistant went on to read is not the code it had seen.
        "edited_text_file" => Gloss::new(GlossKind::Note)
            .maybe_gist(text(attachment, "filename"))
            .note("edited outside the session")
            .plain(text(attachment, "snippet")?),
        "file" => Gloss::new(GlossKind::Note)
            .maybe_gist(subject(attachment))
            .note("attached")
            .prose(text(attachment.get("content")?.get("file")?, "content")?),
        "directory" => Gloss::new(GlossKind::Note)
            .maybe_gist(subject(attachment))
            .note("attached")
            .plain(text(attachment, "content")?),
        // Only part of a file reached the assistant, which is why what followed
        // may not answer to the whole of it.
        "read_truncation_notice" => Gloss::new(GlossKind::Note)
            .gist("read truncated")
            .plain(text(attachment, "banner")?),
        "goal_status" => Gloss::new(GlossKind::Note)
            .maybe_gist(text(attachment, "condition"))
            .note(
                if flag(attachment, "met") {
                    "met"
                } else {
                    "not met"
                },
            ),
        _ => return None,
    };
    (!gloss.is_bare()).then_some(gloss)
}

/// What a hook's own output has to say.
///
/// `content` is what the hook contributed to the session and `stdout` is what
/// it printed, and for a hook that simply prints context they are the same text
/// twice. A hook that instead answers in the control protocol prints JSON that
/// contributes nothing itself: what it asked for arrives as the
/// `hook_system_message` and `hook_additional_context` notes beside it, so
/// setting the protocol here would say the same thing a second time in a form
/// no reader wants. `stderr` never reaches the model at all, and is the hook
/// talking to whoever is watching.
fn hook_output(attachment: &Value) -> Option<Gloss> {
    let contributed = text(attachment, "content")
        .or_else(|| text(attachment, "stdout").filter(|stdout| !is_protocol(stdout)))
        .map(str::trim)
        .filter(|contributed| !contributed.is_empty());
    let printed = text(attachment, "stderr")
        .map(str::trim)
        .filter(|printed| !printed.is_empty());
    let said: Vec<&str> = contributed.into_iter().chain(printed).collect();
    if said.is_empty() {
        return None;
    }
    let exit = attachment.get("exitCode").and_then(Value::as_i64);
    Some(
        Gloss::new(GlossKind::Hook)
            .firing(text(attachment, "toolUseID"), text(attachment, "command"))
            .maybe_gist(text(attachment, "hookName"))
            .maybe_note(text(attachment, "command"))
            .maybe_note(
                exit.filter(|code| *code != 0)
                    .map(|code| format!("exit {code}")),
            )
            .plain(said.join("\n")),
    )
}

/// Commands that work the harness rather than the conversation: copying the last
/// reply, listing what is installed, signing in, picking a session to resume. A
/// reader learns nothing from having been told one ran, and what it printed was
/// for whoever was sitting at the terminal.
///
/// This is a list rather than a rule because the transcript records every slash
/// command the same way, whether it injected a whole skill or only redrew the
/// screen. Match a new one against real transcripts before adding it, the way
/// [`crate::tools::acknowledges`] is matched: dropping a command that *did*
/// inject something loses the reason a session changed course. Commands that
/// change the conversation stay, even the terse ones: `/compact` rewrites the
/// context and `/model` changes who answers.
///
/// `/clear` is not here. It is a session boundary rather than a command with
/// nothing to say, and [`crate::transcript::Turn::is_clear_command`] drops it
/// before this is ever reached.
const HARNESS_CONTROLS: &[&str] = &[
    "/copy",
    "/skills",
    "/agents",
    "/login",
    "/logout",
    "/resume",
    "/config",
    "/status",
    "/cost",
    "/doctor",
    "/help",
    "/terminal-setup",
];

fn controls_the_harness(name: &str) -> bool {
    HARNESS_CONTROLS.contains(&name)
}

/// True for hook output written in the control protocol rather than addressed
/// to the session: a JSON object asking the harness to say or inject something,
/// which arrives as its own note.
fn is_protocol(stdout: &str) -> bool {
    serde_json::from_str::<Value>(stdout.trim()).is_ok_and(|value| {
        value.get("systemMessage").is_some() || value.get("hookSpecificOutput").is_some()
    })
}

/// Leaving plan mode, named by the plan's own file the way a rule or a skill is
/// named by its path: the subject goes on the summary line and what happened to
/// it qualifies, so a long path wraps where a short phrase would be crushed. The
/// file is worth naming here and not on the way in, because a session that
/// writes its plan writes it with an ordinary tool call the folio already sets,
/// and the path is what ties this boundary to that panel. A boundary the harness
/// recorded no path for is named by what happened instead, since a gloss with
/// neither gist nor body is dropped.
fn plan_left(attachment: &Value, state: &str) -> Gloss {
    match text(attachment, "planFilePath") {
        Some(path) => Gloss::new(GlossKind::Plan).gist(path).note(state),
        None => Gloss::new(GlossKind::Plan).gist(state),
    }
}

/// Entering plan mode, which is the whole of what the boundary says. The plan
/// file the harness names here is not set: nothing has been written to it yet,
/// and a reader holding only the folio can't open it in any case.
///
/// The harness repeats the note while the mode stays on, so only the one that
/// marks the boundary is set: the reminders after it say nothing that has
/// changed.
fn plan_entered(attachment: &Value) -> Option<Gloss> {
    (text(attachment, "reminderType") == Some("full")).then(|| {
        Gloss::new(GlossKind::Plan)
            .gist("entered plan mode")
            .maybe_note(flag(attachment, "isSubAgent").then_some("subagent"))
    })
}

/// The note a `system` line carries, or `None` for the ones that say nothing a
/// reader needs: how long a turn took, and a summary of the hooks whose own
/// notes sit beside it.
pub fn system(system: &Value) -> Option<Gloss> {
    let said = text(system, "content").map(str::trim).unwrap_or_default();
    let gloss = match text(system, "subtype")? {
        "local_command" => {
            let printed = unwrap_tag(said, "local-command-stdout")?.trim();
            (!printed.is_empty())
                .then(|| Gloss::new(GlossKind::Command).gist("output").plain(printed))?
        }
        // A message was retracted and the model swapped under the conversation,
        // which is why the reply that follows came from a different one.
        "model_refusal_fallback" => Gloss::new(GlossKind::Note)
            .gist("model switched")
            .maybe_note(
                text(system, "originalModel")
                    .zip(text(system, "fallbackModel"))
                    .map(|(from, to)| format!("{from} → {to}")),
            )
            .prose(said),
        "informational" | "away_summary" => told(GlossKind::Note, said),
        _ => return None,
    };
    (!gloss.is_bare()).then_some(gloss)
}

/// What a `user` turn carries once the wrappers the harness records around it
/// are accounted for. A turn in the user's role is not always the user: the
/// harness writes its own notes there, and wraps what the user typed in
/// scaffolding addressed to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrapped {
    /// A note the harness wrote, to be set as a gloss.
    Note(Gloss),
    /// A wrapper with nothing under it, to be dropped: the caveat standing in
    /// front of a slash command is its own turn, and says only that what
    /// follows isn't addressed to the model.
    Nothing,
}

/// What a `user` turn really is, or `None` when it is the user speaking and
/// belongs in the conversation as it stands.
pub fn wrapped(said: &str, is_meta: bool) -> Option<Wrapped> {
    // The caveat is addressed to the model, telling it not to answer what
    // follows; only what it wraps is the reader's business.
    let said = strip_tag(said, "local-command-caveat");
    let said = said.trim();
    if let Some(wrapped) = command(said) {
        return Some(wrapped);
    }
    if !is_meta {
        return None;
    }
    Some(match meta(said) {
        Some(gloss) => Wrapped::Note(gloss),
        None => Wrapped::Nothing,
    })
}

/// The note a `user` turn the harness wrote for itself carries. A skill (or a
/// slash command standing on one) is injected as its base directory followed by
/// the whole of what it instructs, which is the most load-bearing context a
/// session has and the thing the folio hid outright; anything else the harness
/// says to itself is set as it came.
fn meta(said: &str) -> Option<Gloss> {
    const SKILL_OPENING: &str = "Base directory for this skill:";
    // How an image was scaled on its way to the model, so it can map what it
    // sees back onto the original. The image is set beside this note and shows
    // a reader everything the note is about.
    const IMAGE_SCALING: (&str, &str) = ("[Image: original ", "Multiply coordinates");

    if said.is_empty() || (said.starts_with(IMAGE_SCALING.0) && said.contains(IMAGE_SCALING.1)) {
        return None;
    }
    let Some(rest) = said.strip_prefix(SKILL_OPENING) else {
        return Some(told(GlossKind::Note, said));
    };
    // The base directory is its own line, and everything under it is what the
    // skill instructs. A skill recorded as that line alone still names itself.
    let (directory, instructions) = rest.trim_start().split_once('\n').unwrap_or((rest, ""));
    Some(
        Gloss::new(GlossKind::Skill)
            .maybe_gist(directory.trim().rsplit('/').next())
            .maybe_prose(Some(instructions.trim_start()).filter(|body| !body.is_empty())),
    )
}

/// The slash command a `user` turn stands for. The harness records the command
/// as a wrapper around the name, the arguments, and whatever the command
/// printed, and the tags arrive in different orders across harness versions, so
/// each is taken where it sits rather than by splitting the text up.
/// Answers three ways, and the third is what keeps a dropped command from
/// reading as the user speaking: `Nothing` for one that only works the harness,
/// `Note` for one that shaped the conversation, and `None` when the turn is no
/// command at all.
fn command(said: &str) -> Option<Wrapped> {
    let name = unwrap_tag(said, "command-name")?.trim();
    if name.is_empty() {
        return None;
    }
    if controls_the_harness(name) {
        return Some(Wrapped::Nothing);
    }
    let args = unwrap_tag(said, "command-args").unwrap_or_default().trim();
    let printed = unwrap_tag(said, "local-command-stdout")
        .unwrap_or_default()
        .trim();
    Some(Wrapped::Note(
        Gloss::new(GlossKind::Command)
            .gist(name)
            .maybe_note((!args.is_empty()).then_some(args))
            .maybe_plain((!printed.is_empty()).then_some(printed)),
    ))
}

/// Something the harness stated in prose. A single short line is the whole of
/// what it says, so it goes on the summary line with no fold to open; anything
/// longer is titled by its opening line and held in the fold.
fn told(kind: GlossKind, said: &str) -> Gloss {
    const AT_A_GLANCE: usize = 90;

    let opening = said.lines().next().unwrap_or(said).trim();
    if said.lines().count() == 1 && said.chars().count() <= AT_A_GLANCE {
        return Gloss::new(kind).gist(opening);
    }
    Gloss::new(kind).gist(opening).prose(said)
}

/// The path a note is about, preferring the one written for a reader.
fn subject(attachment: &Value) -> Option<&str> {
    ["displayPath", "path", "filename"]
        .iter()
        .find_map(|field| text(attachment, field))
}

/// Several pieces of injected context as one document, or `None` when the
/// harness recorded something other than a list of them.
fn paragraphs(content: &Value) -> Option<String> {
    let joined = content
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<&str>>()
        .join("\n\n");
    (!joined.trim().is_empty()).then_some(joined)
}

/// What one of the harness's XML-ish wrappers encloses, or `None` when the text
/// carries no such wrapper.
fn unwrap_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let opening = format!("<{tag}>");
    let closing = format!("</{tag}>");
    let from = text.find(&opening)? + opening.len();
    let to = text[from..].find(&closing)? + from;
    Some(&text[from..to])
}

/// The text with one of the harness's wrappers, and everything it encloses,
/// taken out of it.
fn strip_tag<'a>(text: &'a str, tag: &str) -> Cow<'a, str> {
    let closing = format!("</{tag}>");
    let Some(from) = text.find(&format!("<{tag}>")) else {
        return Cow::Borrowed(text);
    };
    let Some(to) = text[from..]
        .find(&closing)
        .map(|at| at + from + closing.len())
    else {
        return Cow::Borrowed(text);
    };
    Cow::Owned(format!("{}{}", &text[..from], &text[to..]))
}

fn text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field)?.as_str()
}

fn flag(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The note a command wrapper makes, for the tests that are about the
    /// command rather than about whether it is one.
    fn noted(said: &str) -> Option<Gloss> {
        match command(said) {
            Some(Wrapped::Note(gloss)) => Some(gloss),
            _ => None,
        }
    }

    #[test]
    fn a_slash_command_is_read_out_of_its_wrapper() {
        let gloss = noted(
            "<command-message>debug-gha</command-message>\n\
             <command-name>/debug-gha</command-name>",
        )
        .expect("the wrapper names a command");

        assert_eq!(gloss.kind, GlossKind::Command);
        assert_eq!(gloss.gist.as_deref(), Some("/debug-gha"));
        assert!(gloss.notes.is_empty());
        assert!(gloss.body.is_empty());
    }

    #[test]
    fn a_command_carries_its_arguments_and_what_it_printed() {
        // The tags arrive in either order across harness versions, so this one
        // leads with the name where the test above leads with the message.
        let gloss = noted(
            "<command-name>/loop</command-name>\n\
             <command-message>loop</command-message>\n\
             <command-args>5m /babysit-prs</command-args>\n\
             <local-command-stdout>scheduled</local-command-stdout>",
        )
        .expect("the wrapper names a command");

        assert_eq!(gloss.gist.as_deref(), Some("/loop"));
        assert_eq!(gloss.notes, ["5m /babysit-prs"]);
        assert_eq!(gloss.body, [Body::Plain("scheduled".into())]);
    }

    #[test]
    fn a_command_that_only_controls_the_harness_is_not_set() {
        // `/copy` puts the last reply on the clipboard and says so. Neither the
        // command nor what it printed is about the conversation. It answers
        // `Nothing` rather than `None`, or the turn would fall through and be
        // set as the user speaking the wrapper aloud.
        assert_eq!(
            command(
                "<command-name>/copy</command-name>\n\
                 <local-command-stdout>Copied to clipboard (464 characters, 7 lines)\
                 </local-command-stdout>"
            ),
            Some(Wrapped::Nothing)
        );
        assert_eq!(
            command("<command-name>/config</command-name>"),
            Some(Wrapped::Nothing)
        );
    }

    #[test]
    fn a_command_that_changes_the_conversation_is_still_set() {
        // The terse ones are not all alike: compacting rewrites the context the
        // model sees, which is exactly the sort of turn a reader needs to see.
        let gloss = noted("<command-name>/compact</command-name>")
            .expect("a command that changes the conversation is a gloss");

        assert_eq!(gloss.gist.as_deref(), Some("/compact"));
    }

    #[test]
    fn a_command_that_printed_nothing_has_no_fold() {
        let gloss = noted(
            "<command-name>/clear</command-name>\n\
             <command-args></command-args>\n\
             <local-command-stdout></local-command-stdout>",
        )
        .expect("the wrapper names a command");

        assert!(gloss.notes.is_empty());
        assert!(gloss.body.is_empty());
    }

    #[test]
    fn the_caveat_wrapping_a_command_is_not_set() {
        // The caveat is addressed to the model, telling it not to answer what
        // follows; only the command it wraps is the reader's business.
        let said = "<local-command-caveat>Caveat: The messages below were generated \
                    by the user while running local commands.</local-command-caveat>\n\
                    <command-name>/review</command-name>";

        let Some(Wrapped::Note(gloss)) = wrapped(said, true) else {
            panic!("the command should survive the caveat");
        };
        assert_eq!(gloss.kind, GlossKind::Command);
        assert_eq!(gloss.gist.as_deref(), Some("/review"));
    }

    #[test]
    fn a_caveat_with_nothing_left_under_it_is_dropped_rather_than_left_as_a_turn() {
        // The caveat stands in front of a slash command as a turn of its own,
        // so answering "no note here" would leave it to render as the user
        // speaking, which is exactly what it isn't.
        assert_eq!(
            wrapped(
                "<local-command-caveat>Caveat: do not respond.</local-command-caveat>",
                true
            ),
            Some(Wrapped::Nothing)
        );
    }

    #[test]
    fn what_the_user_actually_typed_is_left_in_the_conversation() {
        assert_eq!(wrapped("Does the formatter run on nightly?", false), None);
    }

    #[test]
    fn a_skill_is_named_by_its_directory_and_holds_its_instructions() {
        let gloss = meta(
            "Base directory for this skill: /home/jtk/.claude/skills/debug-gha\n\n\
             # Debug GitHub Actions Runs\n\nThe objective is to **fix** the failing run.",
        )
        .expect("a skill body is a gloss");

        assert_eq!(gloss.kind, GlossKind::Skill);
        assert_eq!(gloss.gist.as_deref(), Some("debug-gha"));
        assert_eq!(
            gloss.body,
            [Body::Prose(
                "# Debug GitHub Actions Runs\n\nThe objective is to **fix** the failing run."
                    .into()
            )]
        );
    }

    #[test]
    fn a_short_harness_note_is_stated_on_the_line_with_no_fold() {
        let gloss = meta("Continue from where you left off").expect("a note is a gloss");

        assert_eq!(gloss.kind, GlossKind::Note);
        assert_eq!(
            gloss.gist.as_deref(),
            Some("Continue from where you left off")
        );
        assert!(gloss.body.is_empty());
    }

    #[test]
    fn how_an_image_was_scaled_for_the_model_is_not_set() {
        // The note tells the model how to map coordinates back onto the
        // original; the image itself sits beside it and shows a reader all of
        // what the note is about.
        assert_eq!(
            meta(
                "[Image: original 1400x2006, displayed at 1396x2000. \
                 Multiply coordinates by 1.00 to map to original image.]"
            ),
            None
        );
    }

    #[test]
    fn a_hook_sets_what_it_contributed_and_what_it_printed_to_stderr() {
        let gloss = attachment(&json!({
            "type": "hook_success",
            "hookName": "SessionStart:clear",
            "command": "claude-branch-journal",
            "content": "",
            "stdout": "",
            "stderr": "No journal yet for branch 'wide-margins'\n",
            "exitCode": 0,
        }))
        .expect("a hook that printed something is a gloss");

        assert_eq!(gloss.kind, GlossKind::Hook);
        assert_eq!(gloss.gist.as_deref(), Some("SessionStart:clear"));
        assert_eq!(gloss.notes, ["claude-branch-journal"]);
        assert_eq!(
            gloss.body,
            [Body::Plain(
                "No journal yet for branch 'wide-margins'".into()
            )]
        );
    }

    #[test]
    fn a_hooks_contribution_is_set_once_though_it_is_recorded_twice() {
        // `content` and `stdout` are the same text: what the hook printed, and
        // what that put into the session.
        let gloss = attachment(&json!({
            "type": "hook_success",
            "hookName": "SessionStart:clear",
            "content": "Current git status:\n\n## wide-margins",
            "stdout": "Current git status:\n\n## wide-margins\n",
            "stderr": "",
            "exitCode": 0,
        }))
        .expect("a hook that contributed context is a gloss");

        assert_eq!(
            gloss.body,
            [Body::Plain("Current git status:\n\n## wide-margins".into())]
        );
    }

    #[test]
    fn a_failing_hook_says_what_it_exited_with() {
        let gloss = attachment(&json!({
            "type": "hook_success",
            "hookName": "PreToolUse:Bash",
            "command": "claude-read-check",
            "stdout": "use the Read tool",
            "exitCode": 2,
        }))
        .expect("a hook that printed something is a gloss");

        assert_eq!(gloss.notes, ["claude-read-check", "exit 2"]);
    }

    #[test]
    fn hook_output_in_the_control_protocol_is_left_to_the_notes_it_asks_for() {
        // The JSON contributes nothing itself: what it asks for arrives as the
        // system-message and additional-context notes beside it.
        assert_eq!(
            attachment(&json!({
                "type": "hook_success",
                "hookName": "Stop",
                "command": "claude-stop",
                "content": "",
                "stdout": "{\"systemMessage\": \"finishing pass\", \
                            \"hookSpecificOutput\": {\"additionalContext\": \"…\"}}",
                "stderr": "",
                "exitCode": 0,
            })),
            None
        );
    }

    #[test]
    fn context_a_hook_injected_is_joined_into_one_document() {
        let gloss = attachment(&json!({
            "type": "hook_additional_context",
            "hookName": "Stop",
            "content": ["first thing", "second thing"],
        }))
        .expect("injected context is a gloss");

        assert_eq!(gloss.gist.as_deref(), Some("Stop"));
        assert_eq!(gloss.notes, ["added context"]);
        // A hook printed this, so its own line breaks are kept: `Printed`, not
        // `Prose`, which would fold a single newline into a space and run a
        // list of files together.
        assert_eq!(
            gloss.body,
            [Body::Printed("first thing\n\nsecond thing".into())]
        );
    }

    #[test]
    fn a_rule_pulled_into_context_is_named_by_the_path_written_for_a_reader() {
        let gloss = attachment(&json!({
            "type": "nested_memory",
            "path": "/home/jtk/dotfiles/claude/rules/rust.md",
            "displayPath": "claude/rules/rust.md",
            "content": {
                "path": "/home/jtk/dotfiles/claude/rules/rust.md",
                "type": "User",
                "content": "# Rust Style Guide\n\nUse the latest stable edition.",
            },
        }))
        .expect("a memory file is a gloss");

        assert_eq!(gloss.kind, GlossKind::Rule);
        assert_eq!(gloss.gist.as_deref(), Some("claude/rules/rust.md"));
        assert_eq!(gloss.notes, ["user"]);
        assert_eq!(
            gloss.body,
            [Body::Prose(
                "# Rust Style Guide\n\nUse the latest stable edition.".into()
            )]
        );
    }

    #[test]
    fn plan_mode_marks_its_boundaries_and_says_whether_a_plan_was_written() {
        let entered = attachment(&json!({
            "type": "plan_mode",
            "reminderType": "full",
            "isSubAgent": false,
            "planFilePath": "/home/jtk/.claude/plans/wide-margins.md",
            "planExists": false,
        }))
        .expect("entering plan mode is a gloss");
        assert_eq!(entered.kind, GlossKind::Plan);
        // Nothing has been written to the plan file yet, and a reader holding
        // only the folio could not open it, so entering says only that.
        assert_eq!(entered.gist.as_deref(), Some("entered plan mode"));
        assert!(entered.notes.is_empty());

        let written = attachment(&json!({
            "type": "plan_mode_exit",
            "planFilePath": "/home/jtk/.claude/plans/wide-margins.md",
            "planExists": true,
        }))
        .expect("leaving plan mode is a gloss");
        // Leaving does name the file: a session that writes its plan writes it
        // with a tool call the folio sets, so the path ties the two together.
        assert_eq!(
            written.gist.as_deref(),
            Some("/home/jtk/.claude/plans/wide-margins.md")
        );
        assert_eq!(written.notes, ["left, plan written"]);

        let unwritten = attachment(&json!({
            "type": "plan_mode_exit",
            "planFilePath": "/home/jtk/.claude/plans/wide-margins.md",
            "planExists": false,
        }))
        .expect("leaving plan mode is a gloss");
        assert_eq!(unwritten.notes, ["left, no plan written"]);
    }

    #[test]
    fn a_plan_boundary_with_no_file_is_named_by_what_happened() {
        // A gloss with neither gist nor body is dropped, so the phrase has to
        // become the gist when there is no path to stand as the subject.
        let gloss = attachment(&json!({ "type": "plan_mode_exit", "planExists": false }))
            .expect("leaving plan mode is a gloss even with no file named");

        assert_eq!(gloss.gist.as_deref(), Some("left, no plan written"));
        assert!(gloss.notes.is_empty());
    }

    #[test]
    fn a_reminder_that_plan_mode_is_still_on_is_not_a_boundary() {
        assert_eq!(
            attachment(&json!({
                "type": "plan_mode",
                "reminderType": "sparse",
                "planFilePath": "/home/jtk/.claude/plans/wide-margins.md",
                "planExists": true,
            })),
            None
        );
    }

    #[test]
    fn a_file_edited_outside_the_session_is_set() {
        let gloss = attachment(&json!({
            "type": "edited_text_file",
            "filename": "/home/jtk/projects/scriptorium/src/gloss.rs",
            "snippet": "9\tfn setting() {}",
        }))
        .expect("an external edit is a gloss");

        assert_eq!(gloss.kind, GlossKind::Note);
        assert_eq!(gloss.notes, ["edited outside the session"]);
        assert_eq!(gloss.body, [Body::Plain("9\tfn setting() {}".into())]);
    }

    #[test]
    fn scaffolding_the_reader_cannot_act_on_is_not_set() {
        for kind in [
            "task_reminder",
            "skill_listing",
            "deferred_tools_delta",
            "agent_listing_delta",
            "command_permissions",
            "date_change",
        ] {
            assert_eq!(
                attachment(&json!({ "type": kind, "content": "…" })),
                None,
                "{kind} should stay unset"
            );
        }
    }

    #[test]
    fn a_local_commands_output_is_set_and_an_empty_one_is_not() {
        let gloss = system(&json!({
            "subtype": "local_command",
            "content": "<local-command-stdout>Copied to clipboard</local-command-stdout>",
        }))
        .expect("a command that printed something is a gloss");

        assert_eq!(gloss.kind, GlossKind::Command);
        assert_eq!(gloss.body, [Body::Plain("Copied to clipboard".into())]);

        assert_eq!(
            system(&json!({
                "subtype": "local_command",
                "content": "<local-command-stdout></local-command-stdout>",
            })),
            None
        );
    }

    #[test]
    fn a_model_swapped_under_the_conversation_is_set() {
        let gloss = system(&json!({
            "subtype": "model_refusal_fallback",
            "originalModel": "claude-fable-5",
            "fallbackModel": "claude-opus-4-8",
            "content": "The safeguards flagged this message.",
        }))
        .expect("a fallback is a gloss");

        assert_eq!(gloss.gist.as_deref(), Some("model switched"));
        assert_eq!(gloss.notes, ["claude-fable-5 → claude-opus-4-8"]);
    }

    #[test]
    fn a_systems_own_bookkeeping_is_not_set() {
        for subtype in ["turn_duration", "stop_hook_summary"] {
            assert_eq!(
                system(&json!({ "subtype": subtype, "content": "…" })),
                None,
                "{subtype} should stay unset"
            );
        }
    }
}
